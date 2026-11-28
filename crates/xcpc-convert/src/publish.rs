//! The publishing pipeline the GUI, the CLI and MCP share: workspace → Typst →
//! diagnostics, pages and PDF.
//!
//! [`document`] reads a problem or a contest and converts it; [`compile`] typesets it
//! with an [`Engine`] rooted at the workspace and reports diagnostics in workspace
//! files (`problems/a/statement.md:3:5`), where the problems start, and overflows.
//! [`check`] combines conversion, compilation, overflow checks and `xcpc-lint` into one
//! deduplicated [`Report`]; [`locate_point`] maps preview coordinates back to Markdown;
//! [`freeze_contest`] validates, builds PDFs and atomically archives a contest snapshot.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use serde::Serialize;
use xcpc_domain::{
    ContestItem, ContestMeta, Language, LocalizedText, SnippetVerifyStatus, Statement,
    TemplateFieldScope, TemplateFieldType, TemplateInfo, TemplateKind, TemplateOptionValue,
    TemplateOptionsMap, TemplateSource, TextFile, TrdFreezeResult, TrdPreflightSeverity, check_id,
    format_plugin_template_ref, label_for_index, parse_plugin_template_ref,
};
use xcpc_engine::{Compiled, Engine, PluginMount};
use xcpc_storage::{Error, SnapshotTemplateArchive, SnapshotTemplateMeta, Workspace};

use crate::{Book, Document, Entry, TrdBook, TrdChapterInput};

/// Diagnostic code emitted when the resolved `.typ` template file does not exist on disk.
pub const MISSING_TEMPLATE: &str = "missing_template";
/// Diagnostic code emitted when a referenced `plugin:<plugin_id>/<template_id>` template
/// is not installed, disabled, untrusted, or missing from the plugin.
pub const PLUGIN_TEMPLATE_NOT_FOUND: &str = "PLUGIN_TEMPLATE_NOT_FOUND";

/// What to publish.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Target {
    Problem(String),
    Contest(String),
}

impl Target {
    pub fn id(&self) -> &str {
        match self {
            Target::Problem(id) | Target::Contest(id) => id,
        }
    }

    /// `problem` or `contest`.
    pub fn kind(&self) -> &'static str {
        match self {
            Target::Problem(_) => "problem",
            Target::Contest(_) => "contest",
        }
    }
}

/// Which book.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Kind {
    /// The statements (a contest's with its cover).
    #[default]
    Statements,
    /// The A4 editorial book.
    Editorial,
}

/// Reads `target` from the workspace and converts it. `language` overrides a single
/// problem's (a contest has its own).
pub fn document(
    ws: &Workspace,
    target: &Target,
    kind: Kind,
    language: Option<Language>,
) -> Result<Document, Error> {
    document_with(ws, target, kind, language, None)
}

/// [`document`], with the text of one workspace file (`problems/a/statement.md`) as an
/// editor has it rather than as it is on disk.
pub fn document_with(
    ws: &Workspace,
    target: &Target,
    kind: Kind,
    language: Option<Language>,
    unsaved: Option<(&str, &str)>,
) -> Result<Document, Error> {
    document_with_engine(ws, None, target, kind, language, unsaved, None)
}

/// [`document_with`] extended with an optional [`Engine`] (for resolving mounted
/// `plugin:<plugin_id>/<template_id>` templates) and optional `--template` override.
pub fn document_with_engine(
    ws: &Workspace,
    engine: Option<&Engine>,
    target: &Target,
    kind: Kind,
    language: Option<Language>,
    unsaved: Option<(&str, &str)>,
    template_override: Option<&str>,
) -> Result<Document, Error> {
    let _span = xcpc_trace::span_with("convert", "build_document", || {
        serde_json::json!({
            "kind": target.kind(),
            "id": target.id(),
            "hasUnsaved": unsaved.is_some(),
        })
    });
    let overlay = |problem: &mut xcpc_domain::Problem| {
        let Some((path, text)) = unsaved else { return };
        let mut matched = false;
        for stmt in &mut problem.statements {
            if stmt.file.path == path {
                stmt.file.text = text.to_owned();
                matched = true;
            }
        }
        if let Some(ed) = problem.editorial.as_mut()
            && ed.path == path
        {
            ed.text = text.to_owned();
            matched = true;
        }
        if !matched && !text.is_empty() {
            let ed_path = format!("{}/editorial.md", problem.dir);
            if path == ed_path {
                problem.editorial = Some(TextFile {
                    path: ed_path,
                    text: text.to_owned(),
                });
                return;
            }
            for (lang, name) in [
                (None, "statement.md"),
                (Some(Language::Chinese), "statement.zh.md"),
                (Some(Language::English), "statement.en.md"),
            ] {
                let stmt_path = format!("{}/{name}", problem.dir);
                if path == stmt_path {
                    problem.statements.push(Statement {
                        language: lang,
                        file: TextFile {
                            path: stmt_path,
                            text: text.to_owned(),
                        },
                    });
                    break;
                }
            }
        }
    };
    let ws_template_opts = ws.workspace_template_options();
    let merge_book_options = |tpl_name: &str, contest_opts: Option<&TemplateOptionsMap>| {
        let mut merged: BTreeMap<String, TemplateOptionValue> =
            ws_template_opts.get(tpl_name).cloned().unwrap_or_default();
        if let Some(c_opts) = contest_opts.and_then(|m| m.get(tpl_name)) {
            for (k, v) in c_opts {
                merged.insert(k.clone(), v.clone());
            }
        }
        merged
    };
    let template_override = template_override.map(str::trim).filter(|s| !s.is_empty());
    let resolve_workspace_template = |book: &mut Book,
                                      contest_meta: Option<&ContestMeta>,
                                      diag_file: String|
     -> (Option<String>, Option<crate::Diagnostic>) {
        book.preamble = ws.load_preamble();
        let contest_editorial_template = contest_meta
            .and_then(|m| m.editorial_template.as_deref())
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let contest_template = contest_meta
            .and_then(|m| m.template.as_deref())
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let name = if kind == Kind::Editorial {
            template_override
                .map(str::to_owned)
                .or_else(|| contest_editorial_template.map(str::to_owned))
                .or_else(|| ws.default_editorial_template())
                .or_else(|| contest_template.map(str::to_owned))
                .or_else(|| ws.default_template())
                .unwrap_or_else(|| "olymp-classic".into())
        } else {
            template_override
                .map(str::to_owned)
                .or_else(|| contest_template.map(str::to_owned))
                .or_else(|| ws.default_template())
                .unwrap_or_else(|| "olymp-classic".into())
        };
        if let Some((plugin_id, template_id)) = parse_plugin_template_ref(&name) {
            if let Some((mount, tpl_entry)) =
                engine.and_then(|e| e.find_plugin_template(plugin_id, template_id))
            {
                book.template_name = name.clone();
                book.template_import =
                    Some(format!("/plugins/{}/{}", mount.plugin_id, tpl_entry.entry));
                let mut opts = merge_book_options(&name, contest_meta.map(|m| &m.template_options));
                if let Some(meta) = contest_meta {
                    fold_legacy_contest_options(meta, &mut opts);
                }
                book.template_options = opts;
                (Some(mount.plugin_id.clone()), None)
            } else {
                book.template_name = name.clone();
                book.template_import = None;
                (
                    None,
                    Some(crate::Diagnostic {
                        error: true,
                        code: Some(PLUGIN_TEMPLATE_NOT_FOUND.into()),
                        message: format!(
                            "未找到插件模板「{name}」（插件 {plugin_id} 未安装、未启用或不包含模板 {template_id}）"
                        ),
                        file: diag_file,
                        line: 1,
                        column: 1,
                    }),
                )
            }
        } else if check_id(&name).is_ok() && ws.has_custom_template(&name) {
            book.template_name = name.clone();
            book.template_import = Some(format!("/templates/{name}.typ"));
            let mut opts = merge_book_options(&name, contest_meta.map(|m| &m.template_options));
            if let Some(meta) = contest_meta {
                fold_legacy_contest_options(meta, &mut opts);
            }
            book.template_options = opts;
            (None, None)
        } else {
            book.template_name = name.clone();
            book.template_import = None;
            (
                None,
                Some(crate::Diagnostic {
                    error: true,
                    code: Some(MISSING_TEMPLATE.into()),
                    message: format!(
                        "未找到模板「{name}」（templates/{name}.typ 不存在）；请从预设安装或在 templates/ 中提供该模板"
                    ),
                    file: diag_file,
                    line: 1,
                    column: 1,
                }),
            )
        }
    };
    match target {
        Target::Problem(id) => {
            let mut problem = ws.load_problem(id)?;
            overlay(&mut problem);
            let mut book = Book::single(language.unwrap_or_else(|| problem.default_language()));
            let (active_plugin_id, err) =
                resolve_workspace_template(&mut book, None, "workspace.toml".into());
            let entries = [Entry::single(&problem)];
            let mut doc = match kind {
                Kind::Statements => crate::statements(&book, &entries),
                Kind::Editorial => crate::editorials(&book, &entries),
            };
            doc.active_plugin_id = active_plugin_id;
            if let Some(d) = err {
                doc.diagnostics.push(d);
            }
            Ok(doc)
        }
        Target::Contest(id) => {
            let contest = ws.load_contest(id)?;
            let mut problems = ws.contest_problems(&contest)?;
            for (_, problem) in &mut problems {
                overlay(problem);
            }
            let mut book = Book::contest(&contest.meta);
            if let Some(language) = language {
                book.language = language;
            }
            let mut active_plugin_id: Option<String> = None;
            let mut extra_font_key: Option<String> = None;
            let mut extra_font_dirs: Vec<PathBuf> = Vec::new();
            let err = if contest.meta.frozen && template_override.is_none() {
                let ed_snap = (kind == Kind::Editorial)
                    .then(|| ws.load_snapshot_editorial_template(id))
                    .flatten();
                let ed_bundle_entry =
                    ed_snap
                        .as_ref()
                        .and_then(|s| s.entry.as_deref())
                        .and_then(|entry_rel| {
                            let rel =
                                format!("contests/{id}/snapshot/editorial-template/{entry_rel}");
                            ws.resolve(&rel).ok().filter(|p| p.is_file()).map(|_| rel)
                        });
                let ed_tpl_rel = format!("contests/{id}/snapshot/editorial-template.typ");
                let has_ed_tpl = kind == Kind::Editorial
                    && ed_snap.is_some()
                    && ws.resolve(&ed_tpl_rel).is_ok_and(|p| p.is_file());

                if let Some(bundle_rel) = ed_bundle_entry {
                    book.template_name = ed_snap
                        .as_ref()
                        .map(|s| s.template.clone())
                        .unwrap_or_else(|| "local".into());
                    book.template_options = ed_snap
                        .as_ref()
                        .map(|s| s.options.clone())
                        .unwrap_or_default();
                    book.preamble = ws.load_snapshot_preamble(id);
                    book.template_import = Some(format!("/{bundle_rel}"));
                    if let Some(s) = &ed_snap
                        && !s.font_dirs.is_empty()
                    {
                        extra_font_key = Some(format!("contest-snap-ed:{id}"));
                        for dir in &s.font_dirs {
                            let rel = format!("contests/{id}/snapshot/editorial-template/{dir}");
                            if let Ok(abs) = ws.resolve(&rel)
                                && abs.is_dir()
                            {
                                extra_font_dirs.push(abs);
                            }
                        }
                    }
                    None
                } else if has_ed_tpl {
                    book.template_name = ed_snap
                        .as_ref()
                        .map(|s| s.template.clone())
                        .unwrap_or_else(|| "local".into());
                    book.template_options = ed_snap
                        .as_ref()
                        .map(|s| s.options.clone())
                        .unwrap_or_default();
                    book.preamble = ws.load_snapshot_preamble(id);
                    book.template_import = Some(format!("/{ed_tpl_rel}"));
                    None
                } else {
                    let snap = ws.load_snapshot_template(id);
                    let snap_bundle_entry = snap
                        .as_ref()
                        .and_then(|s| s.entry.as_deref())
                        .and_then(|entry_rel| {
                            let rel = format!("contests/{id}/snapshot/template/{entry_rel}");
                            ws.resolve(&rel).ok().filter(|p| p.is_file()).map(|_| rel)
                        });
                    let snap_tpl_rel = format!("contests/{id}/snapshot/template.typ");
                    let has_snap_tpl = ws.resolve(&snap_tpl_rel).is_ok_and(|p| p.is_file());
                    if let Some(bundle_rel) = snap_bundle_entry {
                        book.template_name = snap
                            .as_ref()
                            .map(|s| s.template.clone())
                            .unwrap_or_else(|| "local".into());
                        book.template_options =
                            snap.as_ref().map(|s| s.options.clone()).unwrap_or_default();
                        book.preamble = ws.load_snapshot_preamble(id);
                        book.template_import = Some(format!("/{bundle_rel}"));
                        if let Some(s) = &snap
                            && !s.font_dirs.is_empty()
                        {
                            extra_font_key = Some(format!("contest-snap:{id}"));
                            for dir in &s.font_dirs {
                                let rel = format!("contests/{id}/snapshot/template/{dir}");
                                if let Ok(abs) = ws.resolve(&rel)
                                    && abs.is_dir()
                                {
                                    extra_font_dirs.push(abs);
                                }
                            }
                        }
                        None
                    } else if has_snap_tpl {
                        book.template_name = snap
                            .as_ref()
                            .map(|s| s.template.clone())
                            .unwrap_or_else(|| "local".into());
                        book.template_options =
                            snap.as_ref().map(|s| s.options.clone()).unwrap_or_default();
                        book.preamble = ws.load_snapshot_preamble(id);
                        book.template_import = Some(format!("/{snap_tpl_rel}"));
                        None
                    } else {
                        book.template_import = None;
                        Some(crate::Diagnostic {
                            error: true,
                            code: Some(MISSING_TEMPLATE.into()),
                            message: format!(
                                "定稿比赛「{id}」缺少快照模板文件 {snap_tpl_rel}，无法复现排版"
                            ),
                            file: format!("contests/{id}/contest.toml"),
                            line: 1,
                            column: 1,
                        })
                    }
                }
            } else if template_override.is_none()
                && !(kind == Kind::Editorial
                    && (contest
                        .meta
                        .editorial_template
                        .as_deref()
                        .is_some_and(|s| !s.trim().is_empty())
                        || ws.default_editorial_template().is_some()))
                && ws.has_contest_template(id)
            {
                book.template_name = "local".into();
                book.template_import = Some(format!("/contests/{id}/template.typ"));
                let mut opts = contest
                    .meta
                    .template_options
                    .get("local")
                    .cloned()
                    .unwrap_or_default();
                fold_legacy_contest_options(&contest.meta, &mut opts);
                book.template_options = opts;
                book.preamble = ws.load_preamble();
                None
            } else {
                let diag_file = if (kind == Kind::Editorial
                    && contest
                        .meta
                        .editorial_template
                        .as_deref()
                        .is_some_and(|s| !s.trim().is_empty()))
                    || contest
                        .meta
                        .template
                        .as_deref()
                        .is_some_and(|s| !s.trim().is_empty())
                {
                    format!("contests/{id}/contest.toml")
                } else {
                    "workspace.toml".into()
                };
                let (pid, err) =
                    resolve_workspace_template(&mut book, Some(&contest.meta), diag_file);
                active_plugin_id = pid;
                err
            };
            let entries: Vec<Entry> = problems
                .iter()
                .map(|(item, problem)| Entry {
                    item: item.clone(),
                    problem,
                })
                .collect();
            let mut doc = match kind {
                Kind::Statements => crate::statements(&book, &entries),
                Kind::Editorial => crate::editorials(&book, &entries),
            };
            doc.active_plugin_id = active_plugin_id;
            doc.extra_font_key = extra_font_key;
            doc.extra_font_dirs = extra_font_dirs;
            if let Some(d) = err {
                doc.diagnostics.push(d);
            }
            Ok(doc)
        }
    }
}

