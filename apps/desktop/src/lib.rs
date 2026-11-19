//! The desktop shell's Rust side: what `App.tsx` imports from `xcpc-workstation/native`.
//! Their TypeScript declarations are generated into `.nsh/types/xcpc-workstation/native.d.ts`.
//!
//! Only glue lives here; typesetting is `xcpc-engine`'s.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use nsh_base as _;
use nsh_core::prelude::*;
use xcpc_engine::{Compiled, Engine, FontOptions};

/// The crate's UI set (`nsh.json` `"rust"`).
#[native_module("xcpc-workstation/native")]
fn native(registry: &mut Registry) {
    registry.class::<Preview>();
}

/// The Phase 0 sample: a hand-written contest in `olymp-classic`.
const SAMPLE: &str = include_str!("../../../tests/fixtures/olymp-ref/contest-zh/main.typ");

/// One rendered page.
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct PreviewPage {
    /// A `file://` URL of its PNG. Named by the page's content, so a page an edit did
    /// not change keeps its URL (and the canvas its decoded image).
    pub url: String,
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
/// the pages that changed to PNG files the canvas draws.
#[class]
pub struct Preview {
    state: Arc<Mutex<State>>,
}

struct State {
    engine: Engine,
    dir: PathBuf,
    /// The files of the last two results: the canvas may still be drawing the older one.
    live: [HashSet<PathBuf>; 2],
}

#[class]
impl Preview {
    /// A preview with the statement fonts of `XCPC_FONT_DIRS`.
    pub fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("xcpc-preview-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let engine = Engine::new(&FontOptions::from_env(), None);
        Preview { state: Arc::new(Mutex::new(State { engine, dir, live: Default::default() })) }
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
                return PreviewResult { error, compile_ms, ..Default::default() };
            }
        };

        let start = Instant::now();
        let paths: Vec<PathBuf> = (0..compiled.page_count())
            .map(|i| {
                let hash = compiled.page_hash(i).unwrap_or_default();
                self.dir.join(format!("{hash:032x}@{scale}.png"))
            })
            .collect();
        let missing: Vec<usize> = (0..paths.len()).filter(|&i| !paths[i].exists()).collect();
        render_pages(&compiled, scale, &missing, &paths);
        let render_ms = start.elapsed().as_secs_f64() * 1000.0;

        let pages = paths
            .iter()
            .enumerate()
            .map(|(i, path)| {
                let (width, height) = compiled.page_size(i).unwrap_or_default();
                PreviewPage { url: format!("file://{}", path.display()), width, height }
            })
            .collect();
        self.retire(paths.into_iter().collect());
        PreviewResult { pages, error: None, compile_ms, render_ms, rendered: missing.len() as u32 }
    }

    /// Keeps the files of this result and the one before; removes the rest.
    fn retire(&mut self, current: HashSet<PathBuf>) {
        let [_, last] = std::mem::take(&mut self.live);
        if let Ok(entries) = std::fs::read_dir(&self.dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !current.contains(&path) && !last.contains(&path) {
                    let _ = std::fs::remove_file(path);
                }
            }
        }
        self.live = [last, current];
    }
}

/// Rasterizes `pages` in parallel, writing each to its path.
fn render_pages(compiled: &Compiled, scale: f32, pages: &[usize], paths: &[PathBuf]) {
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(pages.len().max(1));
    std::thread::scope(|s| {
        for chunk in pages.chunks(pages.len().div_ceil(threads).max(1)) {
            s.spawn(move || {
                for &i in chunk {
                    if let Some(png) = compiled.render_png(i, scale) {
                        write_atomic(&paths[i], &png);
                    }
                }
            });
        }
    });
}

/// Writes through a temporary file, so the canvas never reads half a PNG.
fn write_atomic(path: &Path, bytes: &[u8]) {
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, bytes).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}
