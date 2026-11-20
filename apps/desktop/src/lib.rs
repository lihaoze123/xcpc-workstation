//! The desktop shell's Rust side: what `App.tsx` imports from `xcpc-workstation/native`.
//! Their TypeScript declarations are generated into `.nsh/types/xcpc-workstation/native.d.ts`.
//!
//! Only glue lives here; typesetting is `xcpc-engine`'s.
//!
//! Pages go to the screen as pixel buffers ([`ImageSrc::rgba`]), not encoded files:
//! rasterizing a page takes a fraction of what encoding it as PNG would. A page shows as
//! layers ([`PreviewLayer`]): the whole page at the scale the app asks for, up to
//! [`BASE_SCALE`]; sharper, the whole page at that scale and over it tiles of [`TILE`]
//! pixels at the scale asked for, covering the part in view. Pixels are kept by key:
//! a whole page's by its span-free fingerprint and the scale, a tile's by the
//! fingerprint of what draws in it, the scale and its place. So what an edit did not
//! change is neither rasterized nor uploaded again: typing redraws the tiles it touches,
//! and the small whole-page layer under them. Pages out of view keep a whole-page layer they have at
//! another scale, or show none until they come into view.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Instant;

use nsh_base as _;
use nsh_core::element::ImageSrc;
use nsh_core::geometry::relative;
use nsh_core::prelude::*;
use xcpc_engine::{Compiled, Engine, FontOptions};

/// The crate's UI set (`nsh.json` `"rust"`).
#[native_module("xcpc-workstation/native")]
fn native(registry: &mut Registry) {
    registry.class::<Preview>();
    registry.register::<PageImage>();
}

/// The Phase 0 sample: a hand-written contest in `olymp-classic`.
const SAMPLE: &str = include_str!("../../../tests/fixtures/olymp-ref/contest-zh/main.typ");

/// The most pixels per point of a whole-page layer: sharper than this, the part in view
/// is tiled over it (an A4 page at 1.5 is 893 × 1263 pixels, 4.5 MB).
const BASE_SCALE: f32 = 1.5;

/// The side of a tile in pixels.
const TILE: u32 = 512;

/// The rasterized layers by key, for [`PageImage`]: those of the last two results (the
/// screen may still show the older one).
static PIXELS: LazyLock<Mutex<HashMap<String, ImageSrc>>> = LazyLock::new(Default::default);

/// Pixels to draw over a page.
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct PreviewLayer {
    /// Its pixels, named by what the page draws, the scale and the tile.
    pub key: String,
    /// Where it goes, as fractions of the page's width and height.
    pub left: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
}

/// One rendered page.
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct PreviewPage {
    /// Its size in typographic points.
    pub width: f64,
    pub height: f64,
    /// What `PageImage` draws, bottom first: the whole page, then tiles. None for a
    /// page out of view that has no pixels yet.
    pub layers: Vec<PreviewLayer>,
}

