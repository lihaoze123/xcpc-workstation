//! Lightweight, zero-overhead-when-disabled performance trace collector.
//!
//! Produces Chrome Trace Event Format (compatible with Perfetto UI and
//! `chrome://tracing`) and supports merging Rust Core spans with JS/UI spans
//! onto a single monotonic microsecond timeline.

use std::borrow::Cow;
use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Hard cap on buffered events so long recordings never grow unboundedly.
const MAX_EVENTS: usize = 200_000;
/// Hard cap on always-on structured logs in the in-memory ring buffer.
const MAX_LOGS: usize = 5_000;
/// Hard cap on always-on slow/error events in the in-memory ring buffer.
const MAX_SLOW_EVENTS: usize = 1_000;

/// Chrome Trace process IDs for clear lane separation in Perfetto.
pub const PID_RUST: u32 = 1;
pub const PID_JS: u32 = 2;

/// Single-atomic state: `0 = uninitialized`, `1 = disabled`, `2 = enabled`.
const STATE_UNINIT: u8 = 0;
const STATE_OFF: u8 = 1;
const STATE_ON: u8 = 2;

static STATE: AtomicU8 = AtomicU8::new(STATE_UNINIT);
static EPOCH: OnceLock<Instant> = OnceLock::new();
static NEXT_TID: AtomicU64 = AtomicU64::new(1);
static NEXT_EVENT_SEQ: AtomicU64 = AtomicU64::new(1);
static NEXT_LOG_SEQ: AtomicU64 = AtomicU64::new(1);

thread_local! {
    static THREAD_NUM: u64 = NEXT_TID.fetch_add(1, Ordering::Relaxed);
}

#[derive(Debug, Clone, Serialize)]
pub struct TraceEvent {
    #[serde(skip)]
    pub seq: u64,
    pub name: Cow<'static, str>,
    pub cat: Cow<'static, str>,
    pub ph: &'static str,
    pub ts: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dur: Option<f64>,
    pub pid: u32,
    pub tid: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<Value>,
}

/// Structured log severity level (matches `nsh-o11y` `LogLevel`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

/// One structured log entry in the always-on in-memory ring buffer.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogRecord {
    pub seq: u64,
    pub time_unix_ms: u64,
    pub ts_us: f64,
    pub level: LogLevel,
    pub subsystem: Cow<'static, str>,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attrs: Option<Value>,
}

/// Incremental trace event exported to the live diagnostics window.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct O11yEvent {
    pub seq: u64,
    pub name: String,
    pub cat: String,
    pub ph: &'static str,
    pub ts_us: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dur_us: Option<f64>,
    pub pid: u32,
    pub tid: u64,
    pub thread_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<Value>,
}

/// Incremental snapshot polled by the diagnostics window every ~150-250ms.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct O11yDelta {
    pub enabled: bool,
    pub o11y_env: bool,
    pub rust_now_us: f64,
    pub now_unix_ms: u64,
    pub event_seq: u64,
    pub log_seq: u64,
    pub slow_count: usize,
    pub dropped_events: u64,
    pub dropped_logs: u64,
    pub events: Vec<O11yEvent>,
    pub logs: Vec<LogRecord>,
}

/// Event pushed from the JS/UI collector before timeline merge.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JsTraceEvent {
    pub name: String,
    pub cat: String,
    /// `"X"` (complete), `"i"` (instant), or `"C"` (counter).
    pub ph: String,
    /// JS `performance.now()` timestamp in milliseconds.
    pub ts_ms: f64,
    /// Optional duration in milliseconds (for `"X"` events).
    #[serde(default)]
    pub dur_ms: Option<f64>,
    /// Optional lane ID (defaults to 1 = UI Main).
    #[serde(default = "default_js_tid")]
    pub tid: u64,
    #[serde(default)]
    pub args: Option<Value>,
}

fn default_js_tid() -> u64 {
    1
}

/// Runtime status returned to the desktop UI / diagnostics.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceStatus {
    pub enabled: bool,
    pub event_count: usize,
    pub dropped_count: u64,
    pub rust_now_us: f64,
}