fn fold_legacy_contest_options(
    meta: &ContestMeta,
    opts: &mut BTreeMap<String, TemplateOptionValue>,
) {
    if let Some(cover) = meta.cover {
        opts.entry("cover".into()).or_insert_with(|| {
            TemplateOptionValue::String(if cover { "on" } else { "off" }.into())
        });
    }
    if meta.blank_pages {
        opts.entry("blank-pages".into())
            .or_insert(TemplateOptionValue::Bool(true));
    }
    if !meta.show_metadata {
        opts.entry("show-metadata".into())
            .or_insert(TemplateOptionValue::Bool(false));
    }
}

/// A diagnostic for people and agents.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Diagnostic {
    /// `error` or `warning`.
    pub severity: &'static str,
    /// Stable machine-readable rule code when emitted by preflight/lint checks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    pub message: String,
    /// A workspace file (`problems/a/statement.md`), or a bundled template
    /// (`/xcpc/olymp-classic.typ`); none when it points nowhere.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<usize>,
    /// The page (1-based) a layout warning is about.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<usize>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub hints: Vec<String>,
}

impl Diagnostic {
    pub fn is_error(&self) -> bool {
        self.severity == "error"
    }

    /// `file:line:column: severity: message`.
    pub fn display(&self) -> String {
        let at = match (&self.file, self.line, self.column) {
            (Some(f), Some(l), Some(c)) => format!("{f}:{l}:{c}: "),
            (Some(f), _, _) => format!("{f}: "),
            _ => String::new(),
        };
        let page = self
            .page
            .map(|p| format!(" (page {p})"))
            .unwrap_or_default();
        format!("{at}{}: {}{page}", self.severity, self.message)
    }
}

/// Where a problem's pages are.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Pages {
    pub label: String,
    pub id: String,
    /// Its first page and how many it takes (1-based, physical pages).
    pub first: usize,
    pub count: usize,
}

/// What compiling gave.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Report {
    pub ok: bool,
    /// Pages in all (none when it failed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pages: Option<usize>,
    pub problems: Vec<Pages>,
    pub diagnostics: Vec<Diagnostic>,
    pub compile_ms: u64,
}

/// A 1-based location inside a workspace Markdown source file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceLocation {
    pub file: String,
    pub line: usize,
    pub column: usize,
}

/// Result of [`freeze_contest`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FreezeOutcome {
    pub frozen: bool,
    pub report: Report,
    pub outputs: Vec<String>,
}

/// Options for compiling an ad-hoc merged PDF booklet from selected problems without
/// creating any `contests/<id>/` directory.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MergedBookOptions {
    /// Optional booklet title (shown on cover / running head and used for default filename).
    pub title: Option<String>,
    /// Optional statement/editorial language override (defaults to the first problem's default language).
    pub language: Option<Language>,
    /// Statements booklet or editorial booklet (defaults to [`Kind::Statements`]).
    pub kind: Kind,
    /// Optional template ID override (falls back to `ws.default_template()` -> `"olymp-classic"`).
    pub template: Option<String>,
    /// Optional cover toggle override (defaults to `entries.len() >= 2` for statements).
    pub cover: Option<bool>,
}

/// Result of [`export_merged_pdf`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MergedPdfOutcome {
    /// Workspace-relative path of the written PDF (empty when compilation failed).
    pub path: String,
    /// Resolved booklet title.
    pub title: String,
    /// Total pages of the exported PDF (`0` when compilation failed).
    pub pages: usize,
    /// Full compilation report including diagnostics and per-problem page spans.
    pub report: Report,
}

fn dedup_problem_ids(problem_ids: &[String]) -> Vec<String> {
    let mut seen = HashSet::with_capacity(problem_ids.len());
    let mut out = Vec::with_capacity(problem_ids.len());
    for id in problem_ids {
        if seen.insert(id.clone()) {
            out.push(id.clone());
        }
    }
    out
}

fn resolve_merged_title(problem_count: usize, opts: &MergedBookOptions) -> String {
    opts.title
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("题集（{problem_count} 题）"))
}

/// Default workspace-relative output path for an ad-hoc merged PDF:
/// `build/<sanitized_title>-试题册.pdf` / `build/<sanitized_title>-题解.pdf`,
/// falling back to `build/题集-<N>题-试题册.pdf` / `build/题集-<N>题-题解.pdf`.
pub fn default_merged_output(problem_count: usize, opts: &MergedBookOptions) -> String {
    let base = opts
        .title
        .as_deref()
        .map(sanitize_filename)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("题集-{problem_count}题"));
    match opts.kind {
        Kind::Statements => format!("build/{base}-试题册.pdf"),
        Kind::Editorial => format!("build/{base}-题解.pdf"),
    }
}

/// Builds a [`Document`] combining `problem_ids` into a single statements or editorial
/// booklet without creating any `contests/<id>/` directory.
///
/// - Deduplicates `problem_ids` preserving caller order.
/// - Labels problems `"A"..="Z"` when `len <= 26`, and `"1"..="N"` when `len > 26`.
/// - Resolves template via `opts.template` -> `ws.default_template()` -> `"olymp-classic"`,
///   inheriting workspace `[template-options]` and `templates/preamble.typ`.
pub fn document_problems(
    ws: &Workspace,
    problem_ids: &[String],
    opts: &MergedBookOptions,
) -> Result<Document, Error> {
    document_problems_with_engine(ws, None, problem_ids, opts)
}

