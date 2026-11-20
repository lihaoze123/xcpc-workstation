//! The desktop shell's Rust side: what `App.tsx` imports from `xcpc-workstation/native`.
//! Their TypeScript declarations are generated into `.nsh/types/xcpc-workstation/native.d.ts`.
//!
//! Only glue lives here: the workspace is `xcpc-storage`'s, publishing is
//! `xcpc-convert`'s, typesetting `xcpc-engine`'s.
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
use nsh_core::file_dialog::OpenDialog;
use nsh_core::geometry::relative;
use nsh_core::prelude::*;
use xcpc_convert::publish::{self, Kind, Target};
use xcpc_engine::{Compiled, Engine, FontOptions};
use xcpc_storage::Workspace;

/// The crate's UI set (`nsh.json` `"rust"`).
#[native_module("xcpc-workstation/native")]
fn native(registry: &mut Registry) {
    registry.class::<Preview>();
    registry.register::<PageImage>();
}

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

/// A problem or contest of the workspace to typeset, with the file the editor has
/// open as it is there (saved or not).
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct PreviewRequest {
    /// `"problem"` or `"contest"`.
    pub target: String,
    pub id: String,
    /// `"statements"` or `"editorial"`.
    pub book: String,
    /// The open file, relative to the workspace, and its text; an empty path for none.
    pub path: String,
    pub text: String,
}

impl PreviewRequest {
    fn target(&self) -> Target {
        match self.target.as_str() {
            "contest" => Target::Contest(self.id.clone()),
            _ => Target::Problem(self.id.clone()),
        }
    }

    fn kind(&self) -> Kind {
        if self.book == "editorial" {
            Kind::Editorial
        } else {
            Kind::Statements
        }
    }

    fn unsaved(&self) -> Option<(&str, &str)> {
        (!self.path.is_empty()).then_some((self.path.as_str(), self.text.as_str()))
    }
}

/// A diagnostic, in a workspace file when it points into one.
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct PreviewDiagnostic {
    /// `"error"` or `"warning"`.
    pub severity: String,
    pub message: String,
    /// The file (relative to the workspace), and the line and column (1-based).
    #[prop(skip_none)]
    pub file: Option<String>,
    #[prop(skip_none)]
    pub line: Option<u32>,
    #[prop(skip_none)]
    pub column: Option<u32>,
    /// The page (0-based) a layout warning is about.
    #[prop(skip_none)]
    pub page: Option<u32>,
}

/// Where a problem starts.
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct PreviewStart {
    pub label: String,
    /// The page, 0-based.
    pub page: u32,
}

/// What a compilation gave.
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct PreviewResult {
    /// The pages; empty when compilation failed.
    pub pages: Vec<PreviewPage>,
    /// The first error, as the user should read it, if any.
    #[prop(skip_none)]
    pub error: Option<String>,
    /// Errors and warnings.
    pub diagnostics: Vec<PreviewDiagnostic>,
    /// Where each problem starts.
    pub starts: Vec<PreviewStart>,
    /// Milliseconds spent typesetting and rasterizing, and how many layers were rasterized.
    pub compile_ms: f64,
    pub render_ms: f64,
    pub rendered: u32,
}

/// A problem of the workspace.
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct WorkspaceProblem {
    pub id: String,
    pub title: String,
    /// The statement and editorial files, relative to the workspace.
    pub statement: String,
    pub editorial: String,
}

/// A problem of a contest.
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct WorkspaceItem {
    pub label: String,
    pub id: String,
    pub title: String,
    /// Its statement (in the contest's language) and editorial files, relative to the
    /// workspace: the library's, or the snapshot's once the contest is frozen.
    pub statement: String,
    pub editorial: String,
}

/// A contest of the workspace.
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct WorkspaceContest {
    pub id: String,
    pub name: String,
    pub frozen: bool,
    pub items: Vec<WorkspaceItem>,
}