#[derive(Default)]
struct Buffer {
    events: VecDeque<TraceEvent>,
    dropped: u64,
    thread_names: HashMap<u64, String>,
    logs: VecDeque<LogRecord>,
    dropped_logs: u64,
    slow_events: VecDeque<O11yEvent>,
}

static BUFFER: OnceLock<Mutex<Buffer>> = OnceLock::new();

fn buffer() -> &'static Mutex<Buffer> {
    BUFFER.get_or_init(|| Mutex::new(Buffer::default()))
}

fn epoch() -> Instant {
    *EPOCH.get_or_init(Instant::now)
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Returns true when the `XCPC_O11Y` environment variable requests opening the diagnostics window.
pub fn o11y_env_enabled() -> bool {
    env_flag_enabled("XCPC_O11Y")
}

fn ensure_init() -> u8 {
    let _ = epoch();
    let initial = if env_flag_enabled("XCPC_TRACE") || o11y_env_enabled() {
        STATE_ON
    } else {
        STATE_OFF
    };
    match STATE.compare_exchange(STATE_UNINIT, initial, Ordering::AcqRel, Ordering::Acquire) {
        Ok(_) => initial,
        Err(actual) => actual,
    }
}

fn env_flag_enabled(var: &str) -> bool {
    matches!(
        std::env::var(var).ok().as_deref().map(str::trim),
        Some("1" | "true" | "TRUE" | "yes" | "on")
    )
}

fn current_tid() -> u64 {
    THREAD_NUM.with(|id| *id)
}

/// Current monotonic timestamp in microseconds since the process trace epoch.
#[inline]
pub fn now_us() -> f64 {
    if STATE.load(Ordering::Relaxed) == STATE_UNINIT {
        ensure_init();
    }
    epoch().elapsed().as_secs_f64() * 1_000_000.0
}

/// True 1-load fast-path check: a single relaxed `AtomicU8` load when initialized.
#[inline]
pub fn is_enabled() -> bool {
    match STATE.load(Ordering::Relaxed) {
        STATE_OFF => false,
        STATE_ON => true,
        _ => ensure_init() == STATE_ON,
    }
}

/// Starts (or restarts) a recording session, clearing any previously buffered events.
pub fn start() -> TraceStatus {
    let _ = epoch();
    if let Ok(mut buf) = buffer().lock() {
        buf.events.clear();
        buf.dropped = 0;
        buf.thread_names.clear();
    }
    STATE.store(STATE_ON, Ordering::Release);
    status()
}

/// Stops recording without clearing buffered events so they can be inspected or exported.
pub fn stop() -> TraceStatus {
    let _ = epoch();
    STATE.store(STATE_OFF, Ordering::Release);
    status()
}

/// Clears buffered trace events without changing the enabled state or always-on logs.
pub fn clear() {
    if let Ok(mut buf) = buffer().lock() {
        buf.events.clear();
        buf.dropped = 0;
    }
}

/// Clears all buffered trace events, always-on logs, and slow events.
pub fn clear_all() {
    if let Ok(mut buf) = buffer().lock() {
        buf.events.clear();
        buf.dropped = 0;
        buf.logs.clear();
        buf.dropped_logs = 0;
        buf.slow_events.clear();
    }
}

/// Records a structured log entry into the always-on in-memory ring buffer.
///
/// Never writes to disk and never blocks on I/O. Safe to call even when live
/// tracing is disabled.
pub fn log(
    level: LogLevel,
    subsystem: &'static str,
    message: impl Into<String>,
    attrs: Option<Value>,
) {
    let seq = NEXT_LOG_SEQ.fetch_add(1, Ordering::Relaxed);
    let rec = LogRecord {
        seq,
        time_unix_ms: now_unix_ms(),
        ts_us: now_us(),
        level,
        subsystem: Cow::Borrowed(subsystem),
        message: message.into(),
        trace_id: None,
        span_id: None,
        attrs,
    };
    if let Ok(mut buf) = buffer().lock() {
        if buf.logs.len() >= MAX_LOGS {
            buf.logs.pop_front();
            buf.dropped_logs += 1;
        }
        buf.logs.push_back(rec);
    }
}

#[inline]
pub fn log_debug(subsystem: &'static str, message: impl Into<String>, attrs: Option<Value>) {
    log(LogLevel::Debug, subsystem, message, attrs);
}

#[inline]
pub fn log_info(subsystem: &'static str, message: impl Into<String>, attrs: Option<Value>) {
    log(LogLevel::Info, subsystem, message, attrs);
}

#[inline]
pub fn log_warn(subsystem: &'static str, message: impl Into<String>, attrs: Option<Value>) {
    log(LogLevel::Warn, subsystem, message, attrs);
}

#[inline]
pub fn log_error(subsystem: &'static str, message: impl Into<String>, attrs: Option<Value>) {
    log(LogLevel::Error, subsystem, message, attrs);
}

/// Records a slow or noteworthy operation into the always-on slow-event ring
/// and emits a corresponding `WARN` structured log so it can be inspected later
/// even when the diagnostics window was closed at the time.
pub fn record_slow_event(cat: &'static str, name: &'static str, dur_ms: f64, args: Option<Value>) {
    let tid = current_tid();
    let ts_us = now_us();
    let seq = NEXT_EVENT_SEQ.fetch_add(1, Ordering::Relaxed);
    let tname = std::thread::current()
        .name()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("rust-worker-{tid}"));
    let ev = O11yEvent {
        seq,
        name: name.to_owned(),
        cat: cat.to_owned(),
        ph: "X",
        ts_us: (ts_us - dur_ms * 1000.0).max(0.0),
        dur_us: Some(dur_ms.max(0.0) * 1000.0),
        pid: PID_RUST,
        tid,
        thread_name: tname,
        args: args.clone(),
    };
    if let Ok(mut buf) = buffer().lock() {
        if buf.slow_events.len() >= MAX_SLOW_EVENTS {
            buf.slow_events.pop_front();
        }
        buf.slow_events.push_back(ev);
    }
    let mut log_attrs = match args {
        Some(Value::Object(map)) => map,
        _ => serde_json::Map::new(),
    };
    log_attrs.insert("durMs".to_owned(), json!((dur_ms * 100.0).round() / 100.0));
    log_attrs.insert("op".to_owned(), Value::String(name.to_owned()));
    log(
        LogLevel::Warn,
        cat,
        format!("Slow {name} ({dur_ms:.1} ms)"),
        Some(Value::Object(log_attrs)),
    );
}