/// [`document_problems`] with an optional [`Engine`] for resolving mounted `plugin:` templates.
pub fn document_problems_with_engine(
    ws: &Workspace,
    engine: Option<&Engine>,
    problem_ids: &[String],
    opts: &MergedBookOptions,
) -> Result<Document, Error> {
    let unique_ids = dedup_problem_ids(problem_ids);
    if unique_ids.is_empty() {
        return Err(Error::Invalid {
            path: "problems".into(),
            message: "at least one problem ID is required".into(),
        });
    }

    let mut problems = Vec::with_capacity(unique_ids.len());
    for id in &unique_ids {
        problems.push(ws.load_problem(id)?);
    }

    let language = opts
        .language
        .unwrap_or_else(|| problems[0].default_language());
    let title = resolve_merged_title(problems.len(), opts);

    let mut book = Book {
        name: title,
        location: None,
        date: None,
        author: None,
        language,
        show_metadata: true,
        cover: opts.cover.unwrap_or(problems.len() >= 2),
        blank_pages: false,
        single: false,
        template_name: "olymp-classic".into(),
        template_import: None,
        template_options: BTreeMap::new(),
        preamble: ws.load_preamble(),
    };

    let tpl_name = opts
        .template
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            if opts.kind == Kind::Editorial {
                ws.default_editorial_template()
            } else {
                None
            }
        })
        .or_else(|| ws.default_template())
        .unwrap_or_else(|| "olymp-classic".into());

    let mut active_plugin_id: Option<String> = None;
    let missing_tpl_diag = if let Some((pid, tid)) = parse_plugin_template_ref(&tpl_name) {
        if let Some((mount, tpl_entry)) = engine.and_then(|e| e.find_plugin_template(pid, tid)) {
            book.template_name = tpl_name.clone();
            book.template_import =
                Some(format!("/plugins/{}/{}", mount.plugin_id, tpl_entry.entry));
            let mut merged_opts = ws
                .workspace_template_options()
                .remove(&tpl_name)
                .unwrap_or_default();
            if let Some(cover) = opts.cover {
                merged_opts.insert(
                    "cover".into(),
                    TemplateOptionValue::String(if cover { "on" } else { "off" }.into()),
                );
            }
            book.template_options = merged_opts;
            active_plugin_id = Some(mount.plugin_id.clone());
            None
        } else {
            book.template_name = tpl_name.clone();
            book.template_import = None;
            Some(crate::Diagnostic {
                error: true,
                code: Some(PLUGIN_TEMPLATE_NOT_FOUND.into()),
                message: format!(
                    "未找到插件模板「{tpl_name}」（插件 {pid} 未安装、未启用或不包含模板 {tid}）"
                ),
                file: "workspace.toml".into(),
                line: 1,
                column: 1,
            })
        }
    } else if check_id(&tpl_name).is_ok() && ws.has_custom_template(&tpl_name) {
        book.template_name = tpl_name.clone();
        book.template_import = Some(format!("/templates/{tpl_name}.typ"));
        let mut merged_opts = ws
            .workspace_template_options()
            .remove(&tpl_name)
            .unwrap_or_default();
        if let Some(cover) = opts.cover {
            merged_opts.insert(
                "cover".into(),
                TemplateOptionValue::String(if cover { "on" } else { "off" }.into()),
            );
        }
        book.template_options = merged_opts;
        None
    } else {
        book.template_name = tpl_name.clone();
        book.template_import = None;
        Some(crate::Diagnostic {
            error: true,
            code: Some(MISSING_TEMPLATE.into()),
            message: format!(
                "未找到模板「{tpl_name}」（templates/{tpl_name}.typ 不存在）；请从预设安装或在 templates/ 中提供该模板"
            ),
            file: "workspace.toml".into(),
            line: 1,
            column: 1,
        })
    };

    let use_letters = problems.len() <= 26;
    let entries: Vec<Entry> = problems
        .iter()
        .enumerate()
        .map(|(i, problem)| {
            let label = if use_letters {
                label_for_index(i).unwrap_or_else(|| (i + 1).to_string())
            } else {
                (i + 1).to_string()
            };
            Entry {
                item: ContestItem {
                    id: problem.id.clone(),
                    label,
                    ..Default::default()
                },
                problem,
            }
        })
        .collect();

    let mut doc = match opts.kind {
        Kind::Statements => crate::statements(&book, &entries),
        Kind::Editorial => crate::editorials(&book, &entries),
    };
    doc.active_plugin_id = active_plugin_id;
    if let Some(d) = missing_tpl_diag {
        doc.diagnostics.push(d);
    }
    Ok(doc)
}

/// Compiles `problem_ids` into an ad-hoc merged PDF in memory without writing to disk.
///
/// Useful for GUI save-file dialogs targeting arbitrary paths outside the workspace.
/// Returns `(Some(pdf_bytes), report, resolved_title)` on success, or `(None, report, resolved_title)`
/// when compilation or PDF generation produced errors.
pub fn compile_merged_pdf_bytes(
    ws: &Workspace,
    engine: &mut Engine,
    problem_ids: &[String],
    opts: &MergedBookOptions,
) -> Result<(Option<Vec<u8>>, Report, String), Error> {
    let unique_ids = dedup_problem_ids(problem_ids);
    let doc = document_problems_with_engine(ws, Some(engine), &unique_ids, opts)?;
    let title = resolve_merged_title(unique_ids.len(), opts);

    engine.reset_files();
    let (compiled, mut report) = compile(engine, &doc);
    let Some(compiled) = compiled.filter(|_| report.ok) else {
        report.ok = false;
        return Ok((None, report, title));
    };

    match compiled.pdf() {
        Ok(bytes) => Ok((Some(bytes), report, title)),
        Err(errs) => {
            report.ok = false;
            merge_diagnostics(
                &mut report.diagnostics,
                errs.into_iter().map(from_engine_plain).collect(),
            );
            Ok((None, report, title))
        }
    }
}

/// Compiles `problem_ids` into a single merged PDF and writes it atomically into the
/// workspace (at `output_rel`, or [`default_merged_output`] when `None`).
///
/// Enforces `ws.is_read_only()` and workspace path sandbox via [`Workspace::write`].
/// Never creates any `contests/<id>/` directory.
pub fn export_merged_pdf(
    ws: &Workspace,
    engine: &mut Engine,
    problem_ids: &[String],
    opts: &MergedBookOptions,
    output_rel: Option<&Path>,
) -> Result<MergedPdfOutcome, Error> {
    if ws.is_read_only() {
        return Err(Error::ReadOnly);
    }

    let unique_ids = dedup_problem_ids(problem_ids);
    let rel_path = match output_rel {
        Some(p) => p.to_string_lossy().into_owned(),
        None => default_merged_output(unique_ids.len(), opts),
    };
    // Validate sandbox & writability upfront before spending time compiling Typst.
    ws.check_writable(&rel_path)?;

    let (pdf_bytes, report, title) = compile_merged_pdf_bytes(ws, engine, &unique_ids, opts)?;
    let Some(bytes) = pdf_bytes else {
        return Ok(MergedPdfOutcome {
            path: String::new(),
            title,
            pages: 0,
            report,
        });
    };

    ws.write(&rel_path, &bytes)?;
    let pages = report.pages.unwrap_or(0);
    Ok(MergedPdfOutcome {
        path: rel_path,
        title,
        pages,
        report,
    })
}

/// Typesets `doc` with `engine` (rooted at the workspace).
pub fn compile(engine: &mut Engine, doc: &Document) -> (Option<Compiled>, Report) {
    let _span = xcpc_trace::span_with(
        "convert",
        "publish_compile",
        || serde_json::json!({ "sourceBytes": doc.source.len() }),
    );
    let mut diagnostics: Vec<Diagnostic> = doc
        .diagnostics
        .iter()
        .map(|d| Diagnostic {
            severity: if d.error { "error" } else { "warning" },
            code: d.code.clone(),
            message: d.message.clone(),
            file: Some(d.file.clone()),
            line: Some(d.line),
            column: Some(d.column),
            page: None,
            hints: Vec::new(),
        })
        .collect();
    if diagnostics.iter().any(|d| {
        d.is_error()
            && matches!(
                d.code.as_deref(),
                Some(MISSING_TEMPLATE) | Some(PLUGIN_TEMPLATE_NOT_FOUND)
            )
    }) {
        return (
            None,
            Report {
                ok: false,
                pages: None,
                problems: Vec::new(),
                diagnostics,
                compile_ms: 0,
            },
        );
    }
    if let Some(pid) = doc.active_plugin_id.as_deref() {
        engine.activate_plugin_fonts(Some(pid));
    } else if let Some(key) = doc.extra_font_key.as_deref()
        && !doc.extra_font_dirs.is_empty()
    {
        engine.activate_extra_font_dirs(key, &doc.extra_font_dirs);
    } else {
        engine.activate_plugin_fonts(None);
    }
    engine.set_main(&doc.source);
    let map = |d: &xcpc_engine::Diagnostic| {
        let mut out = Diagnostic {
            severity: if d.error { "error" } else { "warning" },
            code: None,
            message: d.message.clone(),
            file: d.file.clone(),
            line: d.line,
            column: d.column,
            page: d.page,
            hints: d.hints.clone(),
        };
        if d.file.as_deref() == Some("/main.typ") {
            match d.offset.and_then(|o| doc.map.locate(o)) {
                Some((file, line, column)) => {
                    out.file = Some(file.to_owned());
                    out.line = Some(line);
                    out.column = Some(column);
                }
                // Generated code with no source: say nothing misleading.
                None => (out.file, out.line, out.column) = (None, None, None),
            }
        }
        out
    };
    let start = std::time::Instant::now();
    let mut result = engine.compile();
    if let Ok(ref compiled) = result
        && compiled
            .warnings
            .iter()
            .any(|w| w.message.starts_with("unknown font family:"))
        && engine.ensure_full_system_fonts()
    {
        result = engine.compile();
    }
    let compile_ms = start.elapsed().as_millis() as u64;
    match result {
        Ok(compiled) => {
            diagnostics.extend(compiled.warnings.iter().map(map));
            let total = compiled.page_count();
            let mut starts = compiled.starts("xcpc-problem");
            if starts.is_empty() {
                starts = compiled.starts("xcpc-tutorial");
            }
            if starts.is_empty() {
                starts = compiled.starts("xcpc-trd-item");
            }
            let problems = pages(doc, &starts, total);
            let report = Report {
                ok: !diagnostics.iter().any(Diagnostic::is_error),
                pages: Some(total),
                problems,
                diagnostics,
                compile_ms,
            };
            (Some(compiled), report)
        }
        Err(e) => {
            diagnostics.extend(e.errors.iter().map(map));
            diagnostics.extend(e.warnings.iter().map(map));
            let report = Report {
                ok: false,
                pages: None,
                problems: Vec::new(),
                diagnostics,
                compile_ms,
            };
            (None, report)
        }
    }
}

/// Runs full preflight checking (conversion, compilation, overflow detection and
/// `xcpc-lint`) for `target`, merging and deduplicating all diagnostics into one
/// [`Report`].
///
/// - If `target` itself (`problem <id>` or `contest <id>`) does not exist, returns
///   `Err(Error::NotFound)`.
/// - If a contest references one or more missing problems, records `missing_problem`
///   error(s) instead of returning `Err(Error::NotFound)`, still runs lint on every
///   remaining existing problem, and returns `Ok((None, None, report))` with
///   `report.ok == false`.
pub fn check(
    ws: &Workspace,
    engine: &mut Engine,
    target: &Target,
    kind: Kind,
    language: Option<Language>,
    unsaved: Option<(&str, &str)>,
) -> Result<(Option<Compiled>, Option<Document>, Report), Error> {
    check_inner(
        ws,
        engine,
        target,
        kind,
        language,
        unsaved,
        kind == Kind::Editorial,
        false,
        None,
    )
}

/// [`check`] with an optional `--template` override (workspace template ID or `plugin:<pid>/<tid>`).
pub fn check_with_template(
    ws: &Workspace,
    engine: &mut Engine,
    target: &Target,
    kind: Kind,
    language: Option<Language>,
    unsaved: Option<(&str, &str)>,
    template_override: Option<&str>,
) -> Result<(Option<Compiled>, Option<Document>, Report), Error> {
    check_inner(
        ws,
        engine,
        target,
        kind,
        language,
        unsaved,
        kind == Kind::Editorial,
        false,
        template_override,
    )
}

