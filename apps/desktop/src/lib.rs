//! The desktop shell's Rust side: what `App.tsx` imports from `xcpc-workstation/native`.
//! Their TypeScript declarations are generated into `.nsh/types/xcpc-workstation/native.d.ts`.
//!
//! Only glue lives here; typesetting is `xcpc-engine`'s.
//!
//! Pages go to the screen as pixel buffers ([`ImageSrc::rgba`]), not encoded files: a
//! page's raster costs about as much as one PNG encode of it would take tenths of. They
//! are kept by key (a page's span-free fingerprint and the scale), so a page an edit
//! did not change is neither rasterized again nor uploaded again.

use std::collections::{HashMap, HashSet};
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
    /// so a page an edit did not change keeps its key (and its texture).
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

    /// Compiles `source` and rasterizes its pages at `scale` pixels per point.
    pub async fn compile(this: Instance<Self>, source: String, scale: f64) -> PreviewResult {
        let state = this.with(|p| p.state.clone());
        blocking(move || state.lock().unwrap().run(&source, scale as f32))
            .await
            .unwrap_or_else(|e| PreviewResult {
                error: Some(format!("preview task failed: {e}")),
                ..Default::default()
            })
    }
}

impl State {
    fn run(&mut self, source: &str, scale: f32) -> PreviewResult {
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
        let keys: Vec<String> = (0..compiled.page_count())
            .map(|i| {
                format!(
                    "{:032x}@{scale}",
                    compiled.page_fingerprint(i).unwrap_or_default()
                )
            })
            .collect();
        let missing: Vec<usize> = {
            let pixels = PIXELS.lock().unwrap();
            (0..keys.len())
                .filter(|&i| !pixels.contains_key(&keys[i]))
                .collect()
        };
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
        self.retire(keys.into_iter().collect());
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