/// Polls incremental Rust trace events and structured logs since `(after_event_seq, after_log_seq)`.
pub fn poll_delta(
    after_event_seq: u64,
    after_log_seq: u64,
    max_events: usize,
    max_logs: usize,
) -> O11yDelta {
    let enabled = is_enabled();
    let rust_now_us = now_us();
    let now_unix_ms = now_unix_ms();
    let Ok(buf) = buffer().lock() else {
        return O11yDelta {
            enabled,
            o11y_env: o11y_env_enabled(),
            rust_now_us,
            now_unix_ms,
            event_seq: after_event_seq,
            log_seq: after_log_seq,
            slow_count: 0,
            dropped_events: 0,
            dropped_logs: 0,
            events: Vec::new(),
            logs: Vec::new(),
        };
    };

    let mut events = Vec::new();
    let mut last_event_seq = after_event_seq;
    // Include always-on slow events when live tracing hasn't captured them.
    if after_event_seq == 0 && !buf.slow_events.is_empty() && buf.events.is_empty() {
        for ev in buf.slow_events.iter().take(max_events) {
            if ev.seq > after_event_seq {
                last_event_seq = last_event_seq.max(ev.seq);
                events.push(ev.clone());
            }
        }
    }
    for ev in buf.events.iter() {
        if ev.seq <= after_event_seq {
            continue;
        }
        if events.len() >= max_events {
            break;
        }
        let thread_name = buf
            .thread_names
            .get(&ev.tid)
            .cloned()
            .unwrap_or_else(|| format!("rust-worker-{}", ev.tid));
        last_event_seq = last_event_seq.max(ev.seq);
        events.push(O11yEvent {
            seq: ev.seq,
            name: ev.name.to_string(),
            cat: ev.cat.to_string(),
            ph: ev.ph,
            ts_us: ev.ts,
            dur_us: ev.dur,
            pid: ev.pid,
            tid: ev.tid,
            thread_name,
            args: ev.args.clone(),
        });
    }

    let mut logs = Vec::new();
    let mut last_log_seq = after_log_seq;
    for rec in buf.logs.iter() {
        if rec.seq <= after_log_seq {
            continue;
        }
        if logs.len() >= max_logs {
            break;
        }
        last_log_seq = last_log_seq.max(rec.seq);
        logs.push(rec.clone());
    }

    O11yDelta {
        enabled,
        o11y_env: o11y_env_enabled(),
        rust_now_us,
        now_unix_ms,
        event_seq: last_event_seq,
        log_seq: last_log_seq,
        slow_count: buf.slow_events.len(),
        dropped_events: buf.dropped,
        dropped_logs: buf.dropped_logs,
        events,
        logs,
    }
}