/// Interactive preview variant of [`check`] that skips static package freshness hashing
/// (`tests/`, `samples/`, etc.), since the preview HUD filters out `package_*` diagnostics
/// and package freshness is tracked independently by the authoring status slot.
pub fn check_preview(
    ws: &Workspace,
    engine: &mut Engine,
    target: &Target,
    kind: Kind,
    language: Option<Language>,
    unsaved: Option<(&str, &str)>,
) -> Result<(Option<Compiled>, Option<Document>, Report), Error> {
    check_inner(
        ws,
        engine,
        target,
        kind,
        language,
        unsaved,
        kind == Kind::Editorial,
        true,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn check_inner(
    ws: &Workspace,
    engine: &mut Engine,
    target: &Target,
    kind: Kind,
    language: Option<Language>,
    unsaved: Option<(&str, &str)>,
    check_editorial: bool,
    skip_package_freshness: bool,
    template_override: Option<&str>,
) -> Result<(Option<Compiled>, Option<Document>, Report), Error> {
    let lint_opts = xcpc_lint::LintOptions {
        language,
        check_editorial,
        unsaved,
        skip_package_freshness,
    };
    let lint_findings = {
        let _lint_span = xcpc_trace::span_with(
            "convert",
            "lint",
            || serde_json::json!({ "skipPackageFreshness": skip_package_freshness }),
        );
        match target {
            Target::Problem(id) => xcpc_lint::lint_problem(ws, id, &lint_opts)?,
            Target::Contest(id) => xcpc_lint::lint_contest(ws, id, &lint_opts)?,
        }
    };

    let has_missing_problem = lint_findings
        .iter()
        .any(|f| f.code == xcpc_lint::code::MISSING_PROBLEM);
    if has_missing_problem {
        let mut diagnostics = Vec::new();
        merge_diagnostics(
            &mut diagnostics,
            lint_findings.into_iter().map(from_lint).collect(),
        );
        let report = Report {
            ok: !diagnostics.iter().any(Diagnostic::is_error),
            pages: None,
            problems: Vec::new(),
            diagnostics,
            compile_ms: 0,
        };
        return Ok((None, None, report));
    }

    let doc = document_with_engine(
        ws,
        Some(engine),
        target,
        kind,
        language,
        unsaved,
        template_override,
    )?;
    // Preserve cached workspace templates & asset bytes during pure in-memory edits
    // (disk/watcher mutations reset via workspace revision or `unsaved.is_none()`).
    if unsaved.is_none() {
        engine.reset_files();
    }
    let (compiled, mut report) = compile(engine, &doc);
    merge_diagnostics(
        &mut report.diagnostics,
        lint_findings.into_iter().map(from_lint).collect(),
    );
    report.ok = compiled.is_some() && !report.diagnostics.iter().any(Diagnostic::is_error);
    Ok((compiled, Some(doc), report))
}

/// Maps a point `(x_pt, y_pt)` (typographic points from the top-left of 0-based `page`)
/// on a rendered preview page back to its Markdown source file, line and column (1-based).
///
/// Expects `engine` to currently hold `doc.source` (as set by [`compile`] or [`check`]).
pub fn locate_point(
    engine: &Engine,
    compiled: &Compiled,
    doc: &Document,
    page: usize,
    x_pt: f64,
    y_pt: f64,
) -> Option<SourceLocation> {
    let offset = engine.locate_point(compiled, page, x_pt, y_pt)?;
    let (file, line, column) = doc.map.locate(offset)?;
    Some(SourceLocation {
        file: file.to_owned(),
        line,
        column,
    })
}

/// Validates, compiles, exports PDFs and atomically freezes `contest_id`.
///
/// - Enforces non-read-only workspace, contest existence and unfrozen status.
/// - Runs full statements compilation and preflight lint (including `missing_editorial`
///   warnings). Any error (`missing_problem`, `missing_image`, compile error) aborts
///   without touching disk; warnings never block freezing.
/// - Exports the statements PDF, and if at least one problem has `editorial.md`,
///   compiles and exports the editorial PDF too (aborting without writing if editorial
///   compilation produces any error).
/// - Atomically commits the snapshot and flips `frozen = true` via
///   [`Workspace::freeze_commit`].
pub fn freeze_contest(
    ws: &Workspace,
    engine: &mut Engine,
    contest_id: &str,
) -> Result<FreezeOutcome, Error> {
    check_id(contest_id)?;
    if ws.is_read_only() {
        return Err(Error::ReadOnly);
    }
    let contest = ws.load_contest(contest_id)?;
    if contest.meta.archived {
        return Err(Error::Archived(contest_id.to_owned()));
    }
    if contest.meta.frozen {
        return Err(Error::AlreadyFrozen(contest_id.to_owned()));
    }

    let target = Target::Contest(contest_id.to_owned());
    let (compiled_stmts, _doc_stmts, mut report) = check_inner(
        ws,
        engine,
        &target,
        Kind::Statements,
        None,
        None,
        true,
        false,
        None,
    )?;

    let Some(compiled_stmts) = compiled_stmts.filter(|_| report.ok) else {
        report.ok = false;
        return Ok(FreezeOutcome {
            frozen: false,
            report,
            outputs: Vec::new(),
        });
    };

    let statements_pdf = match compiled_stmts.pdf() {
        Ok(pdf) => pdf,
        Err(errs) => {
            report.ok = false;
            merge_diagnostics(
                &mut report.diagnostics,
                errs.into_iter().map(from_engine_plain).collect(),
            );
            return Ok(FreezeOutcome {
                frozen: false,
                report,
                outputs: Vec::new(),
            });
        }
    };

    let problems = ws.contest_problems(&contest)?;
    let has_any_editorial = problems.iter().any(|(_, p)| p.editorial.is_some());
    let editorial_pdf = if has_any_editorial {
        let ed_doc =
            document_with_engine(ws, Some(engine), &target, Kind::Editorial, None, None, None)?;
        engine.reset_files();
        let (compiled_ed, ed_report) = compile(engine, &ed_doc);
        report.compile_ms = report.compile_ms.saturating_add(ed_report.compile_ms);
        merge_diagnostics(&mut report.diagnostics, ed_report.diagnostics);
        let Some(compiled_ed) = compiled_ed.filter(|_| ed_report.ok) else {
            report.ok = false;
            return Ok(FreezeOutcome {
                frozen: false,
                report,
                outputs: Vec::new(),
            });
        };
        match compiled_ed.pdf() {
            Ok(pdf) => Some(pdf),
            Err(errs) => {
                report.ok = false;
                merge_diagnostics(
                    &mut report.diagnostics,
                    errs.into_iter().map(from_engine_plain).collect(),
                );
                return Ok(FreezeOutcome {
                    frozen: false,
                    report,
                    outputs: Vec::new(),
                });
            }
        }
    } else {
        None
    };

    // Build the template archive so frozen snapshots reproduce even if workspace
    // defaults, templates, plugin bundles, assets, or `templates/preamble.typ` later change.
    let mut plugin_archive_meta: Option<(String, String, String, Vec<String>)> = None;
    let mut plugin_bundle_files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let (eff_template, tpl_import_path, template_bytes, mut merged_opts) =
        if ws.has_contest_template(contest_id) {
            let rel = format!("contests/{contest_id}/template.typ");
            let bytes = ws.read_bytes(&rel)?;
            let opts = contest
                .meta
                .template_options
                .get("local")
                .cloned()
                .unwrap_or_default();
            ("local".to_owned(), format!("/{rel}"), Some(bytes), opts)
        } else {
            let eff = contest
                .meta
                .template
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .or_else(|| ws.default_template())
                .unwrap_or_else(|| "olymp-classic".into());
            let mut opts = ws
                .workspace_template_options()
                .remove(&eff)
                .unwrap_or_default();
            if let Some(c_opts) = contest.meta.template_options.get(&eff) {
                for (k, v) in c_opts {
                    opts.insert(k.clone(), v.clone());
                }
            }
            if let Some((pid, tid)) = parse_plugin_template_ref(&eff) {
                let Some((mount, tpl_entry)) = engine.find_plugin_template(pid, tid) else {
                    return Err(Error::NotFound(format!("plugin template {eff:?}")));
                };
                plugin_bundle_files = collect_plugin_bundle_files(mount)?;
                let rel_font_dirs = relative_font_dirs(mount);
                let import_path = format!("/plugins/{}/{}", mount.plugin_id, tpl_entry.entry);
                plugin_archive_meta = Some((
                    mount.plugin_id.clone(),
                    mount.version.clone(),
                    tpl_entry.entry.clone(),
                    rel_font_dirs,
                ));
                (eff, import_path, None, opts)
            } else {
                let rel = format!("templates/{eff}.typ");
                let bytes = ws.read_bytes(&rel)?;
                (eff, format!("/{rel}"), Some(bytes), opts)
            }
        };
    fold_legacy_contest_options(&contest.meta, &mut merged_opts);

    // Archive any `image`-typed book options into `snapshot/assets/` and rewrite paths.
    let mut snapshot_assets: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let schema = engine.inspect_template_schema(&tpl_import_path);
    for field in &schema.fields {
        if field.scope == TemplateFieldScope::Book
            && field.field_type == TemplateFieldType::Image
            && let Some(TemplateOptionValue::String(rel_path)) = merged_opts.get(&field.key)
        {
            let trimmed = rel_path.trim().trim_start_matches('/');
            if trimmed.is_empty() {
                continue;
            }
            if let Ok(img_bytes) = ws.read_bytes(trimmed) {
                let raw_name = Path::new(trimmed)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("asset.bin");
                let mut filename = raw_name.to_owned();
                let mut idx = 2u32;
                while snapshot_assets.contains_key(&filename) {
                    let p = Path::new(raw_name);
                    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("asset");
                    let ext = p
                        .extension()
                        .and_then(|e| e.to_str())
                        .map(|e| format!(".{e}"))
                        .unwrap_or_default();
                    filename = format!("{stem}-{idx}{ext}");
                    idx += 1;
                }
                snapshot_assets.insert(filename.clone(), img_bytes);
                merged_opts.insert(
                    field.key.clone(),
                    TemplateOptionValue::String(format!(
                        "contests/{contest_id}/snapshot/assets/{filename}"
                    )),
                );
            }
        }
    }

    let preamble_bytes = ws.load_preamble().map(String::into_bytes);
    let (plugin_id, plugin_version, entry, font_dirs) = match plugin_archive_meta {
        Some((pid, ver, ent, fdirs)) => (Some(pid), Some(ver), Some(ent), fdirs),
        None => (None, None, None, Vec::new()),
    };
    let archive = SnapshotTemplateArchive {
        meta: SnapshotTemplateMeta {
            template: eff_template.clone(),
            has_custom_template: true,
            has_preamble: preamble_bytes.is_some(),
            plugin_id,
            plugin_version,
            entry,
            font_dirs,
            options: merged_opts,
        },
        custom_template_bytes: template_bytes,
        preamble_bytes,
        assets: snapshot_assets,
        plugin_bundle_files,
    };

    // If the contest resolves a distinct editorial template, archive it separately so
    // frozen editorial PDFs reproduce offline identically to statement snapshots.
    let eff_editorial_explicit = contest
        .meta
        .editorial_template
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .or_else(|| ws.default_editorial_template());
    let editorial_archive = if editorial_pdf.is_some()
        && let Some(ed_tpl) = eff_editorial_explicit
        && ed_tpl != eff_template
    {
        let mut ed_plugin_meta: Option<(String, String, String, Vec<String>)> = None;
        let mut ed_bundle_files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        let mut ed_opts = ws
            .workspace_template_options()
            .remove(&ed_tpl)
            .unwrap_or_default();
        if let Some(c_opts) = contest.meta.template_options.get(&ed_tpl) {
            for (k, v) in c_opts {
                ed_opts.insert(k.clone(), v.clone());
            }
        }
        let (ed_import_path, ed_tpl_bytes) =
            if let Some((pid, tid)) = parse_plugin_template_ref(&ed_tpl) {
                let Some((mount, tpl_entry)) = engine.find_plugin_template(pid, tid) else {
                    return Err(Error::NotFound(format!("plugin template {ed_tpl:?}")));
                };
                ed_bundle_files = collect_plugin_bundle_files(mount)?;
                let rel_font_dirs = relative_font_dirs(mount);
                let import_path = format!("/plugins/{}/{}", mount.plugin_id, tpl_entry.entry);
                ed_plugin_meta = Some((
                    mount.plugin_id.clone(),
                    mount.version.clone(),
                    tpl_entry.entry.clone(),
                    rel_font_dirs,
                ));
                (import_path, None)
            } else {
                let rel = format!("templates/{ed_tpl}.typ");
                let bytes = ws.read_bytes(&rel)?;
                (format!("/{rel}"), Some(bytes))
            };
        fold_legacy_contest_options(&contest.meta, &mut ed_opts);

        let mut ed_assets: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        let ed_schema = engine.inspect_template_schema(&ed_import_path);
        for field in &ed_schema.fields {
            if field.scope == TemplateFieldScope::Book
                && field.field_type == TemplateFieldType::Image
                && let Some(TemplateOptionValue::String(rel_path)) = ed_opts.get(&field.key)
            {
                let trimmed = rel_path.trim().trim_start_matches('/');
                if trimmed.is_empty() {
                    continue;
                }
                if let Ok(img_bytes) = ws.read_bytes(trimmed) {
                    let raw_name = Path::new(trimmed)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("asset.bin");
                    let mut filename = format!("ed-{raw_name}");
                    let mut idx = 2u32;
                    while ed_assets.contains_key(&filename)
                        || archive.assets.contains_key(&filename)
                    {
                        let p = Path::new(raw_name);
                        let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("asset");
                        let ext = p
                            .extension()
                            .and_then(|e| e.to_str())
                            .map(|e| format!(".{e}"))
                            .unwrap_or_default();
                        filename = format!("ed-{stem}-{idx}{ext}");
                        idx += 1;
                    }
                    ed_assets.insert(filename.clone(), img_bytes);
                    ed_opts.insert(
                        field.key.clone(),
                        TemplateOptionValue::String(format!(
                            "contests/{contest_id}/snapshot/assets/{filename}"
                        )),
                    );
                }
            }
        }
        let (ed_pid, ed_pver, ed_entry, ed_fdirs) = match ed_plugin_meta {
            Some((pid, ver, ent, fdirs)) => (Some(pid), Some(ver), Some(ent), fdirs),
            None => (None, None, None, Vec::new()),
        };
        Some(SnapshotTemplateArchive {
            meta: SnapshotTemplateMeta {
                template: ed_tpl,
                has_custom_template: true,
                has_preamble: archive.meta.has_preamble,
                plugin_id: ed_pid,
                plugin_version: ed_pver,
                entry: ed_entry,
                font_dirs: ed_fdirs,
                options: ed_opts,
            },
            custom_template_bytes: ed_tpl_bytes,
            preamble_bytes: None,
            assets: ed_assets,
            plugin_bundle_files: ed_bundle_files,
        })
    } else {
        None
    };

    ws.freeze_commit_with_templates(
        contest_id,
        &statements_pdf,
        editorial_pdf.as_deref(),
        Some(&archive),
        editorial_archive.as_ref(),
    )?;

    let mut outputs = vec![format!("contests/{contest_id}/snapshot/{contest_id}.pdf")];
    if editorial_pdf.is_some() {
        outputs.push(format!(
            "contests/{contest_id}/snapshot/{contest_id}-editorial.pdf"
        ));
    }
    report.ok = !report.diagnostics.iter().any(Diagnostic::is_error);

    Ok(FreezeOutcome {
        frozen: true,
        report,
        outputs,
    })
}

fn from_lint(finding: xcpc_lint::Finding) -> Diagnostic {
    Diagnostic {
        severity: finding.severity.as_str(),
        code: Some(finding.code),
        message: finding.message,
        file: finding.file,
        line: finding.line,
        column: finding.column,
        page: None,
        hints: Vec::new(),
    }
}

fn from_engine_plain(d: xcpc_engine::Diagnostic) -> Diagnostic {
    Diagnostic {
        severity: if d.error { "error" } else { "warning" },
        code: None,
        message: d.message,
        file: d.file,
        line: d.line,
        column: d.column,
        page: d.page,
        hints: d.hints,
    }
}

fn is_typst_image_load_error(d: &Diagnostic) -> bool {
    d.is_error()
        && d.code.is_none()
        && (d.message.contains("file not found") || d.message.contains("failed to load file"))
}

fn extract_paren_path(message: &str) -> Option<&str> {
    let start = message.rfind('(')?;
    let end = message.rfind(')')?;
    (end > start + 1).then(|| &message[start + 1..end])
}

/// Merges `incoming` diagnostics into `existing`, avoiding duplicates across
/// converter diagnostics, Typst compile diagnostics, and `xcpc-lint` findings.
fn merge_diagnostics(existing: &mut Vec<Diagnostic>, incoming: Vec<Diagnostic>) {
    for diag in incoming {
        match diag.code.as_deref() {
            Some(xcpc_lint::code::MISSING_IMAGE) => {
                if existing.iter().any(|e| {
                    e.code.as_deref() == Some(xcpc_lint::code::MISSING_IMAGE)
                        && e.file == diag.file
                        && e.line == diag.line
                        && e.column == diag.column
                }) {
                    continue;
                }
                let mut replaced = false;
                let mut i = 0;
                while i < existing.len() {
                    let exact_loc = is_typst_image_load_error(&existing[i])
                        && existing[i].file == diag.file
                        && existing[i].line == diag.line
                        && existing[i].column == diag.column;
                    let same_line_path = !exact_loc
                        && is_typst_image_load_error(&existing[i])
                        && existing[i].file == diag.file
                        && existing[i].line == diag.line
                        && extract_paren_path(&diag.message)
                            .is_some_and(|p| existing[i].message.contains(p));
                    if exact_loc || same_line_path {
                        if !replaced {
                            existing[i] = diag.clone();
                            replaced = true;
                            i += 1;
                        } else {
                            existing.remove(i);
                        }
                    } else {
                        i += 1;
                    }
                }
                if !replaced {
                    existing.push(diag);
                }
            }
            Some(xcpc_lint::code::MISSING_EDITORIAL) => {
                if existing.iter().any(|e| {
                    e.code.as_deref() == Some(xcpc_lint::code::MISSING_EDITORIAL)
                        && e.file == diag.file
                }) {
                    continue;
                }
                existing.push(diag);
            }
            _ => {
                if !existing.iter().any(|e| {
                    e.severity == diag.severity
                        && e.code == diag.code
                        && e.file == diag.file
                        && e.line == diag.line
                        && e.column == diag.column
                        && e.page == diag.page
                        && e.message == diag.message
                }) {
                    existing.push(diag);
                }
            }
        }
    }
}

/// Each problem's pages: from its start to the next one's (or the end).
fn pages(doc: &Document, starts: &[(String, usize)], total: usize) -> Vec<Pages> {
    doc.problems
        .iter()
        .zip(starts)
        .enumerate()
        .map(|(i, ((label, id), (_, first)))| {
            let next = starts.get(i + 1).map_or(total + 1, |s| s.1);
            Pages {
                label: label.clone(),
                id: id.clone(),
                first: *first,
                count: next.saturating_sub(*first).max(1),
            }
        })
        .collect()
}

/// An engine for `ws` with the statement fonts.
pub fn engine(ws: &Workspace, fonts: &xcpc_engine::FontOptions) -> Engine {
    Engine::new(fonts, Some(ws.root().to_path_buf()))
}

/// Inspects a single workspace template (`templates/<name>.typ`) and returns its metadata
/// and declarative `#let schema`. Returns `None` if `templates/<name>.typ` does not exist.
pub fn inspect_template(ws: &Workspace, engine: &Engine, name: &str) -> Option<TemplateInfo> {
    if !ws.has_custom_template(name) {
        return None;
    }
    let rel_path = format!("templates/{name}.typ");
    let import_path = format!("/{rel_path}");
    let schema = engine.inspect_template_schema(&import_path);
    let display_name = schema
        .name
        .clone()
        .unwrap_or_else(|| xcpc_domain::LocalizedText::Plain(name.to_owned()));
    let description = schema.description.clone();
    Some(TemplateInfo {
        id: name.to_owned(),
        kind: schema.kind,
        path: rel_path,
        name: display_name,
        description,
        source: TemplateSource::Workspace,
        schema,
    })
}

/// Inspects a contest-local template (`contests/<contest_id>/template.typ`) and returns
/// its metadata and declarative `#let schema` under ID `"local"`.
pub fn inspect_contest_template(
    ws: &Workspace,
    engine: &Engine,
    contest_id: &str,
) -> Option<TemplateInfo> {
    if !ws.has_contest_template(contest_id) {
        return None;
    }
    let rel_path = format!("contests/{contest_id}/template.typ");
    let import_path = format!("/{rel_path}");
    let schema = engine.inspect_template_schema(&import_path);
    let display_name = schema.name.clone().unwrap_or_else(|| {
        xcpc_domain::LocalizedText::bilingual("本场专属模板", "Contest Template")
    });
    let description = schema.description.clone();
    Some(TemplateInfo {
        id: "local".to_owned(),
        kind: schema.kind,
        path: rel_path,
        name: display_name,
        description,
        source: TemplateSource::Local,
        schema,
    })
}

/// Inspects a TRD-local template (`trds/<trd_id>/template.typ`) and returns
/// its metadata and declarative `#let schema` under ID `"local"`.
pub fn inspect_trd_template(ws: &Workspace, engine: &Engine, trd_id: &str) -> Option<TemplateInfo> {
    if !ws.has_trd_template(trd_id) {
        return None;
    }
    let rel_path = format!("trds/{trd_id}/template.typ");
    let import_path = format!("/{rel_path}");
    let schema = engine.inspect_template_schema(&import_path);
    let display_name = schema
        .name
        .clone()
        .unwrap_or_else(|| xcpc_domain::LocalizedText::bilingual("本册专属模板", "TRD Template"));
    let description = schema.description.clone();
    Some(TemplateInfo {
        id: "local".to_owned(),
        kind: TemplateKind::Trd,
        path: rel_path,
        name: display_name,
        description,
        source: TemplateSource::Local,
        schema,
    })
}

/// Inspects a mounted plugin template (`plugin:<plugin_id>/<template_id>`) and returns
/// its metadata and declarative `#let schema`.
pub fn inspect_plugin_template(
    engine: &Engine,
    plugin_id: &str,
    template_id: &str,
) -> Option<TemplateInfo> {
    let (mount, tpl) = engine.find_plugin_template(plugin_id, template_id)?;
    let import_path = format!("/plugins/{}/{}", mount.plugin_id, tpl.entry);
    let schema = engine.inspect_template_schema(&import_path);
    let kind = tpl.kind.unwrap_or(schema.kind);
    let display_name = tpl
        .name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| LocalizedText::Plain(s.to_owned()))
        .or_else(|| schema.name.clone())
        .unwrap_or_else(|| LocalizedText::Plain(tpl.id.clone()));
    let description = tpl
        .description
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| LocalizedText::Plain(s.to_owned()))
        .or_else(|| schema.description.clone());
    Some(TemplateInfo {
        id: format_plugin_template_ref(&mount.plugin_id, &tpl.id),
        kind,
        path: import_path,
        name: display_name,
        description,
        source: TemplateSource::Plugin {
            plugin_id: mount.plugin_id.clone(),
            plugin_name: mount.plugin_name.clone(),
            template_id: tpl.id.clone(),
        },
        schema,
    })
}

