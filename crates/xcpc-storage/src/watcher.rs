//! Recursive workspace file watching (`notify`) with ignore rules, debouncing,
//! and SHA-256 self-write filtering (docs/architecture.md §5, §7.1).

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::{Error, Result, content_hash, io};

/// Default debounce window for coalescing filesystem bursts.
pub const DEFAULT_DEBOUNCE: Duration = Duration::from_millis(80);

/// How long a self-write hash stays eligible for suppressing duplicate OS events.
const SELF_WRITE_TTL: Duration = Duration::from_secs(3);

/// Debounced external filesystem event inside a workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchEvent {
    /// One or more workspace-relative (`/`-separated) paths changed externally.
    Changed(Vec<String>),
    /// The underlying OS watcher requested a rescan (e.g. queue overflow or error).
    Rescan,
}

/// Shared tracker of recent writes performed by this process so the watcher does
/// not echo our own atomic saves back as external modifications.
#[derive(Debug, Clone, Default)]
pub struct SelfWriteTracker {
    inner: Arc<Mutex<HashMap<String, (String, Instant)>>>,
}

impl SelfWriteTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records that this process is writing `hash` (lowercase SHA-256 hex) to `rel`.
    /// Call this *before* renaming the temporary file into place to avoid racing with
    /// asynchronous OS watch delivery.
    pub fn record(&self, rel: &str, hash: &str) {
        let key = normalize_rel(rel);
        let now = Instant::now();
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        prune_expired(&mut map, now);
        map.insert(key, (hash.to_owned(), now));
    }

    /// Removes a recorded entry (e.g. when a write fails before committing).
    pub fn unrecord(&self, rel: &str) {
        let key = normalize_rel(rel);
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        map.remove(&key);
    }

    /// Returns `true` if `actual_hash` matches a recent self-write for `rel`.
    /// If the file on disk now has a different hash, evicts the stale entry and
    /// returns `false` so external edits are always surfaced.
    pub fn is_self_write(&self, rel: &str, actual_hash: &str) -> bool {
        let key = normalize_rel(rel);
        let now = Instant::now();
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        prune_expired(&mut map, now);
        match map.get(&key) {
            Some((recorded, _)) if recorded == actual_hash => true,
            Some(_) => {
                map.remove(&key);
                false
            }
            None => false,
        }
    }
}

fn prune_expired(map: &mut HashMap<String, (String, Instant)>, now: Instant) {
    map.retain(|_, (_, at)| now.saturating_duration_since(*at) <= SELF_WRITE_TTL);
}

/// Returns `true` for workspace-relative paths that should never trigger refresh events:
/// `build/`, `.xcpc/`, `.git/`, `.jj/`, and atomic temp files containing `.tmp-`.
pub fn should_ignore(rel: &str) -> bool {
    let norm = normalize_rel(rel);
    if norm.is_empty() {
        return true;
    }
    let mut parts = norm.split('/');
    let Some(first) = parts.next() else {
        return true;
    };
    if matches!(first, "build" | ".xcpc" | ".git" | ".jj") {
        return true;
    }
    norm.split('/')
        .any(|part| matches!(part, ".git" | ".jj") || part.contains(".tmp-"))
}

fn normalize_rel(rel: &str) -> String {
    let replaced = rel.replace('\\', "/");
    let trimmed = replaced.trim_start_matches("./").trim_start_matches('/');
    trimmed.to_owned()
}

fn path_to_rel(path: &Path) -> String {
    path.components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Active recursive watcher for a workspace directory. Dropping it stops the OS
/// watch and cleanly joins the debouncer thread.
pub struct WorkspaceWatcher {
    watcher: Option<RecommendedWatcher>,
    thread: Option<thread::JoinHandle<()>>,
    tracker: SelfWriteTracker,
}

impl std::fmt::Debug for WorkspaceWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkspaceWatcher")
            .field("active", &self.watcher.is_some())
            .finish()
    }
}

impl WorkspaceWatcher {
    /// Starts recursively watching `root` with [`DEFAULT_DEBOUNCE`].
    pub fn start(root: impl AsRef<Path>) -> Result<(Self, mpsc::Receiver<WatchEvent>)> {
        Self::start_with(root, DEFAULT_DEBOUNCE, SelfWriteTracker::new())
    }