/// The open workspace.
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct WorkspaceInfo {
    /// Its directory; empty when none is open.
    pub root: String,
    pub read_only: bool,
    /// What could not be read, or why the folder picked is no workspace.
    #[prop(skip_none)]
    pub error: Option<String>,
    pub problems: Vec<WorkspaceProblem>,
    pub contests: Vec<WorkspaceContest>,
}

/// A file of the workspace as the editor opens it.
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct WorkspaceFile {
    pub path: String,
    /// Its text; empty for a file that does not exist yet.
    pub text: String,
    /// Edits cannot be saved (a frozen contest, a read-only workspace).
    pub read_only: bool,
    #[prop(skip_none)]
    pub error: Option<String>,
}

/// The workspace and its preview: reads and saves files, compiles a problem or a
/// contest with `xcpc-convert` off the UI thread and rasterizes what is in view for
/// [`PageImage`] to show.
#[class]
pub struct Preview {
    state: Arc<Mutex<State>>,
}

struct State {
    workspace: Option<Workspace>,
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
    #[allow(clippy::new_without_default)] // the JS constructor
    /// A preview of the workspace `XCPC_WORKSPACE` names, else the one the current
    /// directory is in, with the statement fonts of `XCPC_FONT_DIRS`.
    pub fn new() -> Self {
        let workspace = match std::env::var_os("XCPC_WORKSPACE") {
            Some(dir) => Workspace::open(dir, false).ok(),
            None => Workspace::discover(".", false).ok(),
        };
        let root = workspace.as_ref().map(|w| w.root().to_path_buf());
        Preview {
            state: Arc::new(Mutex::new(State {
                workspace,
                engine: Engine::new(&FontOptions::from_env(), root),
                live: Default::default(),
            })),
        }
    }

    /// The workspace of the test fixture `contest-zh`.
    pub fn fixture() -> String {
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/olymp-ref/contest-zh/workspace"
        )
        .to_owned()
    }

    /// Opens the workspace at `path` instead.
    pub fn open_workspace(&self, path: String, read_only: bool) -> WorkspaceInfo {
        match Workspace::open(&path, read_only) {
            Ok(ws) => self.switch(ws),
            Err(e) => WorkspaceInfo {
                error: Some(e.to_string()),
                ..Default::default()
            },
        }
    }

    /// Asks for a folder with the system's dialog and opens the workspace it is in
    /// (it, or the nearest folder above it with a `workspace.toml`). Cancelled, the
    /// workspace stays as it was; a folder in no workspace leaves it too, with the
    /// error said.
    pub async fn pick_workspace(this: Instance<Self>) -> WorkspaceInfo {
        let start = this.with(|p| {
            p.state
                .lock()
                .unwrap()
                .workspace
                .as_ref()
                .map(|w| w.root().to_path_buf())
        });
        let mut dialog = OpenDialog::folders()
            .title("打开工作区")
            .button_label("打开");
        if let Some(dir) = start.as_ref().and_then(|r| r.parent()) {
            dialog = dialog.directory(dir);
        }
        let picked = pick_paths(dialog).await;
        this.with(|p| {
            let dir = match picked {
                Ok(Some(paths)) => paths.into_iter().next(),
                Ok(None) => None,
                Err(e) => {
                    return WorkspaceInfo {
                        error: Some(e),
                        ..p.workspace()
                    };
                }
            };
            let Some(dir) = dir else { return p.workspace() };
            match Workspace::discover(&dir, false) {
                Ok(ws) => p.switch(ws),
                Err(_) => WorkspaceInfo {
                    error: Some(format!(
                        "{} 不在工作区里（没有 workspace.toml）",
                        dir.display()
                    )),
                    ..p.workspace()
                },
            }
        })
    }

    /// What the workspace has.
    pub fn workspace(&self) -> WorkspaceInfo {
        self.info()
    }

    /// Reads a file of the workspace for the editor.
    pub fn read(&self, path: String) -> WorkspaceFile {
        let state = self.state.lock().unwrap();
        let Some(ws) = &state.workspace else {
            return WorkspaceFile {
                path,
                read_only: true,
                ..Default::default()
            };
        };
        let read_only = ws.check_writable(&path).is_err();
        match ws.read_text(&path) {
            Ok(text) => WorkspaceFile {
                path,
                text,
                read_only,
                error: None,
            },
            Err(xcpc_storage::Error::NotFound(_)) => WorkspaceFile {
                path,
                read_only,
                ..Default::default()
            },
            Err(e) => WorkspaceFile {
                path,
                read_only: true,
                error: Some(e.to_string()),
                ..Default::default()
            },
        }
    }

    /// Saves a file of the workspace (atomically); the error, if it could not.
    pub fn save(&self, path: String, text: String) -> Option<String> {
        let state = self.state.lock().unwrap();
        let ws = state.workspace.as_ref()?;
        ws.write(&path, text.as_bytes())
            .err()
            .map(|e| e.to_string())
    }

    /// Compiles `request` and rasterizes the pages in `view` for `scale` pixels per point.
    /// Compiling what did not change is all but free, so call it again when the view or
    /// the scale changes.
    pub async fn compile(
        this: Instance<Self>,
        request: PreviewRequest,
        scale: f64,
        view: Vec<PreviewView>,
    ) -> PreviewResult {
        let state = this.with(|p| p.state.clone());
        blocking(move || state.lock().unwrap().run(&request, scale as f32, &view))
            .await
            .unwrap_or_else(|e| PreviewResult {
                error: Some(format!("preview task failed: {e}")),
                ..Default::default()
            })
    }

    /// Exports `request`'s PDF to `build/` in the workspace: its path, or why not.
    pub async fn export(this: Instance<Self>, request: PreviewRequest) -> WorkspaceFile {
        let state = this.with(|p| p.state.clone());
        blocking(move || state.lock().unwrap().export(&request))
            .await
            .unwrap_or_else(|e| WorkspaceFile {
                error: Some(format!("export failed: {e}")),
                ..Default::default()
            })
    }
}