/// Lists all workspace templates (`templates/*.typ`) and mounted plugin templates
/// (`plugin:<plugin_id>/<template_id>`) with their evaluated declarative schemas.
pub fn list_templates(ws: &Workspace, engine: &Engine) -> Vec<TemplateInfo> {
    let mut out: Vec<TemplateInfo> = ws
        .list_templates()
        .into_iter()
        .filter_map(|id| inspect_template(ws, engine, &id))
        .collect();
    for mount in engine.plugin_mounts() {
        for tpl in &mount.templates {
            if let Some(info) = inspect_plugin_template(engine, &mount.plugin_id, &tpl.id) {
                out.push(info);
            }
        }
    }
    out
}

/// Lists workspace and plugin templates filtered by [`TemplateKind`] (`Statement`,
/// `Editorial`, or `Trd`).
pub fn list_templates_by_kind(
    ws: &Workspace,
    engine: &Engine,
    kind: TemplateKind,
) -> Vec<TemplateInfo> {
    list_templates(ws, engine)
        .into_iter()
        .filter(|t| t.kind == kind)
        .collect()
}

/// Lists workspace and plugin templates suitable for statements or editorials
/// (`TemplateKind::Statement` or `TemplateKind::Editorial`, excluding `Trd`).
pub fn list_statement_and_editorial_templates(
    ws: &Workspace,
    engine: &Engine,
) -> Vec<TemplateInfo> {
    list_templates(ws, engine)
        .into_iter()
        .filter(|t| matches!(t.kind, TemplateKind::Statement | TemplateKind::Editorial))
        .collect()
}

