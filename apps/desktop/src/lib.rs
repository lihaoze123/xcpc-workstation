//! The desktop shell's Rust side: what `App.tsx` imports from `xcpc-workstation/native`.
//! Their TypeScript declarations are generated into `.nsh/types/xcpc-workstation/native.d.ts`.
//!
//! Only glue lives here; typesetting is `xcpc-engine`'s.
//!
//! Pages go to the screen as pixel buffers ([`ImageSrc::rgba`]), not encoded files:
//! rasterizing a page takes a fraction of what encoding it as PNG would. They are kept
//! by key (a page's span-free fingerprint and the scale), so a page an edit did not
//! change is neither rasterized again nor uploaded again. Only the pages the app says
//! are in view are rasterized at the scale it asks for; the others keep the pixels they
//! have at another scale, or show none until they come into view.

use std::collections::{HashMap, HashSet};
use std::ops::RangeInclusive;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Instant;

use nsh_base as _;
use nsh_core::element::ImageSrc;
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

/// The rasterized pages by key, for [`PageImage`]: the pages of the last two results
/// (the screen may still show the older one).
static PIXELS: LazyLock<Mutex<HashMap<String, ImageSrc>>> = LazyLock::new(Default::default);

/// One rendered page.
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct PreviewPage {
    /// Its pixels for `PageImage`'s `page`. Named by what the page draws and the scale,
    /// so a page an edit did not change keeps its key (and its texture). Empty for a page
    /// out of view that has no pixels yet.
    pub key: String,
    /// Its size in typographic points.
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
    /// Milliseconds spent typesetting and rasterizing, and how many pages were rasterized.
    pub compile_ms: f64,
    pub render_ms: f64,
    pub rendered: u32,
}

/// A Typst preview: compiles a source with `xcpc-engine` off the UI thread and rasterizes
/// the pages that changed for [`PageImage`] to show.
#[class]
pub struct Preview {
    state: Arc<Mutex<State>>,
}

struct State {
    engine: Engine,
    /// The keys of the last two results.
    live: [HashSet<String>; 2],
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

    /// Compiles `source` and rasterizes the pages `first..=last` (0-based, those in view)
    /// at `scale` pixels per point. Compiling a source that did not change is all but
    /// free, so call it again when other pages come into view or the scale changes.
    pub async fn compile(
        this: Instance<Self>,
        source: String,
        scale: f64,
        first: u32,
        last: u32,
    ) -> PreviewResult {
        let state = this.with(|p| p.state.clone());
        let view = first as usize..=last as usize;
        blocking(move || state.lock().unwrap().run(&source, scale as f32, view))
            .await
            .unwrap_or_else(|e| PreviewResult {
                error: Some(format!("preview task failed: {e}")),
                ..Default::default()
            })
    }
}

impl State {
    fn run(&mut self, source: &str, scale: f32, view: RangeInclusive<usize>) -> PreviewResult {
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
        let mut keys: Vec<String> = Vec::with_capacity(compiled.page_count());
        let mut missing = Vec::new();
        {
            let pixels = PIXELS.lock().unwrap();
            for i in 0..compiled.page_count() {
                let fingerprint = compiled.page_fingerprint(i).unwrap_or_default();
                let key = format!("{fingerprint:032x}@{scale}");
                if view.contains(&i) {
                    if !pixels.contains_key(&key) {
                        missing.push(i);
                    }
                    keys.push(key);
                } else {
                    keys.push(nearest(&pixels, fingerprint, scale).unwrap_or_default());
                }
            }
        }
        let rendered = render_pages(&compiled, scale, &missing);
        {
            let mut pixels = PIXELS.lock().unwrap();
            for (i, src) in rendered {
                pixels.insert(keys[i].clone(), src);
            }
        }
        let render_ms = start.elapsed().as_secs_f64() * 1000.0;

        let pages = keys
            .iter()
            .enumerate()
            .map(|(i, key)| {
                let (width, height) = compiled.page_size(i).unwrap_or_default();
                PreviewPage {
                    key: key.clone(),
                    width,
                    height,
                }
            })
            .collect();
        self.retire(keys.into_iter().filter(|k| !k.is_empty()).collect());
        PreviewResult {
            pages,
            error: None,
            compile_ms,
            render_ms,
            rendered: missing.len() as u32,
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

/// The key of the pixels of the page `fingerprint` at the scale nearest `scale`, if any.
fn nearest(pixels: &HashMap<String, ImageSrc>, fingerprint: u128, scale: f32) -> Option<String> {
    let prefix = format!("{fingerprint:032x}@");
    pixels
        .keys()
        .filter_map(|k| Some((k, k.strip_prefix(&prefix)?.parse::<f32>().ok()?)))
        .min_by(|(_, a), (_, b)| (*a / scale).ln().abs().total_cmp(&(*b / scale).ln().abs()))
        .map(|(k, _)| k.clone())
}

/// Rasterizes `pages` in parallel into pixel buffers.
fn render_pages(compiled: &Compiled, scale: f32, pages: &[usize]) -> Vec<(usize, ImageSrc)> {
    let threads = std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .min(pages.len().max(1));
    std::thread::scope(|s| {
        let tasks: Vec<_> = pages
            .chunks(pages.len().div_ceil(threads).max(1))
            .map(|chunk| {
                s.spawn(move || {
                    chunk
                        .iter()
                        .filter_map(|&i| {
                            let pixmap = compiled.render(i, scale)?;
                            let (w, h) = (pixmap.width(), pixmap.height());
                            // Pages are opaque, so premultiplied RGBA is straight RGBA.
                            Some((i, ImageSrc::rgba(w, h, pixmap.take())))
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
    /// A page's `key` from `Preview.compile`.
    #[prop(default)]
    pub page: Prop<SharedString>,
}

/// A page `Preview.compile` rasterized, stretched over its box (size it to the page's
/// size in points). It shows nothing for a key whose pixels are gone.
#[component]
pub fn PageImage(props: PageImageProps, _cx: Scope<NoEvent>) -> impl IntoElement {
    image(move || {
        let key = props.page.get();
        PIXELS
            .lock()
            .unwrap()
            .get(key.as_ref())
            .cloned()
            .unwrap_or_else(|| ImageSrc::from(""))
    })
}