impl Preview {
    /// Makes `ws` the workspace: the engine resolves its files from now on.
    fn switch(&self, ws: Workspace) -> WorkspaceInfo {
        {
            let mut state = self.state.lock().unwrap();
            state.engine.set_root(Some(ws.root().to_path_buf()));
            state.workspace = Some(ws);
        }
        self.info()
    }

    fn info(&self) -> WorkspaceInfo {
        let state = self.state.lock().unwrap();
        // None open is a state (an empty root), not an error.
        let Some(ws) = &state.workspace else {
            return WorkspaceInfo::default();
        };
        let mut info = WorkspaceInfo {
            root: ws.root().display().to_string(),
            read_only: ws.is_read_only(),
            ..Default::default()
        };
        let mut errors = Vec::new();
        for id in ws.list_problems().unwrap_or_default() {
            match ws.load_problem(&id) {
                Ok(p) => info.problems.push(WorkspaceProblem {
                    statement: statement_path(&p, p.default_language()),
                    editorial: format!("{}/editorial.md", p.dir),
                    id,
                    title: p.meta.title,
                }),
                Err(e) => errors.push(e.to_string()),
            }
        }
        for id in ws.list_contests().unwrap_or_default() {
            let contest = match ws.load_contest(&id) {
                Ok(c) => c,
                Err(e) => {
                    errors.push(e.to_string());
                    continue;
                }
            };
            let items = match ws.contest_problems(&contest) {
                Ok(list) => list
                    .into_iter()
                    .map(|(item, p)| WorkspaceItem {
                        title: item.apply(&p.meta).title,
                        statement: statement_path(&p, contest.meta.language),
                        editorial: format!("{}/editorial.md", p.dir),
                        label: item.label,
                        id: item.id,
                    })
                    .collect(),
                Err(e) => {
                    errors.push(e.to_string());
                    Vec::new()
                }
            };
            info.contests.push(WorkspaceContest {
                id,
                name: contest.meta.name,
                frozen: contest.meta.frozen,
                items,
            });
        }
        info.error = errors.into_iter().next();
        info
    }
}