/// Forks an existing workspace template `templates/<source_name>.typ` into a new
/// workspace template `templates/<new_name>.typ`, returning the relative path.
pub fn fork_template(ws: &Workspace, source_name: &str, new_name: &str) -> Result<String, Error> {
    check_id(source_name)?;
    check_id(new_name)?;
    if !ws.has_custom_template(source_name) {
        return Err(Error::NotFound(format!("template {source_name:?}")));
    }
    let bytes = ws.read_bytes(&format!("templates/{source_name}.typ"))?;
    ws.fork_template(new_name, &bytes)
}

/// Copies a mounted plugin template (`plugin:<plugin_id>/<template_id>`) into the workspace
/// as a self-contained workspace template (`templates/<target_name>.typ`, plus
/// `templates/<target_name>/...` when the plugin includes companion modules or assets).
/// Returns `(target_name, rel_path)`.
pub fn eject_plugin_template(
    ws: &Workspace,
    engine: &Engine,
    plugin_id: &str,
    template_id: &str,
    target_name: Option<&str>,
) -> Result<(String, String), Error> {
    let Some((mount, tpl_entry)) = engine.find_plugin_template(plugin_id, template_id) else {
        return Err(Error::NotFound(format!(
            "plugin template {:?}",
            format_plugin_template_ref(plugin_id, template_id)
        )));
    };
    let target = target_name
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| {
            if !ws.has_custom_template(template_id) {
                template_id.to_owned()
            } else {
                format!("{plugin_id}-{template_id}")
            }
        });
    check_id(&target)?;

    let bundle = collect_plugin_bundle_files(mount)?;
    let entry_norm = tpl_entry.entry.replace('\\', "/");
    let Some(entry_bytes) = bundle.get(&entry_norm).cloned() else {
        return Err(Error::NotFound(format!(
            "plugin template entry {:?}",
            tpl_entry.entry
        )));
    };

    let rel = if bundle.len() <= 1 {
        ws.eject_template_bundle(&target, &entry_bytes, &BTreeMap::new())?
    } else {
        let escaped = format!("{target}/{entry_norm}")
            .replace('\\', "\\\\")
            .replace('"', "\\\"");
        let wrapper = format!(
            "// Ejected from plugin:{plugin_id}/{template_id} (v{})\n\
             #import \"{escaped}\": *\n",
            mount.version
        );
        ws.eject_template_bundle(&target, wrapper.as_bytes(), &bundle)?
    };
    Ok((target, rel))
}

fn collect_plugin_bundle_files(mount: &PluginMount) -> Result<BTreeMap<String, Vec<u8>>, Error> {
    let canon_root = mount.root.canonicalize().map_err(|e| Error::Io {
        path: mount.root.display().to_string(),
        source: e,
    })?;
    let mut files = BTreeMap::new();
    collect_plugin_bundle_rec(&canon_root, &canon_root, &mut files)?;
    Ok(files)
}

fn collect_plugin_bundle_rec(
    dir: &Path,
    root: &Path,
    out: &mut BTreeMap<String, Vec<u8>>,
) -> Result<(), Error> {
    let entries = std::fs::read_dir(dir).map_err(|e| Error::Io {
        path: dir.display().to_string(),
        source: e,
    })?;
    for entry in entries {
        let entry = entry.map_err(|e| Error::Io {
            path: dir.display().to_string(),
            source: e,
        })?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with('.')
            || matches!(
                name_str.as_ref(),
                "node_modules" | "dist" | "target" | "Thumbs.db"
            )
        {
            continue;
        }
        let path = entry.path();
        let meta = std::fs::symlink_metadata(&path).map_err(|e| Error::Io {
            path: path.display().to_string(),
            source: e,
        })?;
        if meta.file_type().is_symlink() {
            return Err(Error::PathEscape(path.display().to_string()));
        }
        if meta.is_dir() {
            collect_plugin_bundle_rec(&path, root, out)?;
        } else if meta.is_file() {
            let rel = path
                .strip_prefix(root)
                .map_err(|_| Error::PathEscape(path.display().to_string()))?
                .to_string_lossy()
                .replace('\\', "/");
            if matches!(
                rel.as_str(),
                "manifest.toml" | "index.js" | "ui.html" | "ui.js"
            ) || rel.to_ascii_lowercase().ends_with(".zip")
            {
                continue;
            }
            let bytes = std::fs::read(&path).map_err(|e| Error::Io {
                path: path.display().to_string(),
                source: e,
            })?;
            out.insert(rel, bytes);
        }
    }
    Ok(())
}

fn relative_font_dirs(mount: &PluginMount) -> Vec<String> {
    let Ok(canon_root) = mount.root.canonicalize() else {
        return Vec::new();
    };
    mount
        .font_dirs
        .iter()
        .filter_map(|d| d.canonicalize().ok())
        .filter_map(|d| d.strip_prefix(&canon_root).ok().map(Path::to_path_buf))
        .map(|rel| rel.to_string_lossy().replace('\\', "/"))
        .filter(|s| !s.is_empty())
        .collect()
}

/// Maximum UTF-8 byte length for a sanitized PDF filename stem (leaving ample room for
/// suffixes like `-试题册.pdf` within the 255-byte filesystem limit).
const MAX_FILENAME_STEM_BYTES: usize = 160;
/// Maximum character count for a sanitized PDF filename stem.
const MAX_FILENAME_STEM_CHARS: usize = 100;

/// Returns `true` if `name` (ignoring any extension after the first `.`) is a Windows
/// reserved device name (`CON`, `PRN`, `AUX`, `NUL`, `COM1`–`COM9`, `LPT1`–`LPT9`).
fn is_windows_reserved_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).trim();
    if stem.is_empty() {
        return false;
    }
    let upper = stem.to_ascii_uppercase();
    matches!(
        upper.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

/// Sanitizes a human title for use as a cross-platform PDF file stem: replaces path
/// separators, Windows-forbidden characters, and control characters with `-`, collapses
/// repeated whitespace, trims leading/trailing dots and whitespace, rejects Windows
/// reserved device names, and enforces a safe cross-platform length limit.
pub fn sanitize_filename(name: &str) -> String {
    let mut out = String::with_capacity(name.len().min(MAX_FILENAME_STEM_BYTES));
    let mut last_was_space = false;
    let mut char_count = 0usize;
    for c in name.trim().chars() {
        let illegal = matches!(
            c,
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0'
        ) || c.is_control();
        let next_ch = if illegal {
            last_was_space = false;
            '-'
        } else if c.is_whitespace() {
            if last_was_space {
                continue;
            }
            last_was_space = true;
            ' '
        } else {
            last_was_space = false;
            c
        };
        if char_count >= MAX_FILENAME_STEM_CHARS
            || out.len() + next_ch.len_utf8() > MAX_FILENAME_STEM_BYTES
        {
            break;
        }
        out.push(next_ch);
        char_count += 1;
    }
    let trimmed = out.trim().trim_matches('.').trim();
    if trimmed.is_empty() || trimmed.chars().all(|c| c == '-') || is_windows_reserved_name(trimmed)
    {
        String::new()
    } else {
        trimmed.to_owned()
    }
}

/// The default relative PDF path for `target` and `kind` given an optional human title
/// (`<比赛名>-试题册.pdf`, `<比赛名>-题解.pdf`, `<题名>.pdf`, `<题名>-题解.pdf`), falling
/// back to `target.id()` when the sanitized title is empty.
pub fn default_output_for_title(target: &Target, kind: Kind, title: Option<&str>) -> String {
    let base = title
        .map(sanitize_filename)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| target.id().to_owned());
    match (target, kind) {
        (Target::Contest(_), Kind::Statements) => format!("build/{base}-试题册.pdf"),
        (Target::Contest(_), Kind::Editorial) => format!("build/{base}-题解.pdf"),
        (Target::Problem(_), Kind::Statements) => format!("build/{base}.pdf"),
        (Target::Problem(_), Kind::Editorial) => format!("build/{base}-题解.pdf"),
    }
}

/// The default name of an exported PDF, relative to the workspace:
/// `build/<比赛名>-试题册.pdf`, `build/<比赛名>-题解.pdf`, `build/<题名>.pdf`, or
/// `build/<题名>-题解.pdf` (falling back to `<id>` when the title is empty or unavailable).
pub fn default_output(ws: &Workspace, target: &Target, kind: Kind) -> String {
    let title = match target {
        Target::Contest(id) => ws.load_contest(id).ok().map(|c| c.meta.name),
        Target::Problem(id) => ws.load_problem_meta(id).ok().map(|m| m.title),
    };
    default_output_for_title(target, kind, title.as_deref())
}

// ---- Phase 5: Living TRD & Single-Snippet Publishing -----------------------------------

fn is_builtin_trd_preset(name: &str) -> bool {
    matches!(name, "trd-compact" | "trd-book")
}

/// Infers whether a TRD or snippet should use Chinese or English UI labels in templates.
fn infer_trd_language(text: &str) -> Language {
    if text.chars().any(crate::is_cjk) {
        Language::Chinese
    } else {
        Language::English
    }
}

/// Default workspace-relative PDF output path for a TRD (`build/<sanitized_name>.pdf`).
pub fn default_trd_output(ws: &Workspace, trd_id: &str) -> String {
    let base = ws
        .load_trd(trd_id)
        .ok()
        .map(|t| sanitize_filename(&t.meta.name))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| trd_id.to_owned());
    format!("build/{base}.pdf")
}

/// Builds a Typst [`Document`] for `trds/<trd_id>/`.
///
/// Template resolution priority:
/// 1. When `trd.meta.frozen == true`: `trds/<id>/snapshot/template.typ` (and snapshot snippets).
/// 2. Else if `trds/<id>/template.typ` exists: `/trds/<id>/template.typ`.
/// 3. Else if `templates/<name>.typ` exists: `/templates/<name>.typ`.
/// 4. Else if `<name>` is a built-in TRD preset (`trd-compact` / `trd-book`): `/xcpc/<name>.typ`.
/// 5. Otherwise emits a `MISSING_TEMPLATE` diagnostic.
pub fn document_trd(ws: &Workspace, trd_id: &str) -> Result<Document, Error> {
    document_trd_with_engine(ws, None, trd_id, None)
}

