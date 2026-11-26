//! Pixel-buffer LRU cache, lazy per-page fingerprinting, tile rasterization, and
//! generation-based task cancellation shared with the desktop shell's `PageImage` view.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Instant;

use xcpc_convert::{Document, publish};
use xcpc_engine::Compiled;

use crate::types::{PreviewLayer, PreviewPage, PreviewRequest, PreviewResult, PreviewView};

/// Immutable, zero-copy-cloneable RGBA pixel buffer for rasterized preview pages and tiles.
#[derive(Debug, Clone)]
pub struct RgbaPixels {
    pub width: u32,
    pub height: u32,
    pub data: Arc<[u8]>,
}

impl RgbaPixels {
    pub fn new(width: u32, height: u32, data: Vec<u8>) -> Self {
        Self {
            width,
            height,
            data: Arc::from(data),
        }
    }
}

/// The most pixels per point of a whole-page layer: sharper than this, the part in view
/// is tiled over it (an A4 page at 1.5 is 893 × 1263 pixels, 4.5 MB).
pub const BASE_SCALE: f32 = 1.5;

/// The side of a tile in pixels.
pub const TILE: u32 = 512;

/// Byte budget for the rasterized pixel cache (96 MiB).
pub const PIXEL_CACHE_BUDGET: usize = 96 * 1024 * 1024;

/// Unique ID assigned to each `State` instance to isolate its cache keys.
pub(crate) static NEXT_PREVIEW_ID: AtomicU64 = AtomicU64::new(1);

/// Whether a cached pixel buffer is a whole-page base layer or a zoomed tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerKind {
    /// Whole-page layer (`scale <= BASE_SCALE`, no sub-region).
    Base,
    /// High-DPI tile layer (`scale > BASE_SCALE`, sub-region).
    Tile,
}

pub(crate) struct PixelEntry {
    pub(crate) src: RgbaPixels,
    pub(crate) bytes: usize,
    pub(crate) kind: LayerKind,
    pub(crate) owner: u64,
    pub(crate) last_used: u64,
}

/// Byte-budgeted LRU pixel cache shared with the desktop shell's `PageImage`.
///
/// Eviction never drops currently active layers (`protected`), prefers evicting
/// least-recently-used tile layers first, and only evicts base whole-page layers
/// when no non-protected tiles remain.
pub struct PixelCache {
    pub(crate) budget: usize,
    pub(crate) total_bytes: usize,
    clock: u64,
    pub(crate) hits: u64,
    pub(crate) misses: u64,
    pub(crate) entries: HashMap<String, PixelEntry>,
}

/// Snapshot of pixel-cache occupancy and hit-rate counters for diagnostics.
#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PixelCacheStats {
    pub total_bytes: usize,
    pub budget_bytes: usize,
    pub entry_count: usize,
    pub hits: u64,
    pub misses: u64,
}

impl PixelCache {
    pub fn with_budget(budget: usize) -> Self {
        Self {
            budget,
            total_bytes: 0,
            clock: 0,
            hits: 0,
            misses: 0,
            entries: HashMap::new(),
        }
    }

    pub fn stats(&self) -> PixelCacheStats {
        PixelCacheStats {
            total_bytes: self.total_bytes,
            budget_bytes: self.budget,
            entry_count: self.entries.len(),
            hits: self.hits,
            misses: self.misses,
        }
    }

    fn tick(&mut self) -> u64 {
        self.clock = self.clock.wrapping_add(1);
        self.clock
    }

    pub fn contains_and_touch(&mut self, key: &str) -> bool {
        let next = self.clock.wrapping_add(1);
        if let Some(entry) = self.entries.get_mut(key) {
            self.clock = next;
            entry.last_used = next;
            self.hits = self.hits.saturating_add(1);
            true
        } else {
            self.misses = self.misses.saturating_add(1);
            false
        }
    }

    pub fn get(&mut self, key: &str) -> Option<RgbaPixels> {
        let next = self.clock.wrapping_add(1);
        if let Some(entry) = self.entries.get_mut(key) {
            self.clock = next;
            entry.last_used = next;
            Some(entry.src.clone())
        } else {
            None
        }
    }

    pub fn insert(
        &mut self,
        key: String,
        src: RgbaPixels,
        bytes: usize,
        kind: LayerKind,
        owner: u64,
    ) {
        let tick = self.tick();
        if let Some(old) = self.entries.insert(
            key,
            PixelEntry {
                src,
                bytes,
                kind,
                owner,
                last_used: tick,
            },
        ) {
            self.total_bytes = self.total_bytes.saturating_sub(old.bytes);
        }
        self.total_bytes = self.total_bytes.saturating_add(bytes);
    }