/// The part of a page in view, in its points: tiled when the scale asks for more than a
/// whole-page layer has. A page listed with an empty part gets a whole-page layer only.
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct PreviewView {
    /// The page (0-based).
    pub page: u32,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// What a compilation gave.
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct PreviewResult {
    /// The pages; empty when compilation failed.
    pub pages: Vec<PreviewPage>,
    /// The first error, `line:column: message`, if any.
    #[prop(skip_none)]
    pub error: Option<String>,
    /// Milliseconds spent typesetting and rasterizing, and how many layers were rasterized.
    pub compile_ms: f64,
    pub render_ms: f64,
    pub rendered: u32,
}

/// A Typst preview: compiles a source with `xcpc-engine` off the UI thread and rasterizes
/// what is in view for [`PageImage`] to show.
#[class]
pub struct Preview {
    state: Arc<Mutex<State>>,
}

struct State {
    engine: Engine,
    /// The keys of the last two results.
    live: [HashSet<String>; 2],
}

/// A layer to rasterize: a whole page, or a region of it in pixels.
struct Job {
    page: usize,
    scale: f32,
    region: Option<[u32; 4]>,
    key: String,
}

#[class]
impl Preview {
    /// A preview with the statement fonts of `XCPC_FONT_DIRS`.
    pub fn new() -> Self {
        let engine = Engine::new(&FontOptions::from_env(), None);
        Preview {
            state: Arc::new(Mutex::new(State {
                engine,
                live: Default::default(),
            })),
        }
    }

    /// The sample source the editor starts with.
    pub fn sample() -> String {
        SAMPLE.to_owned()
    }

    /// Compiles `source` and rasterizes the pages in `view` for `scale` pixels per point.
    /// Compiling a source that did not change is all but free, so call it again when the
    /// view or the scale changes.
    pub async fn compile(
        this: Instance<Self>,
        source: String,
        scale: f64,
        view: Vec<PreviewView>,
    ) -> PreviewResult {
        let state = this.with(|p| p.state.clone());
        blocking(move || state.lock().unwrap().run(&source, scale as f32, &view))
            .await
            .unwrap_or_else(|e| PreviewResult {
                error: Some(format!("preview task failed: {e}")),
                ..Default::default()
            })
    }
}

impl State {
    fn run(&mut self, source: &str, scale: f32, view: &[PreviewView]) -> PreviewResult {
        let start = Instant::now();
        self.engine.set_main(source);
        let compiled = self.engine.compile();
        let compile_ms = start.elapsed().as_secs_f64() * 1000.0;
        let compiled = match compiled {
            Ok(c) => c,
            Err(e) => {
                let error = e.errors.first().map(|d| match (d.line, d.column) {
                    (Some(l), Some(c)) => format!("{l}:{c}: {}", d.message),
                    _ => d.message.clone(),
                });
                return PreviewResult {
                    error,
                    compile_ms,
                    ..Default::default()
                };
            }
        };

        let start = Instant::now();
        let base = scale.min(BASE_SCALE);
        let mut pages = Vec::with_capacity(compiled.page_count());
        let mut jobs = Vec::new();
        {
            let pixels = PIXELS.lock().unwrap();
            for i in 0..compiled.page_count() {
                let fingerprint = compiled.page_fingerprint(i).unwrap_or_default();
                let (width, height) = compiled.page_size(i).unwrap_or_default();
                let mut layers = Vec::new();
                let mut add =
                    |key: String, region: Option<[u32; 4]>, s: f32, layers: &mut Vec<_>| {
                        let [x, y, w, h] = region.map_or([0.0, 0.0, width, height], |r| {
                            r.map(|v| f64::from(v) / f64::from(s))
                        });
                        if !pixels.contains_key(&key) {
                            jobs.push(Job {
                                page: i,
                                scale: s,
                                region,
                                key: key.clone(),
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
                match view.iter().find(|v| v.page as usize == i) {
                    Some(part) => {
                        add(
                            format!("{fingerprint:032x}@{base}"),
                            None,
                            base,
                            &mut layers,
                        );
                        if scale > base {
                            let regions = tiles(part, scale, (width, height));
                            let prints = compiled
                                .region_fingerprints(i, scale, &regions)
                                .unwrap_or_default();
                            for (region, print) in regions.into_iter().zip(prints) {
                                let [x, y, ..] = region;
                                let key = format!("{print:032x}@{scale}#{x},{y}");
                                add(key, Some(region), scale, &mut layers);
                            }
                        }
                    }
                    None => {
                        if let Some(key) = nearest(&pixels, fingerprint, scale) {
                            layers.push(PreviewLayer {
                                key,
                                width: 1.0,
                                height: 1.0,
                                ..Default::default()
                            });
                        }
                    }
                }
                pages.push(PreviewPage {
                    width,
                    height,
                    layers,
                });
            }
        }
        let rendered = render(&compiled, &jobs);
        PIXELS.lock().unwrap().extend(rendered);
        let render_ms = start.elapsed().as_secs_f64() * 1000.0;

        let keys = pages
            .iter()
            .flat_map(|p| p.layers.iter().map(|l| l.key.clone()))
            .collect();
        self.retire(keys);
        PreviewResult {
            pages,
            error: None,
            compile_ms,
            render_ms,
            rendered: jobs.len() as u32,
        }
    }

    /// Keeps the pixels of this result and the one before; drops the rest.
    fn retire(&mut self, current: HashSet<String>) {
        let [_, last] = std::mem::take(&mut self.live);
        PIXELS
            .lock()
            .unwrap()
            .retain(|k, _| current.contains(k) || last.contains(k));
        self.live = [last, current];
    }
}

/// The tiles (pixel regions `[x, y, width, height]`) of a page of `size` points at
/// `scale` covering `part` and half a tile around it.
fn tiles(part: &PreviewView, scale: f32, (width, height): (f64, f64)) -> Vec<[u32; 4]> {
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

/// The key of the whole-page layer of the page `fingerprint` at the scale nearest
/// `scale`, if any.
fn nearest(pixels: &HashMap<String, ImageSrc>, fingerprint: u128, scale: f32) -> Option<String> {
    let prefix = format!("{fingerprint:032x}@");
    pixels
        .keys()
        .filter_map(|k| Some((k, k.strip_prefix(&prefix)?.parse::<f32>().ok()?)))
        .min_by(|(_, a), (_, b)| (*a / scale).ln().abs().total_cmp(&(*b / scale).ln().abs()))
        .map(|(k, _)| k.clone())
}

/// Rasterizes `jobs` in parallel into pixel buffers, by key.
fn render(compiled: &Compiled, jobs: &[Job]) -> Vec<(String, ImageSrc)> {
    let threads = std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .min(jobs.len().max(1));
    std::thread::scope(|s| {
        let tasks: Vec<_> = jobs
            .chunks(jobs.len().div_ceil(threads).max(1))
            .map(|chunk| {
                s.spawn(move || {
                    chunk
                        .iter()
                        .filter_map(|job| {
                            let pixmap = match job.region {
                                None => compiled.render(job.page, job.scale)?,
                                Some(r) => compiled.render_region(job.page, job.scale, r)?,
                            };
                            let (w, h) = (pixmap.width(), pixmap.height());
                            // Pages are opaque, so premultiplied RGBA is straight RGBA.
                            Some((job.key.clone(), ImageSrc::rgba(w, h, pixmap.take())))
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        tasks
            .into_iter()
            .flat_map(|t| t.join().unwrap_or_default())
            .collect()
    })
}

/// The props of [`PageImage`].
#[derive(Debug, Clone, PartialEq, Props)]
pub struct PageImageProps {
    /// A page's `layers` from `Preview.compile`.
    #[prop(default)]
    pub layers: Prop<Vec<PreviewLayer>>,
}

/// A page `Preview.compile` rasterized, its layers stretched over the box (size it as the
/// page). New layers show together once all of them have loaded; until then the layers
/// they replace stay, so the page never goes blank (nor blurred, by a new whole-page
/// layer over old tiles) while an edit or a zoom lands.
#[component]
pub fn PageImage(props: PageImageProps, _cx: Scope<NoEvent>) -> impl IntoElement {
    // The keys whose image has loaded; the layers laid out (the current ones over the
    // ones they replace); and the keys of those drawn.
    let ready = signal(HashSet::<String>::new());
    let shown = signal(Vec::<PreviewLayer>::new());
    let live = signal(HashSet::<String>::new());
    let settle = move |current: &[PreviewLayer]| {
        if current
            .iter()
            .all(|l| ready.with_untracked(|r| r.contains(&l.key)))
        {
            shown.set(current.to_vec());
            let keep: HashSet<String> = current.iter().map(|l| l.key.clone()).collect();
            ready.update(|r| r.retain(|k| keep.contains(k)));
            live.set(keep);
        }
    };
    effect(move || {
        let current = props.layers.get();
        untracked(|| {
            let mut next: Vec<PreviewLayer> = shown
                .get_untracked()
                .into_iter()
                .filter(|l| !current.iter().any(|c| c.key == l.key))
                .collect();
            next.extend(current.iter().cloned());
            shown.set(next);
            settle(&current);
        });
    });
    view().relative().size_full().child(each(
        move || shown.get(),
        |layer: &PreviewLayer| layer.key.clone(),
        move |layer, _| {
            let layer = layer.get_untracked();
            let key = layer.key.clone();
            let src = PIXELS
                .lock()
                .unwrap()
                .get(&key)
                .cloned()
                .unwrap_or_else(|| ImageSrc::from(""));
            let arrived = move |key: &String| {
                ready.update(|r| {
                    r.insert(key.clone());
                });
                untracked(|| settle(&props.layers.get_untracked()));
            };
            let is_live = {
                let key = key.clone();
                move || live.with(|r| r.contains(&key))
            };
            image(src)
                .absolute()
                .left(relative(layer.left as f32))
                .top(relative(layer.top as f32))
                .w(relative(layer.width as f32))
                .h(relative(layer.height as f32))
                .opacity(move || if is_live() { 1.0_f32 } else { 0.0 })
                .on_load({
                    let key = key.clone();
                    move |_: &Size<DevicePixels>| arrived(&key)
                })
                // One that fails holds nothing up.
                .on_error(move |_: &str| arrived(&key))
        },
    ))
}