/// [`document_trd`] extended with an optional [`Engine`] (for mounted `plugin:` templates)
/// and optional `--template` override.
pub fn document_trd_with_engine(
    ws: &Workspace,
    engine: Option<&Engine>,
    trd_id: &str,
    template_override: Option<&str>,
) -> Result<Document, Error> {
    let trd = ws.load_trd(trd_id)?;
    let mut missing_diags: Vec<crate::Diagnostic> = Vec::new();
    let chapters_owned: Vec<(String, Vec<xcpc_domain::SnippetDetail>)> = if trd.meta.frozen {
        ws.trd_snippets(&trd)?
    } else {
        let mut out = Vec::with_capacity(trd.meta.chapters.len());
        for ch in &trd.meta.chapters {
            let mut items = Vec::with_capacity(ch.items.len());
            for slug in &ch.items {
                match ws.load_snippet(slug) {
                    Ok(detail) => items.push(detail),
                    Err(xcpc_storage::Error::NotFound(_)) => {
                        missing_diags.push(crate::Diagnostic {
                            error: false,
                            code: Some("missing_snippet".into()),
                            message: format!("未找到模板「{slug}」"),
                            file: String::new(),
                            line: 0,
                            column: 0,
                        });
                    }
                    Err(e) => return Err(e),
                }
            }
            out.push((ch.title.clone(), items));
        }
        out
    };
    let lang = infer_trd_language(&trd.meta.name);
    let mut book = TrdBook::from_meta(&trd.meta, lang);
    let template_override = template_override.map(str::trim).filter(|s| !s.is_empty());
    if let Some(ov) = template_override {
        book.template_name = ov.to_owned();
    }

    let ws_template_opts = ws.workspace_template_options();
    let merge_trd_opts = |tpl_name: &str| {
        let mut merged: BTreeMap<String, TemplateOptionValue> =
            ws_template_opts.get(tpl_name).cloned().unwrap_or_default();
        if let Some(trd_opts) = trd.meta.template_options.get(tpl_name) {
            for (k, v) in trd_opts {
                merged.insert(k.clone(), v.clone());
            }
        }
        merged
    };

    let mut missing_tpl_diag: Option<crate::Diagnostic> = None;
    let mut active_plugin_id: Option<String> = None;
    let mut extra_font_key: Option<String> = None;
    let mut extra_font_dirs: Vec<PathBuf> = Vec::new();
    if trd.meta.frozen && template_override.is_none() {
        let snap = ws.load_trd_snapshot_template(trd_id);
        let snap_bundle_entry =
            snap.as_ref()
                .and_then(|s| s.entry.as_deref())
                .and_then(|entry_rel| {
                    let rel = format!("trds/{trd_id}/snapshot/template/{entry_rel}");
                    ws.resolve(&rel).ok().filter(|p| p.is_file()).map(|_| rel)
                });
        let snap_tpl_rel = format!("trds/{trd_id}/snapshot/template.typ");
        let has_snap_tpl = ws.resolve(&snap_tpl_rel).is_ok_and(|p| p.is_file());
        if let Some(bundle_rel) = snap_bundle_entry {
            book.template_name = snap
                .as_ref()
                .map(|s| s.template.clone())
                .unwrap_or_else(|| "local".into());
            book.template_options = snap.as_ref().map(|s| s.options.clone()).unwrap_or_default();
            book.preamble = ws.load_trd_snapshot_preamble(trd_id);
            book.template_import = Some(format!("/{bundle_rel}"));
            if let Some(s) = &snap
                && !s.font_dirs.is_empty()
            {
                extra_font_key = Some(format!("trd-snap:{trd_id}"));
                for dir in &s.font_dirs {
                    let rel = format!("trds/{trd_id}/snapshot/template/{dir}");
                    if let Ok(abs) = ws.resolve(&rel)
                        && abs.is_dir()
                    {
                        extra_font_dirs.push(abs);
                    }
                }
            }
        } else if has_snap_tpl {
            book.template_name = snap
                .as_ref()
                .map(|s| s.template.clone())
                .unwrap_or_else(|| "local".into());
            book.template_options = snap.as_ref().map(|s| s.options.clone()).unwrap_or_default();
            book.preamble = ws.load_trd_snapshot_preamble(trd_id);
            book.template_import = Some(format!("/{snap_tpl_rel}"));
        } else {
            let name = book.template_name.clone();
            if is_builtin_trd_preset(&name) {
                book.template_import = Some(format!("/xcpc/{name}.typ"));
                book.template_options = merge_trd_opts(&name);
            } else {
                book.template_import = None;
                missing_tpl_diag = Some(crate::Diagnostic {
                    error: true,
                    code: Some(MISSING_TEMPLATE.into()),
                    message: format!(
                        "定稿模板册「{trd_id}」缺少快照模板文件 {snap_tpl_rel}，无法复现排版"
                    ),
                    file: format!("trds/{trd_id}/trd.toml"),
                    line: 1,
                    column: 1,
                });
            }
        }
    } else if template_override.is_none() && ws.has_trd_template(trd_id) {
        book.template_name = "local".into();
        book.template_import = Some(format!("/trds/{trd_id}/template.typ"));
        book.template_options = trd
            .meta
            .template_options
            .get("local")
            .cloned()
            .unwrap_or_default();
        book.preamble = ws.load_preamble();
    } else {
        let name = book.template_name.clone();
        book.preamble = ws.load_preamble();
        if let Some((pid, tid)) = parse_plugin_template_ref(&name) {
            if let Some((mount, tpl_entry)) = engine.and_then(|e| e.find_plugin_template(pid, tid))
            {
                book.template_import =
                    Some(format!("/plugins/{}/{}", mount.plugin_id, tpl_entry.entry));
                book.template_options = merge_trd_opts(&name);
                active_plugin_id = Some(mount.plugin_id.clone());
            } else {
                book.template_import = None;
                missing_tpl_diag = Some(crate::Diagnostic {
                    error: true,
                    code: Some(PLUGIN_TEMPLATE_NOT_FOUND.into()),
                    message: format!(
                        "未找到插件模板「{name}」（插件 {pid} 未安装、未启用或不包含模板 {tid}）"
                    ),
                    file: format!("trds/{trd_id}/trd.toml"),
                    line: 1,
                    column: 1,
                });
            }
        } else if check_id(&name).is_ok() && ws.has_custom_template(&name) {
            book.template_import = Some(format!("/templates/{name}.typ"));
            book.template_options = merge_trd_opts(&name);
        } else if is_builtin_trd_preset(&name) {
            book.template_import = Some(format!("/xcpc/{name}.typ"));
            book.template_options = merge_trd_opts(&name);
        } else {
            book.template_import = None;
            missing_tpl_diag = Some(crate::Diagnostic {
                error: true,
                code: Some(MISSING_TEMPLATE.into()),
                message: format!(
                    "未找到模板「{name}」（templates/{name}.typ 不存在）；请从预设安装或在 templates/ 中提供该模板"
                ),
                file: format!("trds/{trd_id}/trd.toml"),
                line: 1,
                column: 1,
            });
        }
    }

    let chapter_inputs: Vec<TrdChapterInput<'_>> = chapters_owned
        .iter()
        .map(|(title, items)| TrdChapterInput {
            title: title.clone(),
            items: items.iter().collect(),
        })
        .collect();

    let mut doc = crate::render_trd(&book, &chapter_inputs);
    doc.active_plugin_id = active_plugin_id;
    doc.extra_font_key = extra_font_key;
    doc.extra_font_dirs = extra_font_dirs;
    if let Some(d) = missing_tpl_diag {
        doc.diagnostics.push(d);
    }
    doc.diagnostics.extend(missing_diags);
    Ok(doc)
}

/// Builds a Typst [`Document`] for a single snippet's live preview (notes + code + cumulative hash gutter).
///
/// `unsaved` optionally overrides one workspace-relative file (`snippets/<slug>/code.hpp` or
/// `snippets/<slug>/notes.md`) in memory without touching disk.
pub fn document_snippet_preview(
    ws: &Workspace,
    slug: &str,
    template: Option<&str>,
    unsaved: Option<(&str, &str)>,
) -> Result<Document, Error> {
    document_snippet_preview_with_engine(ws, None, slug, template, unsaved)
}

/// [`document_snippet_preview`] with an optional [`Engine`] for mounted `plugin:` templates.
pub fn document_snippet_preview_with_engine(
    ws: &Workspace,
    engine: Option<&Engine>,
    slug: &str,
    template: Option<&str>,
    unsaved: Option<(&str, &str)>,
) -> Result<Document, Error> {
    let mut snippet = ws.load_snippet(slug)?;
    if let Some((path, text)) = unsaved {
        let code_rel = format!("snippets/{slug}/code.hpp");
        let notes_rel = format!("snippets/{slug}/notes.md");
        if path == code_rel {
            snippet.code = if text.trim().is_empty() {
                None
            } else {
                Some(TextFile {
                    path: code_rel,
                    text: text.to_owned(),
                })
            };
        } else if path == notes_rel {
            snippet.notes = if text.trim().is_empty() {
                None
            } else {
                Some(TextFile {
                    path: notes_rel,
                    text: text.to_owned(),
                })
            };
        }
    }

    let lang = infer_trd_language(&snippet.meta.title);
    let mut book = TrdBook::single(&snippet.meta.title, lang);
    book.preamble = ws.load_preamble();

    // Populate `slug -> title` for dependency resolution in single-snippet preview (review T2).
    if let Ok(summaries) = ws.list_snippet_summaries() {
        for s in summaries {
            book.snippet_titles.insert(s.slug, s.title);
        }
    }

    // If `template` is `"trd:<trd_id>"`, inherit that TRD's template AND merged template options (review T2).
    let ws_template_opts = ws.workspace_template_options();
    let trd_ref = template
        .map(str::trim)
        .and_then(|s| s.strip_prefix("trd:"))
        .filter(|id| !id.is_empty())
        .and_then(|id| ws.load_trd(id).ok());

    let mut missing_tpl_diag = None;
    let mut active_plugin_id: Option<String> = None;
    if let Some(trd) = trd_ref {
        if !trd.meta.frozen && ws.has_trd_template(&trd.id) {
            book.template_name = "local".into();
            book.template_import = Some(format!("/trds/{}/template.typ", trd.id));
            book.template_options = trd
                .meta
                .template_options
                .get("local")
                .cloned()
                .unwrap_or_default();
        } else {
            let tpl_name = trd
                .meta
                .template
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("trd-compact");
            book.template_name = tpl_name.to_owned();
            let mut merged = ws_template_opts.get(tpl_name).cloned().unwrap_or_default();
            if let Some(trd_opts) = trd.meta.template_options.get(tpl_name) {
                for (k, v) in trd_opts {
                    merged.insert(k.clone(), v.clone());
                }
            }
            book.template_options = merged;
            if let Some((pid, tid)) = parse_plugin_template_ref(tpl_name) {
                if let Some((mount, tpl_entry)) =
                    engine.and_then(|e| e.find_plugin_template(pid, tid))
                {
                    book.template_import =
                        Some(format!("/plugins/{}/{}", mount.plugin_id, tpl_entry.entry));
                    active_plugin_id = Some(mount.plugin_id.clone());
                } else {
                    book.template_import = None;
                    missing_tpl_diag = Some(crate::Diagnostic {
                        error: true,
                        code: Some(PLUGIN_TEMPLATE_NOT_FOUND.into()),
                        message: format!(
                            "未找到插件模板「{tpl_name}」（插件 {pid} 未安装、未启用或不包含模板 {tid}）"
                        ),
                        file: format!("trds/{}/trd.toml", trd.id),
                        line: 1,
                        column: 1,
                    });
                }
            } else if check_id(tpl_name).is_ok() && ws.has_custom_template(tpl_name) {
                book.template_import = Some(format!("/templates/{tpl_name}.typ"));
            } else if is_builtin_trd_preset(tpl_name) {
                book.template_import = Some(format!("/xcpc/{tpl_name}.typ"));
            }
        }
    } else {
        let tpl_name = template
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("trd-compact");
        book.template_name = tpl_name.to_owned();
        book.template_options = ws_template_opts.get(tpl_name).cloned().unwrap_or_default();
        if let Some((pid, tid)) = parse_plugin_template_ref(tpl_name) {
            if let Some((mount, tpl_entry)) = engine.and_then(|e| e.find_plugin_template(pid, tid))
            {
                book.template_import =
                    Some(format!("/plugins/{}/{}", mount.plugin_id, tpl_entry.entry));
                active_plugin_id = Some(mount.plugin_id.clone());
            } else {
                book.template_import = None;
                missing_tpl_diag = Some(crate::Diagnostic {
                    error: true,
                    code: Some(PLUGIN_TEMPLATE_NOT_FOUND.into()),
                    message: format!(
                        "未找到插件模板「{tpl_name}」（插件 {pid} 未安装、未启用或不包含模板 {tid}）"
                    ),
                    file: "workspace.toml".into(),
                    line: 1,
                    column: 1,
                });
            }
        } else if check_id(tpl_name).is_ok() && ws.has_custom_template(tpl_name) {
            book.template_import = Some(format!("/templates/{tpl_name}.typ"));
        } else if is_builtin_trd_preset(tpl_name) {
            book.template_import = Some(format!("/xcpc/{tpl_name}.typ"));
        } else {
            book.template_import = None;
            missing_tpl_diag = Some(crate::Diagnostic {
                error: true,
                code: Some(MISSING_TEMPLATE.into()),
                message: format!(
                    "未找到模板「{tpl_name}」（templates/{tpl_name}.typ 不存在）；请从预设安装或在 templates/ 中提供该模板"
                ),
                file: "workspace.toml".into(),
                line: 1,
                column: 1,
            });
        }
    }

    let chapters = [TrdChapterInput {
        title: String::new(),
        items: vec![&snippet],
    }];
    let mut doc = crate::render_trd(&book, &chapters);
    doc.active_plugin_id = active_plugin_id;
    if let Some(d) = missing_tpl_diag {
        doc.diagnostics.push(d);
    }
    Ok(doc)
}