    pub(crate) fn touch_all<'a>(&mut self, keys: impl IntoIterator<Item = &'a String>) {
        for key in keys {
            let next = self.clock.wrapping_add(1);
            if let Some(entry) = self.entries.get_mut(key) {
                self.clock = next;
                entry.last_used = next;
            }
        }
    }

    pub(crate) fn evict(&mut self, protected: &HashSet<String>) {
        while self.total_bytes > self.budget {
            let victim = self
                .entries
                .iter()
                .filter(|(k, _)| !protected.contains(*k))
                .min_by_key(|(_, e)| {
                    let tier = match e.kind {
                        LayerKind::Tile => 0u8,
                        LayerKind::Base => 1u8,
                    };
                    (tier, e.last_used)
                })
                .map(|(k, _)| k.clone());
            let Some(key) = victim else {
                break;
            };
            if let Some(removed) = self.entries.remove(&key) {
                self.total_bytes = self.total_bytes.saturating_sub(removed.bytes);
            }
        }
    }

    /// The key of the whole-page layer matching `prefix` (`"{owner}:{fingerprint:032x}@"`)
    /// at the scale nearest `scale`, if any.
    pub(crate) fn nearest(&mut self, prefix: &str, scale: f32) -> Option<String> {
        let found = self
            .entries
            .iter()
            .filter(|(_, e)| e.kind == LayerKind::Base)
            .filter_map(|(k, _)| Some((k, k.strip_prefix(prefix)?.parse::<f32>().ok()?)))
            .min_by(|(_, a), (_, b)| (*a / scale).ln().abs().total_cmp(&(*b / scale).ln().abs()))
            .map(|(k, _)| k.clone());
        if let Some(ref key) = found {
            let next = self.clock.wrapping_add(1);
            if let Some(entry) = self.entries.get_mut(key) {
                self.clock = next;
                entry.last_used = next;
            }
        }
        found
    }

    pub(crate) fn remove_owner(&mut self, owner: u64) {
        let mut freed = 0usize;
        self.entries.retain(|_, e| {
            if e.owner == owner {
                freed = freed.saturating_add(e.bytes);
                false
            } else {
                true
            }
        });
        self.total_bytes = self.total_bytes.saturating_sub(freed);
    }
}

/// The rasterized layers by key, for the desktop shell's `PageImage`, managed as a
/// byte-budgeted LRU cache.
pub static PIXELS: LazyLock<Mutex<PixelCache>> =
    LazyLock::new(|| Mutex::new(PixelCache::with_budget(PIXEL_CACHE_BUDGET)));

/// Returns a point-in-time snapshot of the global pixel cache occupancy and hit/miss counters.
pub fn pixel_cache_stats() -> PixelCacheStats {
    PIXELS.lock().map(|cache| cache.stats()).unwrap_or_default()
}

/// One sampled mapping from a 1-based Markdown line to a 0-based preview page and vertical
/// offset (`y_pt`, typographic points from top of page).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SourceAnchor {
    pub(crate) line: u32,
    pub(crate) page: u32,
    pub(crate) y_pt: f64,
}

/// Lightweight, UI-thread-accessible snapshot of the latest compiled preview.
///
/// Kept outside `CompilerState`'s mutex so `export_status`, `save` cache-preservation checks,
/// and `locate_source` never block on an active Typst compilation or rasterization.
pub(crate) struct CachedSnapshot {
    pub(crate) request: PreviewRequest,
    pub(crate) revision: u64,
    /// True when the compiled state is known to match the files currently on disk
    /// (either compiled with no unsaved overlay, or `save()` wrote the exact same text).
    pub(crate) matches_disk: Arc<AtomicBool>,
    pub(crate) doc_source: String,
    pub(crate) problems: Vec<publish::Pages>,
    /// Forward-locate anchors per normalized workspace-relative file path, sorted by `line`.
    pub(crate) anchors: Mutex<HashMap<String, Vec<SourceAnchor>>>,
}