    /// Starts recursively watching `root` with a custom debounce duration and shared
    /// [`SelfWriteTracker`].
    pub fn start_with(
        root: impl AsRef<Path>,
        debounce: Duration,
        tracker: SelfWriteTracker,
    ) -> Result<(Self, mpsc::Receiver<WatchEvent>)> {
        let root_ref = root.as_ref();
        let root = root_ref
            .canonicalize()
            .map_err(|e| io(root_ref.display(), e))?;
        let (raw_tx, raw_rx) = mpsc::channel::<notify::Result<notify::Event>>();
        let (out_tx, out_rx) = mpsc::channel::<WatchEvent>();

        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            let _ = raw_tx.send(res);
        })
        .map_err(|e| Error::Invalid {
            path: root.display().to_string(),
            message: format!("failed to create file watcher: {e}"),
        })?;
        watcher
            .watch(&root, RecursiveMode::Recursive)
            .map_err(|e| Error::Invalid {
                path: root.display().to_string(),
                message: format!("failed to watch directory: {e}"),
            })?;

        let worker_tracker = tracker.clone();
        let thread = thread::Builder::new()
            .name("xcpc-watcher".into())
            .spawn(move || {
                debounce_loop(root, debounce, worker_tracker, raw_rx, out_tx);
            })
            .map_err(|e| io("xcpc-watcher", e))?;

        Ok((
            Self {
                watcher: Some(watcher),
                thread: Some(thread),
                tracker,
            },
            out_rx,
        ))
    }

    /// Returns a cloneable handle to the self-write tracker.
    pub fn tracker(&self) -> SelfWriteTracker {
        self.tracker.clone()
    }

    /// Records a self-written file `(rel, sha256_hex)` so its OS event is suppressed.
    pub fn record_self_write(&self, rel: &str, hash: &str) {
        self.tracker.record(rel, hash);
    }
}

impl Drop for WorkspaceWatcher {
    fn drop(&mut self) {
        // Drop the OS watcher first so its sender closes and the debouncer exits promptly.
        drop(self.watcher.take());
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

fn debounce_loop(
    root: PathBuf,
    debounce: Duration,
    tracker: SelfWriteTracker,
    raw_rx: mpsc::Receiver<notify::Result<notify::Event>>,
    out_tx: mpsc::Sender<WatchEvent>,
) {
    let max_wait = debounce.saturating_mul(5).max(Duration::from_millis(50));
    let mut pending: BTreeSet<String> = BTreeSet::new();
    let mut needs_rescan = false;
    let mut first_event_at: Option<Instant> = None;
    let mut deadline: Option<Instant> = None;

    loop {
        let received = if let Some(dl) = deadline {
            let now = Instant::now();
            let capped_dl = first_event_at
                .map(|first| dl.min(first + max_wait))
                .unwrap_or(dl);
            match raw_rx.recv_timeout(capped_dl.saturating_duration_since(now)) {
                Ok(ev) => Some(ev),
                Err(RecvTimeoutError::Timeout) => {
                    if !flush(&root, &tracker, &mut pending, &mut needs_rescan, &out_tx) {
                        return;
                    }
                    first_event_at = None;
                    deadline = None;
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => {
                    let _ = flush(&root, &tracker, &mut pending, &mut needs_rescan, &out_tx);
                    return;
                }
            }
        } else {
            match raw_rx.recv() {
                Ok(ev) => Some(ev),
                Err(_) => return,
            }
        };

        if let Some(event_result) = received {
            let had_activity = collect_event(&root, event_result, &mut pending, &mut needs_rescan);
            if had_activity {
                let now = Instant::now();
                first_event_at.get_or_insert(now);
                deadline = Some(now + debounce);
            }
        }
    }
}

fn collect_event(
    root: &Path,
    result: notify::Result<notify::Event>,
    pending: &mut BTreeSet<String>,
    needs_rescan: &mut bool,
) -> bool {
    match result {
        Err(_) => {
            *needs_rescan = true;
            true
        }
        Ok(ev) => {
            let mut changed = false;
            if ev.need_rescan() {
                *needs_rescan = true;
                changed = true;
            }
            if matches!(ev.kind, EventKind::Access(_)) {
                return changed;
            }
            for path in ev.paths {
                let Ok(rel_path) = path.strip_prefix(root) else {
                    continue;
                };
                let rel = path_to_rel(rel_path);
                if should_ignore(&rel) {
                    continue;
                }
                pending.insert(rel);
                changed = true;
            }
            changed
        }
    }
}

fn flush(
    root: &Path,
    tracker: &SelfWriteTracker,
    pending: &mut BTreeSet<String>,
    needs_rescan: &mut bool,
    out_tx: &mpsc::Sender<WatchEvent>,
) -> bool {
    let mut paths = Vec::new();
    for rel in std::mem::take(pending) {
        let abs = root.join(&rel);
        match fs::metadata(&abs) {
            Ok(meta) if meta.is_dir() => {
                // Skip pure directory mtime/create events; file changes and removals drive refreshes.
                continue;
            }
            Ok(_) => {
                if let Ok(bytes) = fs::read(&abs) {
                    let hash = content_hash(&bytes);
                    if tracker.is_self_write(&rel, &hash) {
                        continue;
                    }
                }
                paths.push(rel);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // External deletion (file or directory removed).
                paths.push(rel);
            }
            Err(_) => {
                paths.push(rel);
            }
        }
    }

    if !paths.is_empty() && out_tx.send(WatchEvent::Changed(paths)).is_err() {
        return false;
    }
    if std::mem::take(needs_rescan) && out_tx.send(WatchEvent::Rescan).is_err() {
        return false;
    }
    true
}