/// Compiles `trds/<trd_id>/` into a [`Compiled`] Typst document and [`Report`].
pub fn compile_trd(
    ws: &Workspace,
    engine: &mut Engine,
    trd_id: &str,
) -> Result<(Option<Compiled>, Document, Report), Error> {
    let doc = document_trd_with_engine(ws, Some(engine), trd_id, None)?;
    engine.reset_files();
    let (compiled, report) = compile(engine, &doc);
    Ok((compiled, doc, report))
}

/// Runs full TRD preflight (missing snippets, missing transitive deps, cycles, verification
/// status warnings) plus Typst compilation into one combined [`Report`].
pub fn check_trd(
    ws: &Workspace,
    engine: &mut Engine,
    trd_id: &str,
    verify_statuses: &BTreeMap<String, SnippetVerifyStatus>,
) -> Result<(Option<Compiled>, Option<Document>, Report), Error> {
    let trd = ws.load_trd(trd_id)?;
    let preflight = if trd.meta.frozen {
        Vec::new()
    } else {
        ws.preflight_trd(trd_id, verify_statuses)?
    };
    let preflight_diags: Vec<Diagnostic> = preflight
        .iter()
        .map(|i| Diagnostic {
            severity: match i.severity {
                TrdPreflightSeverity::Error => "error",
                TrdPreflightSeverity::Warning => "warning",
            },
            code: Some(
                serde_json::to_value(i.kind)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_else(|| "trd_preflight".into()),
            ),
            message: i.message.clone(),
            file: None,
            line: None,
            column: None,
            page: None,
            hints: Vec::new(),
        })
        .collect();

    let (compiled, doc, mut report) = compile_trd(ws, engine, trd_id)?;
    // Avoid duplicating missing_snippet diagnostics already emitted by preflight.
    report
        .diagnostics
        .retain(|d| d.code.as_deref() != Some("missing_snippet"));
    merge_diagnostics(&mut report.diagnostics, preflight_diags);
    report.ok = compiled.is_some() && !report.diagnostics.iter().any(Diagnostic::is_error);
    Ok((compiled, Some(doc), report))
}

/// Compiles `trds/<trd_id>/` into PDF bytes in memory.
pub fn compile_trd_pdf_bytes(
    ws: &Workspace,
    engine: &mut Engine,
    trd_id: &str,
) -> Result<(Option<Vec<u8>>, Document, Report), Error> {
    let (compiled, doc, mut report) = compile_trd(ws, engine, trd_id)?;
    let Some(compiled) = compiled.filter(|_| report.ok) else {
        report.ok = false;
        return Ok((None, doc, report));
    };
    match compiled.pdf() {
        Ok(bytes) => Ok((Some(bytes), doc, report)),
        Err(errs) => {
            report.ok = false;
            merge_diagnostics(
                &mut report.diagnostics,
                errs.into_iter().map(from_engine_plain).collect(),
            );
            Ok((None, doc, report))
        }
    }
}

/// Compiles `trds/<trd_id>/` and atomically writes the PDF to `output_rel` (or
/// [`default_trd_output`] when `None`). Enforces read-only mode and workspace path sandbox.
pub fn export_trd_pdf(
    ws: &Workspace,
    engine: &mut Engine,
    trd_id: &str,
    output_rel: Option<&Path>,
) -> Result<(Option<String>, Report), Error> {
    if ws.is_read_only() {
        return Err(Error::ReadOnly);
    }
    let rel_path = match output_rel {
        Some(p) => p.to_string_lossy().into_owned(),
        None => default_trd_output(ws, trd_id),
    };
    ws.check_writable(&rel_path)?;
    let (pdf_bytes, _doc, report) = compile_trd_pdf_bytes(ws, engine, trd_id)?;
    let Some(bytes) = pdf_bytes else {
        return Ok((None, report));
    };
    ws.write(&rel_path, &bytes)?;
    Ok((Some(rel_path), report))
}

/// Renders one 1-based page of `trds/<trd_id>/` as PNG bytes.
pub fn render_trd_page_png(
    ws: &Workspace,
    engine: &mut Engine,
    trd_id: &str,
    page_1based: usize,
    pixel_per_pt: f32,
) -> Result<(Option<Vec<u8>>, Report), Error> {
    let (compiled, _doc, report) = compile_trd(ws, engine, trd_id)?;
    let Some(compiled) = compiled else {
        return Ok((None, report));
    };
    let idx = page_1based.saturating_sub(1);
    Ok((compiled.render_png(idx, pixel_per_pt), report))
}

/// Compiles a single snippet preview (`notes.md` + `code.hpp` + cumulative hash gutter).
pub fn compile_snippet_preview(
    ws: &Workspace,
    engine: &mut Engine,
    slug: &str,
    template: Option<&str>,
    unsaved: Option<(&str, &str)>,
) -> Result<(Option<Compiled>, Document, Report), Error> {
    let doc = document_snippet_preview_with_engine(ws, Some(engine), slug, template, unsaved)?;
    if unsaved.is_none() {
        engine.reset_files();
    }
    let (compiled, report) = compile(engine, &doc);
    Ok((compiled, doc, report))
}

/// Renders one 1-based page of a single snippet preview as PNG bytes.
pub fn render_snippet_page_png(
    ws: &Workspace,
    engine: &mut Engine,
    slug: &str,
    template: Option<&str>,
    page_1based: usize,
    pixel_per_pt: f32,
    unsaved: Option<(&str, &str)>,
) -> Result<(Option<Vec<u8>>, Report), Error> {
    let (compiled, _doc, report) = compile_snippet_preview(ws, engine, slug, template, unsaved)?;
    let Some(compiled) = compiled else {
        return Ok((None, report));
    };
    let idx = page_1based.saturating_sub(1);
    Ok((compiled.render_png(idx, pixel_per_pt), report))
}

/// Validates, compiles PDF, builds the effective template archive, and atomically freezes
/// `trds/<trd_id>/` into `snapshot/`.
pub fn freeze_trd_with_pdf(
    ws: &Workspace,
    engine: &mut Engine,
    trd_id: &str,
    strict: bool,
    verify_statuses: &BTreeMap<String, SnippetVerifyStatus>,
) -> Result<(Option<TrdFreezeResult>, Report), Error> {
    check_id(trd_id)?;
    if ws.is_read_only() {
        return Err(Error::ReadOnly);
    }
    let trd = ws.load_trd(trd_id)?;
    if trd.meta.archived {
        return Err(Error::TrdArchived(trd_id.to_owned()));
    }
    if trd.meta.frozen {
        return Err(Error::TrdAlreadyFrozen(trd_id.to_owned()));
    }

    // Run preflight upfront so strict/hard errors return cleanly before compiling Typst.
    let preflight = ws.preflight_trd(trd_id, verify_statuses)?;
    let hard_errors: Vec<String> = preflight
        .iter()
        .filter(|i| i.severity == TrdPreflightSeverity::Error)
        .map(|i| i.message.clone())
        .collect();
    if !hard_errors.is_empty() {
        return Err(Error::TrdFreezeBlocked {
            id: trd_id.to_owned(),
            issues: hard_errors,
        });
    }
    let warning_msgs: Vec<String> = preflight
        .iter()
        .filter(|i| i.severity == TrdPreflightSeverity::Warning)
        .map(|i| i.message.clone())
        .collect();
    if strict && !warning_msgs.is_empty() {
        return Err(Error::TrdFreezeBlocked {
            id: trd_id.to_owned(),
            issues: warning_msgs,
        });
    }

    let (pdf_bytes, _doc, report) = compile_trd_pdf_bytes(ws, engine, trd_id)?;
    let Some(pdf_bytes) = pdf_bytes else {
        return Ok((None, report));
    };

    // Build template archive so frozen snapshots reproduce even if workspace/plugin templates change.
    let mut plugin_archive_meta: Option<(String, String, String, Vec<String>)> = None;
    let mut plugin_bundle_files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let (eff_template, template_bytes, merged_opts) = if ws.has_trd_template(trd_id) {
        let rel = format!("trds/{trd_id}/template.typ");
        let bytes = ws.read_bytes(&rel)?;
        let opts = trd
            .meta
            .template_options
            .get("local")
            .cloned()
            .unwrap_or_default();
        ("local".to_owned(), Some(bytes), opts)
    } else {
        let eff = trd
            .meta
            .template
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("trd-compact")
            .to_owned();
        let mut opts = ws
            .workspace_template_options()
            .remove(&eff)
            .unwrap_or_default();
        if let Some(trd_opts) = trd.meta.template_options.get(&eff) {
            for (k, v) in trd_opts {
                opts.insert(k.clone(), v.clone());
            }
        }
        if let Some((pid, tid)) = parse_plugin_template_ref(&eff) {
            let Some((mount, tpl_entry)) = engine.find_plugin_template(pid, tid) else {
                return Err(Error::NotFound(format!("plugin template {eff:?}")));
            };
            plugin_bundle_files = collect_plugin_bundle_files(mount)?;
            let rel_font_dirs = relative_font_dirs(mount);
            plugin_archive_meta = Some((
                mount.plugin_id.clone(),
                mount.version.clone(),
                tpl_entry.entry.clone(),
                rel_font_dirs,
            ));
            (eff, None, opts)
        } else {
            let bytes = if ws.has_custom_template(&eff) {
                ws.read_bytes(&format!("templates/{eff}.typ"))?
            } else if let Some(src) = xcpc_engine::preset_source(&eff) {
                src.as_bytes().to_vec()
            } else {
                return Err(Error::NotFound(format!("template {eff:?}")));
            };
            (eff, Some(bytes), opts)
        }
    };

    let preamble_bytes = ws.load_preamble().map(String::into_bytes);
    let (plugin_id, plugin_version, entry, font_dirs) = match plugin_archive_meta {
        Some((pid, ver, ent, fdirs)) => (Some(pid), Some(ver), Some(ent), fdirs),
        None => (None, None, None, Vec::new()),
    };
    let archive = SnapshotTemplateArchive {
        meta: SnapshotTemplateMeta {
            template: eff_template,
            has_custom_template: true,
            has_preamble: preamble_bytes.is_some(),
            plugin_id,
            plugin_version,
            entry,
            font_dirs,
            options: merged_opts,
        },
        custom_template_bytes: template_bytes,
        preamble_bytes,
        assets: BTreeMap::new(),
        plugin_bundle_files,
    };

    let result = ws.freeze_trd(
        trd_id,
        strict,
        verify_statuses,
        Some(&archive),
        Some(&pdf_bytes),
    )?;
    Ok((Some(result), report))
}