/// Cached compilation result reused across view/scale changes and `Ctrl+click` source
/// lookup until invalidated.
pub(crate) struct CachedCompile {
    pub(crate) request: PreviewRequest,
    pub(crate) revision: u64,
    /// Shared with [`CachedSnapshot::matches_disk`] so `save()` can promote an unsaved
    /// compilation to disk-clean without locking `CompilerState`.
    pub(crate) matches_disk: Arc<AtomicBool>,
    pub(crate) compiled: Compiled,
    pub(crate) doc: Document,
    pub(crate) report: publish::Report,
    /// Lazily computed span-free fingerprints per page (computed only when a page enters `view`).
    pub(crate) fingerprints: Vec<Option<u128>>,
}

impl CachedCompile {
    /// Returns true when this cached compilation is still valid at `revision` and satisfies
    /// `request` (either exact request match, or disk-clean match when `request.path` is empty).
    pub(crate) fn matches_request(&self, request: &PreviewRequest, revision: u64) -> bool {
        if self.revision != revision {
            return false;
        }
        if self.request == *request {
            return true;
        }
        request.path.is_empty()
            && self.matches_disk.load(Ordering::Acquire)
            && self.request.target == request.target
            && self.request.id == request.id
            && self.request.book == request.book
    }
}

/// Generation-based cancellation check for an in-flight compile/rasterize request.
#[derive(Clone, Copy)]
pub struct CancelToken<'a> {
    pub generation: u64,
    pub latest: &'a AtomicU64,
}

impl CancelToken<'_> {
    #[inline]
    pub fn is_cancelled(self) -> bool {
        self.latest.load(Ordering::Acquire) != self.generation
    }
}

/// A layer to rasterize: a whole page, or a region of it in pixels.
pub(crate) struct Job {
    pub(crate) page: usize,
    pub(crate) scale: f32,
    pub(crate) region: Option<[u32; 4]>,
    pub(crate) key: String,
    pub(crate) kind: LayerKind,
}

/// A completed rasterized layer ready to insert into [`PIXELS`].
pub(crate) struct RenderedLayer {
    pub(crate) key: String,
    pub(crate) src: RgbaPixels,
    pub(crate) bytes: usize,
    pub(crate) kind: LayerKind,
}