/// Current recorder status and clock reading for Rust↔JS clock calibration.
pub fn status() -> TraceStatus {
    let enabled = is_enabled();
    let (event_count, dropped_count) = buffer()
        .lock()
        .map(|b| (b.events.len(), b.dropped))
        .unwrap_or((0, 0));
    TraceStatus {
        enabled,
        event_count,
        dropped_count,
        rust_now_us: now_us(),
    }
}

fn push_event(mut event: TraceEvent) {
    event.seq = NEXT_EVENT_SEQ.fetch_add(1, Ordering::Relaxed);
    let tid = event.tid;
    if let Ok(mut buf) = buffer().lock() {
        buf.thread_names.entry(tid).or_insert_with(|| {
            std::thread::current()
                .name()
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| format!("rust-worker-{tid}"))
        });
        if buf.events.len() >= MAX_EVENTS {
            buf.events.pop_front();
            buf.dropped += 1;
        }
        buf.events.push_back(event);
    }
}

/// RAII span guard that records a Chrome Trace complete (`"X"`) event on drop.
///
/// When tracing is disabled, `SpanGuard` is a tiny `Option<ActiveSpan>` = `None`
/// and performs zero heap allocations.
pub struct SpanGuard {
    active: Option<ActiveSpan>,
}

struct ActiveSpan {
    cat: &'static str,
    name: &'static str,
    start_us: f64,
    args: Option<Value>,
}

impl Drop for SpanGuard {
    fn drop(&mut self) {
        if let Some(span) = self.active.take() {
            let end_us = now_us();
            let dur = (end_us - span.start_us).max(0.0);
            push_event(TraceEvent {
                seq: 0,
                name: Cow::Borrowed(span.name),
                cat: Cow::Borrowed(span.cat),
                ph: "X",
                ts: span.start_us,
                dur: Some(dur),
                pid: PID_RUST,
                tid: current_tid(),
                args: span.args,
            });
        }
    }
}

/// Begins a span with no extra arguments. Zero-cost when tracing is disabled.
#[inline]
pub fn span(cat: &'static str, name: &'static str) -> SpanGuard {
    if !is_enabled() {
        return SpanGuard { active: None };
    }
    SpanGuard {
        active: Some(ActiveSpan {
            cat,
            name,
            start_us: now_us(),
            args: None,
        }),
    }
}

/// Begins a span whose `args` closure is only evaluated when tracing is enabled.
#[inline]
pub fn span_with(cat: &'static str, name: &'static str, args: impl FnOnce() -> Value) -> SpanGuard {
    if !is_enabled() {
        return SpanGuard { active: None };
    }
    SpanGuard {
        active: Some(ActiveSpan {
            cat,
            name,
            start_us: now_us(),
            args: Some(args()),
        }),
    }
}