impl State {
    fn compile(
        &mut self,
        request: &PreviewRequest,
    ) -> Result<(Option<Compiled>, publish::Report), String> {
        let ws = self.workspace.as_ref().ok_or("no workspace")?;
        let doc = publish::document_with(
            ws,
            &request.target(),
            request.kind(),
            None,
            request.unsaved(),
        )
        .map_err(|e| e.to_string())?;
        Ok(publish::compile(&mut self.engine, &doc))
    }

    fn export(&mut self, request: &PreviewRequest) -> WorkspaceFile {
        let path = publish::default_output(&request.target(), request.kind());
        let fail = |error: String| WorkspaceFile {
            path: path.clone(),
            error: Some(error),
            ..Default::default()
        };
        let (compiled, report) = match self.compile(request) {
            Ok(r) => r,
            Err(e) => return fail(e),
        };
        let Some(compiled) = compiled.filter(|_| report.ok) else {
            return fail(first_error(&report).unwrap_or_default());
        };
        let pdf = match compiled.pdf() {
            Ok(pdf) => pdf,
            Err(e) => return fail(e.first().map(|d| d.message.clone()).unwrap_or_default()),
        };
        let ws = self.workspace.as_ref().expect("compiled in it");
        match ws.write(&path, &pdf) {
            Ok(()) => WorkspaceFile {
                path: ws.root().join(&path).display().to_string(),
                ..Default::default()
            },
            Err(e) => fail(e.to_string()),
        }
    }

    fn run(&mut self, request: &PreviewRequest, scale: f32, view: &[PreviewView]) -> PreviewResult {
        let start = Instant::now();
        let compiled = self.compile(request);
        let compile_ms = start.elapsed().as_secs_f64() * 1000.0;
        let (compiled, report) = match compiled {
            Ok(r) => r,
            Err(error) => {
                return PreviewResult {
                    error: Some(error),
                    compile_ms,
                    ..Default::default()
                };
            }
        };
        let diagnostics = report
            .diagnostics
            .iter()
            .map(|d| PreviewDiagnostic {
                severity: d.severity.into(),
                message: d.message.clone(),
                file: d.file.clone(),
                line: d.line.map(|l| l as u32),
                column: d.column.map(|c| c as u32),
                page: d.page.map(|p| p as u32 - 1),
            })
            .collect();
        let starts = report
            .problems
            .iter()
            .map(|p| PreviewStart {
                label: p.label.clone(),
                page: p.first as u32 - 1,
            })
            .collect();
        let error = first_error(&report);
        let Some(compiled) = compiled else {
            return PreviewResult {
                error,
                diagnostics,
                compile_ms,
                ..Default::default()
            };
        };
        let mut result = self.rasterize(&compiled, scale, view);
        result.error = error;
        result.diagnostics = diagnostics;
        result.starts = starts;
        result.compile_ms = compile_ms;
        result
    }

    fn rasterize(
        &mut self,
        compiled: &Compiled,
        scale: f32,
        view: &[PreviewView],
    ) -> PreviewResult {
        let compile_ms = 0.0;
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
        let rendered = render(compiled, &jobs);
        PIXELS.lock().unwrap().extend(rendered);
        let render_ms = start.elapsed().as_secs_f64() * 1000.0;

        let keys = pages
            .iter()
            .flat_map(|p| p.layers.iter().map(|l| l.key.clone()))
            .collect();
        self.retire(keys);
        PreviewResult {
            pages,
            compile_ms,
            render_ms,
            rendered: jobs.len() as u32,
            ..Default::default()
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

/// The statement file set in `language` (`statement.md` for a problem that has none yet).
fn statement_path(p: &xcpc_domain::Problem, language: xcpc_domain::Language) -> String {
    p.statement(language)
        .map(|s| s.file.path.clone())
        .unwrap_or_else(|| format!("{}/statement.md", p.dir))
}

/// The first error, `file:line: message`.
fn first_error(report: &publish::Report) -> Option<String> {
    let d = report.diagnostics.iter().find(|d| d.is_error())?;
    Some(match (&d.file, d.line) {
        (Some(f), Some(l)) => format!("{f}:{l}: {}", d.message),
        _ => d.message.clone(),
    })
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
