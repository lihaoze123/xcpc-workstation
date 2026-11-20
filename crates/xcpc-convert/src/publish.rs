//! The publishing pipeline the GUI, the CLI and MCP share: workspace → Typst →
//! diagnostics, pages and PDF.
//!
//! [`document`] reads a problem or a contest and converts it; [`compile`] typesets it
//! with an [`Engine`] rooted at the workspace and reports diagnostics in workspace
//! files (`problems/a/statement.md:3:5`), where the problems start, and overflows.

use serde::Serialize;
use xcpc_domain::Language;
use xcpc_engine::{Compiled, Engine};
use xcpc_storage::{Error, Workspace};

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
        let files = problem
            .statements
            .iter_mut()
            .map(|s| &mut s.file)
            .chain(problem.editorial.as_mut());
        for file in files.filter(|f| f.path == path) {
            file.text = text.to_owned();
        }
    };
    match target {
        Target::Problem(id) => {
            let mut problem = ws.load_problem(id)?;
            overlay(&mut problem);
            let book = Book::single(language.unwrap_or_else(|| problem.default_language()));
            let entries = [Entry::single(&problem)];
            Ok(match kind {
                Kind::Statements => crate::statements(&book, &entries),
                Kind::Editorial => crate::editorials(&book, &entries),
            })
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
            let entries: Vec<Entry> = problems
                .iter()
                .map(|(item, problem)| Entry {
                    item: item.clone(),
                    problem,
                })
                .collect();
            Ok(match kind {
                Kind::Statements => crate::statements(&book, &entries),
                Kind::Editorial => crate::editorials(&book, &entries),
            })
        }
    }
}

/// A diagnostic for people and agents.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Diagnostic {
    /// `error` or `warning`.
    pub severity: &'static str,
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

/// Typesets `doc` with `engine` (rooted at the workspace).
pub fn compile(engine: &mut Engine, doc: &Document) -> (Option<Compiled>, Report) {
    engine.set_main(&doc.source);
    let mut diagnostics: Vec<Diagnostic> = doc
        .diagnostics
        .iter()
        .map(|d| Diagnostic {
            severity: if d.error { "error" } else { "warning" },
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

/// The default name of an exported PDF, relative to the workspace:
/// `build/<id>.pdf`, `build/<id>-editorial.pdf`.
pub fn default_output(target: &Target, kind: Kind) -> String {
    match kind {
        Kind::Statements => format!("build/{}.pdf", target.id()),
        Kind::Editorial => format!("build/{}-editorial.pdf", target.id()),
    }
}