/// Records an instant (`"i"`) event if tracing is enabled.
#[inline]
pub fn instant(cat: &'static str, name: &'static str, args: impl FnOnce() -> Value) {
    if !is_enabled() {
        return;
    }
    push_event(TraceEvent {
        seq: 0,
        name: Cow::Borrowed(name),
        cat: Cow::Borrowed(cat),
        ph: "i",
        ts: now_us(),
        dur: None,
        pid: PID_RUST,
        tid: current_tid(),
        args: Some(args()),
    });
}

/// Records a numeric counter (`"C"`) event if tracing is enabled.
#[inline]
pub fn counter(cat: &'static str, name: &'static str, value: f64) {
    if !is_enabled() {
        return;
    }
    push_event(TraceEvent {
        seq: 0,
        name: Cow::Borrowed(name),
        cat: Cow::Borrowed(cat),
        ph: "C",
        ts: now_us(),
        dur: None,
        pid: PID_RUST,
        tid: current_tid(),
        args: Some(json!({ "value": value })),
    });
}

/// Merges buffered Rust events with JS/UI events into a single Chrome Trace JSON string.
///
/// `js_clock_offset_us` satisfies: `rust_ts_us ≈ js_ts_ms * 1000.0 + js_clock_offset_us`.
pub fn export_chrome_json(js_events: &[JsTraceEvent], js_clock_offset_us: f64) -> String {
    let (rust_events, thread_names, dropped) = buffer()
        .lock()
        .map(|b| {
            (
                b.events.iter().cloned().collect::<Vec<_>>(),
                b.thread_names.clone(),
                b.dropped,
            )
        })
        .unwrap_or_default();

    let mut out: Vec<Value> = Vec::with_capacity(rust_events.len() + js_events.len() + 8);

    // Process metadata names for Perfetto lane clarity.
    out.push(json!({
        "name": "process_name",
        "ph": "M",
        "pid": PID_RUST,
        "tid": 0,
        "args": { "name": "Rust Core" }
    }));
    out.push(json!({
        "name": "process_name",
        "ph": "M",
        "pid": PID_JS,
        "tid": 0,
        "args": { "name": "UI / JS" }
    }));
    out.push(json!({
        "name": "thread_name",
        "ph": "M",
        "pid": PID_JS,
        "tid": 1,
        "args": { "name": "UI Main" }
    }));
    out.push(json!({
        "name": "thread_name",
        "ph": "M",
        "pid": PID_JS,
        "tid": 2,
        "args": { "name": "IPC Bridge" }
    }));

    for (tid, tname) in thread_names {
        out.push(json!({
            "name": "thread_name",
            "ph": "M",
            "pid": PID_RUST,
            "tid": tid,
            "args": { "name": tname }
        }));
    }

    for ev in &rust_events {
        if let Ok(v) = serde_json::to_value(ev) {
            out.push(v);
        }
    }

    for ev in js_events {
        let ph = match ev.ph.as_str() {
            "X" | "i" | "C" => ev.ph.as_str(),
            _ => "i",
        };
        let ts = (ev.ts_ms * 1000.0 + js_clock_offset_us).max(0.0);
        let mut obj = serde_json::Map::new();
        obj.insert("name".to_owned(), Value::String(ev.name.clone()));
        obj.insert("cat".to_owned(), Value::String(ev.cat.clone()));
        obj.insert("ph".to_owned(), Value::String(ph.to_owned()));
        obj.insert("ts".to_owned(), json!(ts));
        if ph == "X" {
            let dur_us = ev.dur_ms.unwrap_or(0.0).max(0.0) * 1000.0;
            obj.insert("dur".to_owned(), json!(dur_us));
        }
        obj.insert("pid".to_owned(), json!(PID_JS));
        obj.insert("tid".to_owned(), json!(ev.tid));
        if let Some(args) = &ev.args {
            obj.insert("args".to_owned(), args.clone());
        }
        out.push(Value::Object(obj));
    }

    let exported_at_unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let root = json!({
        "displayTimeUnit": "ms",
        "traceEvents": out,
        "metadata": {
            "app": "XCPC Workstation",
            "exportedAtUnixMs": exported_at_unix_ms,
            "rustEvents": rust_events.len(),
            "jsEvents": js_events.len(),
            "droppedEvents": dropped,
        }
    });

    serde_json::to_string(&root).unwrap_or_else(|_| "{}".to_owned())
}