pub(crate) fn rasterize_cached(
    owner: u64,
    active: &mut HashSet<String>,
    cached: &mut CachedCompile,
    scale: f32,
    view: &[PreviewView],
    cancel: CancelToken<'_>,
) -> (PreviewResult, bool) {
    let _span = xcpc_trace::span_with("desktop", "rasterize_cached", || {
        serde_json::json!({
            "scale": scale,
            "viewPages": view.len(),
            "totalPages": cached.compiled.page_count(),
        })
    });
    let start = Instant::now();
    let compiled = &cached.compiled;
    let fingerprints = &mut cached.fingerprints;
    let page_count = compiled.page_count();
    let base = scale.min(BASE_SCALE);

    // Compute fingerprints lazily ONLY for pages in `view` (outside PIXELS lock).
    let mut in_view = HashSet::with_capacity(view.len());
    let mut view_tiles: HashMap<usize, Vec<([u32; 4], u128)>> = HashMap::new();
    for part in view {
        if cancel.is_cancelled() {
            return (
                PreviewResult {
                    render_ms: start.elapsed().as_secs_f64() * 1000.0,
                    ..Default::default()
                },
                true,
            );
        }
        let i = part.page as usize;
        if i >= page_count || !in_view.insert(i) {
            continue;
        }
        fingerprints[i].get_or_insert_with(|| compiled.page_fingerprint(i).unwrap_or_default());
        if scale > base && part.width > 0.0 && part.height > 0.0 {
            let size = compiled.page_size(i).unwrap_or_default();
            let regions = tiles(part, scale, size);
            let prints = compiled
                .region_fingerprints(i, scale, &regions)
                .unwrap_or_default();
            view_tiles.insert(i, regions.into_iter().zip(prints).collect());
        }
    }

    let mut pages = Vec::with_capacity(page_count);
    let mut jobs = Vec::new();
    let mut queued = HashSet::new();
    {
        let mut pixels = PIXELS.lock().unwrap();
        for (i, fp) in fingerprints.iter().enumerate().take(page_count) {
            let (width, height) = compiled.page_size(i).unwrap_or_default();
            let mut layers = Vec::new();
            let mut add = |key: String, region: Option<[u32; 4]>, s: f32, layers: &mut Vec<_>| {
                let [x, y, w, h] = region.map_or([0.0, 0.0, width, height], |r| {
                    r.map(|v| f64::from(v) / f64::from(s))
                });
                if !pixels.contains_and_touch(&key) && queued.insert(key.clone()) {
                    let kind = if region.is_none() && s <= BASE_SCALE {
                        LayerKind::Base
                    } else {
                        LayerKind::Tile
                    };
                    jobs.push(Job {
                        page: i,
                        scale: s,
                        region,
                        key: key.clone(),
                        kind,
                    });
                }
                layers.push(PreviewLayer {
                    key,
                    left: x / width,
                    top: y / height,
                    width: (w / width).min(1.0 - x / width),
                    height: (h / height).min(1.0 - y / height),
                });
            };
            if in_view.contains(&i) {
                let fingerprint = fp.expect("computed for in-view page");
                add(
                    format!("{owner}:{fingerprint:032x}@{base}"),
                    None,
                    base,
                    &mut layers,
                );
                if let Some(tile_list) = view_tiles.remove(&i) {
                    for (region, print) in tile_list {
                        let [x, y, ..] = region;
                        let key = format!("{owner}:{print:032x}@{scale}#{x},{y}");
                        add(key, Some(region), scale, &mut layers);
                    }
                }
            } else if let Some(fingerprint) = *fp {
                let prefix = format!("{owner}:{fingerprint:032x}@");
                if let Some(key) = pixels.nearest(&prefix, scale) {
                    layers.push(PreviewLayer {
                        key,
                        width: 1.0,
                        height: 1.0,
                        ..Default::default()
                    });
                }
            }
            pages.push(PreviewPage {
                width,
                height,
                layers,
            });
        }
    }

    if cancel.is_cancelled() {
        return (
            PreviewResult {
                render_ms: start.elapsed().as_secs_f64() * 1000.0,
                ..Default::default()
            },
            true,
        );
    }

    let rendered = render(compiled, &jobs, cancel);
    let rendered_count = rendered.len() as u32;
    let cancelled = cancel.is_cancelled();
    let current_keys: HashSet<String> = if cancelled {
        HashSet::new()
    } else {
        // Protect only currently in-view pages so offscreen visited pages stay eligible
        // for LRU eviction and never pin the 96 MiB budget across 22+ page contests.
        pages
            .iter()
            .enumerate()
            .filter(|(i, _)| in_view.contains(i))
            .flat_map(|(_, p)| p.layers.iter().map(|l| l.key.clone()))
            .collect()
    };

    {
        let mut pixels = PIXELS.lock().unwrap();
        for item in rendered {
            pixels.insert(item.key, item.src, item.bytes, item.kind, owner);
        }
        if !cancelled {
            *active = current_keys;
            pixels.touch_all(active.iter());
        }
        pixels.evict(active);
    }

    let render_ms = start.elapsed().as_secs_f64() * 1000.0;
    if !cancelled && render_ms > 50.0 {
        xcpc_trace::record_slow_event(
            "engine",
            "rasterize_cached",
            render_ms,
            Some(serde_json::json!({
                "scale": scale,
                "rendered": rendered_count,
                "viewPages": view.len(),
            })),
        );
    }
    if cancelled {
        return (
            PreviewResult {
                render_ms,
                rendered: rendered_count,
                ..Default::default()
            },
            true,
        );
    }
    (
        PreviewResult {
            pages,
            compile_ms: 0.0,
            render_ms,
            rendered: rendered_count,
            ..Default::default()
        },
        false,
    )
}

