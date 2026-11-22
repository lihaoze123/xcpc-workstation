//! The publishing pipeline the GUI, the CLI and MCP share: workspace → Typst →
//! diagnostics, pages and PDF.
//!
//! [`document`] reads a problem or a contest and converts it; [`compile`] typesets it
//! with an [`Engine`] rooted at the workspace and reports diagnostics in workspace
//! files (`problems/a/statement.md:3:5`), where the problems start, and overflows.
//! [`check`] combines conversion, compilation, overflow checks and `xcpc-lint` into one
//! deduplicated [`Report`]; [`locate_point`] maps preview coordinates back to Markdown;
//! [`freeze_contest`] validates, builds PDFs and atomically archives a contest snapshot.

use std::collections::BTreeMap;

use serde::Serialize;
use xcpc_domain::{
    Language, Statement, TemplateInfo, TemplateOptionValue, TemplateOptionsMap, TextFile, check_id,
};
use xcpc_engine::{Compiled, Engine};
use xcpc_storage::{Error, SnapshotTemplateArchive, SnapshotTemplateMeta, Workspace};

use crate::{Book, Document, Entry};

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
        let mut merged: BTreeMap<String, TemplateOptionValue> = ws_template_opts
            .get(tpl_name)
            .cloned()
            .unwrap_or_default();
        if let Some(c_opts) = contest_opts.and_then(|m| m.get(tpl_name)) {
            for (k, v) in c_opts {
                merged.insert(k.clone(), v.clone());
            }
        }
        merged
    };
    let resolve_book = |book: &mut Book,
                        contest_template: Option<&str>,
                        contest_opts: Option<&TemplateOptionsMap>|
     -> Option<crate::Diagnostic> {
        book.preamble = ws.load_preamble();
        let wanted = contest_template
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .or_else(|| ws.default_template());
        let Some(name) = wanted else {
            book.template_name = "olymp-classic".into();
            book.template_options = merge_book_options("olymp-classic", contest_opts);
            return None;
        };
        if check_id(&name).is_ok() && ws.has_custom_template(&name) {
            book.template_name = name.clone();
            book.template_import = Some(format!("/templates/{name}.typ"));
            book.template_options = merge_book_options(&name, contest_opts);
            return None;
        }
        match name.as_str() {
            "olymp-classic" => {
                book.template_name = "olymp-classic".into();
                book.template_import = Some("/xcpc/olymp-classic.typ".into());
                book.template_options = merge_book_options("olymp-classic", contest_opts);
                None
            }
            "cnoi-classic" => {
                book.template_name = "cnoi-classic".into();
                book.template_import = Some("/xcpc/cnoi-classic.typ".into());
                book.template_options = merge_book_options("cnoi-classic", contest_opts);
                None
            }
            _ => {
                book.template_name = "olymp-classic".into();
                book.template_options = merge_book_options("olymp-classic", contest_opts);
                Some(crate::Diagnostic {
                    error: false,
                    code: Some("unknown_template".into()),
                    message: format!(
                        "未找到模板「{name}」（已回退到 olymp-classic）；请在 templates/{name}.typ 中提供该模板"
                    ),
                    file: if contest_template.is_some() {
                        format!("contests/{}/contest.toml", target.id())
                    } else {
                        "workspace.toml".into()
                    },
                    line: 1,
                    column: 1,
                })
            }
        }
    };
    match target {
        Target::Problem(id) => {
            let mut problem = ws.load_problem(id)?;
            overlay(&mut problem);
            let mut book = Book::single(language.unwrap_or_else(|| problem.default_language()));
            let warn = resolve_book(&mut book, None, None);
            let entries = [Entry::single(&problem)];
            let mut doc = match kind {
                Kind::Statements => crate::statements(&book, &entries),
                Kind::Editorial => crate::editorials(&book, &entries),
            };
            if let Some(w) = warn {
                doc.diagnostics.push(w);
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
            let warn = if contest.meta.frozen
                && let Some(snap) = ws.load_snapshot_template(id)
            {
                book.template_name = snap.template.clone();
                book.template_options = snap.options;
                book.preamble = if snap.has_preamble {
                    ws.load_snapshot_preamble(id)
                } else {
                    None
                };
                if snap.has_custom_template {
                    book.template_import = Some(format!("/contests/{id}/snapshot/template.typ"));
                    None
                } else {
                    match snap.template.as_str() {
                        "cnoi-classic" => {
                            book.template_import = Some("/xcpc/cnoi-classic.typ".into());
                            None
                        }
                        _ => {
                            book.template_import = Some("/xcpc/olymp-classic.typ".into());
                            None
                        }
                    }
                }
            } else {
                resolve_book(
                    &mut book,
                    contest.meta.template.as_deref(),
                    Some(&contest.meta.template_options),
                )
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
            if let Some(w) = warn {
                doc.diagnostics.push(w);
            }
            Ok(doc)
        }
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

/// Typesets `doc` with `engine` (rooted at the workspace).
pub fn compile(engine: &mut Engine, doc: &Document) -> (Option<Compiled>, Report) {
    engine.set_main(&doc.source);
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
    let result = engine.compile();
    let compile_ms = start.elapsed().as_millis() as u64;
    match result {
        Ok(compiled) => {
            diagnostics.extend(compiled.warnings.iter().map(map));
            let total = compiled.page_count();
            let mut starts = compiled.starts("xcpc-problem");
            if starts.is_empty() {
                starts = compiled.starts("xcpc-tutorial");
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
    )
}

fn check_inner(
    ws: &Workspace,
    engine: &mut Engine,
    target: &Target,
    kind: Kind,
    language: Option<Language>,
    unsaved: Option<(&str, &str)>,
    check_editorial: bool,
) -> Result<(Option<Compiled>, Option<Document>, Report), Error> {
    let lint_opts = xcpc_lint::LintOptions {
        language,
        check_editorial,
        unsaved,
    };
    let lint_findings = match target {
        Target::Problem(id) => xcpc_lint::lint_problem(ws, id, &lint_opts)?,
        Target::Contest(id) => xcpc_lint::lint_contest(ws, id, &lint_opts)?,
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

    let doc = document_with(ws, target, kind, language, unsaved)?;
    engine.reset_files();
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
    if contest.meta.frozen {
        return Err(Error::AlreadyFrozen(contest_id.to_owned()));
    }

    let target = Target::Contest(contest_id.to_owned());
    let (compiled_stmts, _doc_stmts, mut report) =
        check_inner(ws, engine, &target, Kind::Statements, None, None, true)?;

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
        let ed_doc = document(ws, &target, Kind::Editorial, None)?;
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
    // defaults, custom templates, or `.xcpc/preamble.typ` later change.
    let eff_template = contest
        .meta
        .template
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .or_else(|| ws.default_template())
        .unwrap_or_else(|| "olymp-classic".into());
    let custom_template_bytes = if ws.has_custom_template(&eff_template) {
        ws.read_bytes(&format!("templates/{eff_template}.typ")).ok()
    } else {
        None
    };
    let preamble_bytes = ws.load_preamble().map(String::into_bytes);
    let mut merged_opts = ws
        .workspace_template_options()
        .remove(&eff_template)
        .unwrap_or_default();
    if let Some(c_opts) = contest.meta.template_options.get(&eff_template) {
        for (k, v) in c_opts {
            merged_opts.insert(k.clone(), v.clone());
        }
    }
    let archive = SnapshotTemplateArchive {
        meta: SnapshotTemplateMeta {
            template: eff_template,
            has_custom_template: custom_template_bytes.is_some(),
            has_preamble: preamble_bytes.is_some(),
            options: merged_opts,
        },
        custom_template_bytes,
        preamble_bytes,
    };

    ws.freeze_commit_with_template(
        contest_id,
        &statements_pdf,
        editorial_pdf.as_deref(),
        Some(&archive),
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

/// Inspects a single template (`"olymp-classic"`, `"cnoi-classic"`, or a workspace
/// `templates/<name>.typ`) and returns its metadata and declarative `#let schema`.
pub fn inspect_template(ws: &Workspace, engine: &Engine, name: &str) -> Option<TemplateInfo> {
    let custom = ws.has_custom_template(name);
    let builtin = matches!(name, "olymp-classic" | "cnoi-classic");
    if !custom && !builtin {
        return None;
    }
    let import_path = if custom {
        format!("/templates/{name}.typ")
    } else {
        format!("/xcpc/{name}.typ")
    };
    let schema = engine.inspect_template_schema(&import_path);
    let display_name = schema.name.clone().unwrap_or_else(|| name.to_owned());
    let description = schema.description.clone();
    Some(TemplateInfo {
        id: name.to_owned(),
        builtin: builtin && !custom,
        name: display_name,
        description,
        schema,
    })
}

/// Lists all available templates in `ws` (built-in + `templates/*.typ`) with their
/// evaluated declarative schemas.
pub fn list_templates(ws: &Workspace, engine: &Engine) -> Vec<TemplateInfo> {
    ws.list_templates()
        .into_iter()
        .filter_map(|id| inspect_template(ws, engine, &id))
        .collect()
}

/// Forks an existing built-in or workspace template `source_name` into a new custom
/// workspace template `templates/<new_name>.typ`, returning the relative path.
pub fn fork_template(ws: &Workspace, source_name: &str, new_name: &str) -> Result<String, Error> {
    check_id(source_name)?;
    check_id(new_name)?;
    let bytes = if ws.has_custom_template(source_name) {
        ws.read_bytes(&format!("templates/{source_name}.typ"))?
    } else if let Some(src) = xcpc_engine::builtin_template_source(source_name) {
        src.as_bytes().to_vec()
    } else {
        return Err(Error::NotFound(format!("template {source_name:?}")));
    };
    ws.fork_template(new_name, &bytes)
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
        Target::Problem(id) => ws.load_problem(id).ok().map(|p| p.meta.title),
    };
    default_output_for_title(target, kind, title.as_deref())
}