/// Writes the merged Chrome Trace JSON atomically (via temp file + rename) to `path`.
pub fn export_to_file(
    path: &Path,
    js_events: &[JsTraceEvent],
    js_clock_offset_us: f64,
) -> std::io::Result<usize> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    let payload = export_chrome_json(js_events, js_clock_offset_us);
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, payload.as_bytes())?;
    std::fs::rename(&tmp, path)?;
    Ok(payload.len())
}

/// If `XCPC_TRACE_OUT` is set in the environment, flushes current Rust trace events to it.
pub fn flush_if_env_configured() {
    if let Ok(out) = std::env::var("XCPC_TRACE_OUT") {
        let trimmed = out.trim();
        if !trimmed.is_empty() {
            let _ = export_to_file(Path::new(trimmed), &[], 0.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn disabled_fast_path_records_nothing() {
        let _guard = TEST_LOCK.lock().unwrap();
        stop();
        clear();
        {
            let _g = span("test", "noop");
            let mut evaluated = false;
            let _g2 = span_with("test", "noop_args", || {
                evaluated = true;
                json!({ "k": 1 })
            });
            assert!(!evaluated, "args closure must not run when disabled");
        }
        assert_eq!(status().event_count, 0);
    }

    #[test]
    fn enabled_records_and_merges_chrome_json() {
        let _guard = TEST_LOCK.lock().unwrap();
        start();
        {
            let _g = span_with("engine", "compile", || json!({ "pages": 3 }));
            instant("engine", "cache_hit", || json!({ "page": 0 }));
            counter("cache", "entries", 12.0);
        }
        let st = stop();
        assert_eq!(st.event_count, 3);

        let js = vec![JsTraceEvent {
            name: "ipc.compile".to_owned(),
            cat: "ipc".to_owned(),
            ph: "X".to_owned(),
            ts_ms: 10.0,
            dur_ms: Some(4.5),
            tid: 2,
            args: Some(json!({ "ok": true })),
        }];
        let raw = export_chrome_json(&js, 1000.0);
        let parsed: Value = serde_json::from_str(&raw).expect("valid json");
        let events = parsed["traceEvents"].as_array().expect("traceEvents array");
        assert!(
            events
                .iter()
                .any(|e| e["name"] == "compile" && e["pid"] == PID_RUST)
        );
        assert!(
            events
                .iter()
                .any(|e| e["name"] == "ipc.compile" && e["pid"] == PID_JS)
        );
        assert_eq!(parsed["metadata"]["rustEvents"], 3);
        assert_eq!(parsed["metadata"]["jsEvents"], 1);
    }

    #[test]
    fn always_on_logs_and_slow_events_survive_when_live_disabled() {
        let _guard = TEST_LOCK.lock().unwrap();
        stop();
        clear_all();

        log_info(
            "engine",
            "workspace opened",
            Some(json!({ "problems": 12 })),
        );
        record_slow_event("engine", "compile", 72.5, Some(json!({ "pages": 4 })));

        let delta = poll_delta(0, 0, 100, 100);
        assert!(!delta.enabled);
        assert_eq!(delta.logs.len(), 2);
        assert_eq!(delta.logs[0].level, LogLevel::Info);
        assert_eq!(delta.logs[0].subsystem, "engine");
        assert_eq!(delta.logs[1].level, LogLevel::Warn);
        assert!(delta.logs[1].message.contains("Slow compile"));
        assert_eq!(delta.slow_count, 1);
        assert_eq!(delta.events.len(), 1);
        assert_eq!(delta.events[0].name, "compile");

        // Second poll with returned cursors yields empty deltas.
        let delta2 = poll_delta(delta.event_seq, delta.log_seq, 100, 100);
        assert!(delta2.events.is_empty());
        assert!(delta2.logs.is_empty());

        clear_all();
        let delta3 = poll_delta(0, 0, 100, 100);
        assert!(delta3.events.is_empty());
        assert!(delta3.logs.is_empty());
        assert_eq!(delta3.slow_count, 0);
    }
}
