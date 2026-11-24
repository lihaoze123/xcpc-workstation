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

thread_local! {
    static THREAD_NUM: u64 = NEXT_TID.fetch_add(1, Ordering::Relaxed);
}

#[derive(Debug, Clone, Serialize)]
pub struct TraceEvent {
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
}

static BUFFER: OnceLock<Mutex<Buffer>> = OnceLock::new();

fn buffer() -> &'static Mutex<Buffer> {
    BUFFER.get_or_init(|| Mutex::new(Buffer::default()))
}

fn epoch() -> Instant {
    *EPOCH.get_or_init(Instant::now)
}

fn ensure_init() -> u8 {
    let _ = epoch();
    let initial = if env_flag_enabled("XCPC_TRACE") {
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

/// Clears buffered events without changing the enabled state.
pub fn clear() {
    if let Ok(mut buf) = buffer().lock() {
        buf.events.clear();
        buf.dropped = 0;
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

fn push_event(event: TraceEvent) {
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
}