/// Speculatively rasterizes adjacent pages at base scale into the shared `PIXELS` cache
/// so scrolling into `current ± 1` hits warm pixels immediately. Checks `cancel` before
/// every page and never mutates `active` so visible layers are never disturbed.
pub(crate) fn prewarm_cached(
    owner: u64,
    active: &HashSet<String>,
    cached: &mut CachedCompile,
    scale: f32,
    neighbor_pages: &[u32],
    cancel: CancelToken<'_>,
) -> u32 {
    if cancel.is_cancelled() || neighbor_pages.is_empty() {
        return 0;
    }
    let _span = xcpc_trace::span_with("desktop", "prewarm_neighbors", || {
        serde_json::json!({
            "scale": scale,
            "requested": neighbor_pages.len(),
        })
    });
    let compiled = &cached.compiled;
    let fingerprints = &mut cached.fingerprints;
    let page_count = compiled.page_count();
    let base = scale.min(BASE_SCALE);

    let mut jobs = Vec::new();
    let mut queued = HashSet::new();
    for &p in neighbor_pages {
        if cancel.is_cancelled() {
            return 0;
        }
        let i = p as usize;
        if i >= page_count {
            continue;
        }
        let fingerprint = *fingerprints[i]
            .get_or_insert_with(|| compiled.page_fingerprint(i).unwrap_or_default());
        let key = format!("{owner}:{fingerprint:032x}@{base}");
        let cached_hit = {
            let mut pixels = PIXELS.lock().unwrap();
            pixels.contains_and_touch(&key)
        };
        if !cached_hit && queued.insert(key.clone()) {
            jobs.push(Job {
                page: i,
                scale: base,
                region: None,
                key,
                kind: LayerKind::Base,
            });
        }
    }
    if jobs.is_empty() || cancel.is_cancelled() {
        return 0;
    }

    let rendered = render(compiled, &jobs, cancel);
    let count = rendered.len() as u32;
    if count > 0 {
        let mut protected = active.clone();
        let mut pixels = PIXELS.lock().unwrap();
        for item in rendered {
            protected.insert(item.key.clone());
            pixels.insert(item.key, item.src, item.bytes, item.kind, owner);
        }
        pixels.evict(&protected);
    }
    count
}

/// The tiles (pixel regions `[x, y, width, height]`) of a page of `size` points at
/// `scale` covering `part` and half a tile around it.
pub(crate) fn tiles(part: &PreviewView, scale: f32, (width, height): (f64, f64)) -> Vec<[u32; 4]> {
    if part.width <= 0.0 || part.height <= 0.0 {
        return Vec::new();
    }
    let s = f64::from(scale);
    let (pw, ph) = ((width * s).round() as u32, (height * s).round() as u32);
    let margin = f64::from(TILE / 2);
    let span = |from: f64, len: f64, max: u32| {
        let a = ((from * s - margin).max(0.0) as u32 / TILE).min(max.saturating_sub(1) / TILE);
        let b = (((from + len) * s + margin).max(0.0) as u32).min(max.saturating_sub(1)) / TILE;
        a..=b
    };
    let mut out = Vec::new();
    for ty in span(part.y, part.height, ph) {
        for tx in span(part.x, part.width, pw) {
            let (x, y) = (tx * TILE, ty * TILE);
            out.push([x, y, TILE.min(pw - x), TILE.min(ph - y)]);
        }
    }
    out
}

/// Rasterizes a slice of `jobs`, checking `cancel` before each layer so stale work aborts immediately.
pub(crate) fn render_chunk(
    compiled: &Compiled,
    chunk: &[Job],
    cancel: CancelToken<'_>,
    mut after_each: impl FnMut(),
) -> Vec<RenderedLayer> {
    let mut out = Vec::with_capacity(chunk.len());
    for job in chunk {
        if cancel.is_cancelled() {
            break;
        }
        let Some(pixmap) = (match job.region {
            None => compiled.render(job.page, job.scale),
            Some(r) => compiled.render_region(job.page, job.scale, r),
        }) else {
            continue;
        };
        let (w, h) = (pixmap.width(), pixmap.height());
        let bytes = (w as usize).saturating_mul(h as usize).saturating_mul(4);
        // Pages are opaque, so premultiplied RGBA is straight RGBA.
        out.push(RenderedLayer {
            key: job.key.clone(),
            src: RgbaPixels::new(w, h, pixmap.take()),
            bytes,
            kind: job.kind,
        });
        after_each();
    }
    out
}

/// Rasterizes `jobs` in parallel into pixel buffers, by key, stopping early if `cancel` triggers.
fn render(compiled: &Compiled, jobs: &[Job], cancel: CancelToken<'_>) -> Vec<RenderedLayer> {
    if jobs.is_empty() || cancel.is_cancelled() {
        return Vec::new();
    }
    // Leave headroom for the UI/compositor thread so background tile rasterization
    // never starves GTK4 frame presentation during zoom/scroll.
    let threads = std::thread::available_parallelism()
        .map_or(2, |n| n.get().saturating_sub(1).clamp(1, 4))
        .min(jobs.len());
    std::thread::scope(|s| {
        let tasks: Vec<_> = jobs
            .chunks(jobs.len().div_ceil(threads).max(1))
            .map(|chunk| s.spawn(move || render_chunk(compiled, chunk, cancel, || {})))
            .collect();
        tasks
            .into_iter()
            .flat_map(|t| t.join().unwrap_or_default())
            .collect()
    })
}
