//! Minimal preflight linter for XCPC problems and contests (docs/roadmap.md Phase 2 §5).
//!
//! Only catches defects that would break a published booklet or freeze archive — no
//! style checks, no rule switches, and no ignore comments:
//!
//! - **Errors** (block freezing):
//!   - [`code::MISSING_PROBLEM`]: a contest references a problem that does not exist on
//!     disk (`problems/<id>` when unfrozen, `contests/<cid>/snapshot/<label>` when frozen).
//!   - [`code::MISSING_IMAGE`]: a local image referenced in `statement*.md` or
//!     `editorial.md` does not exist on disk, is an external URL, or leaves the problem
//!     directory (with exact `file`, `line`, `column`).
//! - **Warnings** (do not block freezing):
//!   - [`code::SAMPLE_UNPAIRED`]: orphan `.in` or `.ans`/`.out` file in `samples/`.
//!   - [`code::SAMPLE_NON_CONTIGUOUS`]: paired samples are not contiguously numbered
//!     starting from `01`.
//!   - [`code::MISSING_IO_SECTION`]: a statement without an `interaction` section lacks
//!     an `input` section or lacks an `output` section.
//!   - [`code::MISSING_EDITORIAL`]: when [`LintOptions::check_editorial`] is `true`, a
//!     problem has no `editorial.md`.

use std::collections::HashSet;
use std::fs;

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use serde::{Deserialize, Serialize};
use xcpc_domain::{Language, SectionKind, recognize_section};
use xcpc_judge::PackageFreshness;
use xcpc_storage::{Error, Workspace};

/// Stable machine-readable lint rule codes.
pub mod code {
    pub const MISSING_PROBLEM: &str = "missing_problem";
    pub const MISSING_IMAGE: &str = "missing_image";
    pub const SAMPLE_UNPAIRED: &str = "sample_unpaired";
    pub const SAMPLE_NON_CONTIGUOUS: &str = "sample_non_contiguous";
    pub const MISSING_IO_SECTION: &str = "missing_io_section";
    pub const MISSING_INTERACTION_SECTION: &str = "missing_interaction_section";
    pub const MISSING_SUBTASKS_SECTION: &str = "missing_subtasks_section";
    pub const MISSING_EDITORIAL: &str = "missing_editorial";
    pub const PACKAGE_UNVERIFIED: &str = "package_unverified";
    pub const PACKAGE_STALE: &str = "package_stale";
    pub const PACKAGE_FAILED: &str = "package_failed";
}

/// Diagnostic severity for a lint [`Finding`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
        }
    }
}

/// One lint finding pointing into the workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub severity: Severity,
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<usize>,
}

impl Finding {
    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }
}

/// Options controlling a lint pass.
#[derive(Debug, Clone, Copy, Default)]
pub struct LintOptions<'a> {
    /// Preferred language context (reserved for caller alignment; all existing
    /// `statement*.md` files of a problem are checked).
    pub language: Option<Language>,
    /// Whether to warn (`missing_editorial`) when a problem has no `editorial.md`.
    /// Enabled when checking/exporting editorial books or freezing a contest.
    pub check_editorial: bool,
    /// Optional unsaved editor buffer `(workspace_relative_path, text)` overlaying disk.
    pub unsaved: Option<(&'a str, &'a str)>,
}

/// Validates and normalizes an image URL referenced inside `problem_dir` into a
/// workspace-relative `/`-separated path (without a leading `/`).
///
/// Rejects empty paths, external/data URLs, absolute paths, backslashes, `..` escaping
/// `problem_dir`, and paths pointing to `problem_dir` itself.
pub fn resolve_image_path(problem_dir: &str, url: &str) -> Result<String, String> {
    if url.trim().is_empty() {
        return Err("picture path is empty".to_owned());
    }
    if url.contains("://") || url.starts_with("data:") {
        return Err(format!(
            "picture {url:?}: only files of the problem can be shown"
        ));
    }
    if url.starts_with('/') {
        return Err(format!(
            "picture {url:?}: write a path relative to the problem"
        ));
    }
    if url.contains('\\') {
        return Err(format!(
            "picture {url:?}: use '/' in relative picture paths"
        ));
    }
    let mut parts: Vec<&str> = problem_dir.split('/').filter(|p| !p.is_empty()).collect();
    let base = parts.len();
    for part in url.split('/') {
        match part {
            "" | "." => {}
            ".." if parts.len() > base => {
                parts.pop();
            }
            ".." => {
                return Err(format!(
                    "picture {url:?} is outside the problem's directory"
                ));
            }
            p => parts.push(p),
        }
    }
    if parts.len() == base {
        return Err(format!(
            "picture {url:?}: write a file path inside the problem"
        ));
    }
    Ok(parts.join("/"))
}

/// Runs all problem-level lint rules on `problems/<id>`.
///
/// Returns `Err(Error::NotFound)` if the problem itself does not exist.
pub fn lint_problem(
    ws: &Workspace,
    id: &str,
    opts: &LintOptions<'_>,
) -> Result<Vec<Finding>, Error> {
    let problem = ws.load_problem(id)?;
    let mut findings = Vec::new();
    lint_problem_dir(
        ws,
        None,
        &problem.id,
        None,
        &problem.dir,
        false,
        opts,
        &mut findings,
    )?;
    Ok(findings)
}

/// Runs all contest-level and per-problem lint rules on `contests/<id>`.
///
/// Returns `Err(Error::NotFound)` if the contest itself does not exist. If one or more
/// problems referenced by `contest.toml` are missing, records [`code::MISSING_PROBLEM`]
/// errors instead of failing with `NotFound`, and still lints every remaining problem
/// that exists.
pub fn lint_contest(
    ws: &Workspace,
    id: &str,
    opts: &LintOptions<'_>,
) -> Result<Vec<Finding>, Error> {
    let contest = ws.load_contest(id)?;
    let contest_toml = format!("{}/contest.toml", contest.dir);
    let mut findings = Vec::new();
    let mut seen_dirs: HashSet<String> = HashSet::new();

    for item in &contest.meta.problems {
        if contest.meta.frozen {
            let snap_dir = format!("{}/snapshot/{}", contest.dir, item.label);
            let meta_rel = format!("{snap_dir}/problem.toml");
            if !ws.resolve(&meta_rel)?.is_file() {
                findings.push(Finding {
                    severity: Severity::Error,
                    code: code::MISSING_PROBLEM.to_owned(),
                    message: format!(
                        "problem {} ({:?}) not found in snapshot ({snap_dir})",
                        item.label, item.id
                    ),
                    file: Some(contest_toml.clone()),
                    line: None,
                    column: None,
                });
                continue;
            }
            if seen_dirs.insert(snap_dir.clone()) {
                lint_problem_dir(
                    ws,
                    Some(&contest.id),
                    &item.id,
                    Some(&item.label),
                    &snap_dir,
                    true,
                    opts,
                    &mut findings,
                )?;
            }
        } else {
            match ws.load_problem(&item.id) {
                Ok(problem) => {
                    if seen_dirs.insert(problem.dir.clone()) {
                        lint_problem_dir(
                            ws,
                            Some(&contest.id),
                            &problem.id,
                            Some(&item.label),
                            &problem.dir,
                            false,
                            opts,
                            &mut findings,
                        )?;
                    }
                }
                Err(Error::NotFound(_)) => {
                    findings.push(Finding {
                        severity: Severity::Error,
                        code: code::MISSING_PROBLEM.to_owned(),
                        message: format!(
                            "problem {} ({:?}) not found in problems/{}",
                            item.label, item.id, item.id
                        ),
                        file: Some(contest_toml.clone()),
                        line: None,
                        column: None,
                    });
                }
                Err(other) => return Err(other),
            }
        }
    }

    Ok(findings)
}

#[allow(clippy::too_many_arguments)]
fn lint_problem_dir(
    ws: &Workspace,
    contest_id: Option<&str>,
    problem_id: &str,
    label: Option<&str>,
    dir: &str,
    is_snapshot: bool,
    opts: &LintOptions<'_>,
    findings: &mut Vec<Finding>,
) -> Result<(), Error> {
    // 1. Samples: unpaired files and non-contiguous numbering.
    lint_samples(ws, dir, findings)?;

    // Detect Phase 3C package features (`interactor.cpp` and `[[subtask]]`) for non-blocking
    // statement section hints.
    let has_interactor = !is_snapshot
        && ws
            .resolve(dir)
            .ok()
            .is_some_and(|p| xcpc_judge::find_interactor_source(&p).is_some());
    let has_subtasks = !is_snapshot
        && ws
            .load_problem(problem_id)
            .ok()
            .is_some_and(|p| !p.meta.subtask.is_empty());

    // 2. Statements (`statement.md`, `statement.zh.md`, `statement.en.md`):
    //    check images (`missing_image`), I/O sections (`missing_io_section`),
    //    interactive protocol section (`missing_interaction_section`), and subtasks section
    //    (`missing_subtasks_section`).
    for name in ["statement.md", "statement.zh.md", "statement.en.md"] {
        let rel = format!("{dir}/{name}");
        if let Some(text) = read_with_unsaved(ws, &rel, opts.unsaved)? {
            lint_markdown_file(
                ws,
                dir,
                &rel,
                &text,
                true,
                has_interactor,
                has_subtasks,
                findings,
            )?;
        }
    }

    // 3. Editorial (`editorial.md`):
    //    check images (`missing_image`) when present; warn `missing_editorial` when
    //    absent and `opts.check_editorial` is enabled.
    let editorial_rel = format!("{dir}/editorial.md");
    match read_with_unsaved(ws, &editorial_rel, opts.unsaved)? {
        Some(text) => {
            lint_markdown_file(
                ws,
                dir,
                &editorial_rel,
                &text,
                false,
                false,
                false,
                findings,
            )?;
        }
        None if opts.check_editorial => {
            let message = match label {
                Some(lbl) => format!("problem {lbl} ({problem_id}) has no editorial"),
                None => format!("problem {problem_id:?} has no editorial"),
            };
            findings.push(Finding {
                severity: Severity::Warning,
                code: code::MISSING_EDITORIAL.to_owned(),
                message,
                file: Some(editorial_rel),
                line: None,
                column: None,
            });
        }
        None => {}
    }

    // 4. Static package freshness check (never compiles or executes anything).
    //    Frozen contest snapshots exclude package files and are already frozen.
    if !is_snapshot {
        lint_package_freshness(ws, contest_id, problem_id, label, dir, findings);
    }

    Ok(())
}

fn lint_package_freshness(
    ws: &Workspace,
    contest_id: Option<&str>,
    problem_id: &str,
    label: Option<&str>,
    dir: &str,
    findings: &mut Vec<Finding>,
) {
    let prefix = match label {
        Some(lbl) => format!("题目 {lbl}（{problem_id}）"),
        None => format!("题目「{problem_id}」"),
    };
    let meta_rel = format!("{dir}/problem.toml");
    match xcpc_judge::check_package_freshness_in(ws, problem_id, contest_id) {
        Ok(PackageFreshness::NotAPackage | PackageFreshness::VerifiedFresh { .. }) => {}
        Ok(PackageFreshness::Unverified { .. }) => {
            findings.push(Finding {
                severity: Severity::Error,
                code: code::PACKAGE_UNVERIFIED.to_owned(),
                message: format!("{prefix}的测试数据还没检查，请先检查"),
                file: Some(meta_rel),
                line: None,
                column: None,
            });
        }
        Ok(PackageFreshness::Stale { .. }) => {
            findings.push(Finding {
                severity: Severity::Error,
                code: code::PACKAGE_STALE.to_owned(),
                message: format!("{prefix}改动后还没重新检查，请重新检查"),
                file: Some(meta_rel),
                line: None,
                column: None,
            });
        }
        Ok(PackageFreshness::Failed { summary, .. }) => {
            findings.push(Finding {
                severity: Severity::Error,
                code: code::PACKAGE_FAILED.to_owned(),
                message: format!("{prefix}检查有问题（{summary}），请修复后重新检查"),
                file: Some(meta_rel),
                line: None,
                column: None,
            });
        }
        Err(err) => {
            findings.push(Finding {
                severity: Severity::Error,
                code: code::PACKAGE_FAILED.to_owned(),
                message: format!("{prefix}的题目包状态检查失败：{err}"),
                file: Some(meta_rel),
                line: None,
                column: None,
            });
        }
    }
}

/// Reads `rel` from disk, applying `unsaved` if it matches `rel`.
///
/// If the file exists on disk, `unsaved` always replaces its text (even if empty). If
/// the file does not exist on disk yet, `unsaved` activates it only when non-empty so
/// an untouched empty editor tab for a missing `editorial.md` still reports missing.
fn read_with_unsaved(
    ws: &Workspace,
    rel: &str,
    unsaved: Option<(&str, &str)>,
) -> Result<Option<String>, Error> {
    let on_disk = match ws.read_text(rel) {
        Ok(text) => Some(text),
        Err(Error::NotFound(_)) => None,
        Err(e) => return Err(e),
    };
    if let Some((u_path, u_text)) = unsaved
        && u_path == rel
    {
        if on_disk.is_some() || !u_text.is_empty() {
            return Ok(Some(u_text.to_owned()));
        }
        return Ok(None);
    }
    Ok(on_disk)
}

fn lint_samples(ws: &Workspace, dir: &str, findings: &mut Vec<Finding>) -> Result<(), Error> {
    let samples_dir = format!("{dir}/samples");
    let inv = ws.inspect_samples(&samples_dir)?;

    for file in inv.orphan_inputs {
        findings.push(Finding {
            severity: Severity::Warning,
            code: code::SAMPLE_UNPAIRED.to_owned(),
            message: format!("sample input {file:?} has no matching .ans/.out file"),
            file: Some(format!("{samples_dir}/{file}")),
            line: None,
            column: None,
        });
    }
    for file in inv.orphan_outputs {
        findings.push(Finding {
            severity: Severity::Warning,
            code: code::SAMPLE_UNPAIRED.to_owned(),
            message: format!("sample output {file:?} has no matching .in file"),
            file: Some(format!("{samples_dir}/{file}")),
            line: None,
            column: None,
        });
    }
    if inv.non_contiguous {
        findings.push(Finding {
            severity: Severity::Warning,
            code: code::SAMPLE_NON_CONTIGUOUS.to_owned(),
            message: format!(
                "sample numbering in {samples_dir} is not contiguous from 01 (found: {})",
                inv.pairs.join(", ")
            ),
            file: Some(samples_dir),
            line: None,
            column: None,
        });
    }

    Ok(())
}

fn markdown_options() -> Options {
    let mut options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_MATH
        | Options::ENABLE_GFM
        | Options::ENABLE_SUPERSCRIPT
        | Options::ENABLE_SUBSCRIPT;
    options.remove(Options::ENABLE_SMART_PUNCTUATION);
    options
}

#[allow(clippy::too_many_arguments)]
fn lint_markdown_file(
    ws: &Workspace,
    problem_dir: &str,
    file_rel: &str,
    text: &str,
    check_io_sections: bool,
    has_interactor: bool,
    has_subtasks: bool,
    findings: &mut Vec<Finding>,
) -> Result<(), Error> {
    let mut active_heading: Option<String> = None;
    let mut has_input = false;
    let mut has_output = false;
    let mut has_interaction = false;
    let mut has_subtasks_section = false;

    for (event, range) in Parser::new_ext(text, markdown_options()).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { level, .. })
                if check_io_sections && level <= HeadingLevel::H2 =>
            {
                active_heading = Some(String::new());
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some(title) = active_heading.take() {
                    match recognize_section(&title) {
                        Some(SectionKind::Known("input")) => has_input = true,
                        Some(SectionKind::Known("output")) => has_output = true,
                        Some(SectionKind::Known("interaction")) => has_interaction = true,
                        Some(SectionKind::Known("subtasks")) => has_subtasks_section = true,
                        _ => {}
                    }
                }
            }
            Event::Text(chunk) => {
                if let Some(heading) = active_heading.as_mut() {
                    heading.push_str(&chunk);
                }
            }
            Event::SoftBreak => {
                if let Some(heading) = active_heading.as_mut() {
                    heading.push(' ');
                }
            }
            Event::Start(Tag::Image { dest_url, .. }) => {
                check_image(
                    ws,
                    problem_dir,
                    file_rel,
                    text,
                    range.start,
                    &dest_url,
                    findings,
                )?;
            }
            _ => {}
        }
    }

    if check_io_sections {
        if !has_interaction {
            if has_interactor {
                findings.push(Finding {
                    severity: Severity::Warning,
                    code: code::MISSING_INTERACTION_SECTION.to_owned(),
                    message: "interactive problem (interactor.cpp) is missing an interaction section (e.g. ## 交互方式 / ## Interaction)".to_owned(),
                    file: Some(file_rel.to_owned()),
                    line: None,
                    column: None,
                });
            } else {
                if !has_input {
                    findings.push(Finding {
                        severity: Severity::Warning,
                        code: code::MISSING_IO_SECTION.to_owned(),
                        message:
                            "statement is missing an input section (e.g. ## 输入格式 / ## Input)"
                                .to_owned(),
                        file: Some(file_rel.to_owned()),
                        line: None,
                        column: None,
                    });
                }
                if !has_output {
                    findings.push(Finding {
                        severity: Severity::Warning,
                        code: code::MISSING_IO_SECTION.to_owned(),
                        message:
                            "statement is missing an output section (e.g. ## 输出格式 / ## Output)"
                                .to_owned(),
                        file: Some(file_rel.to_owned()),
                        line: None,
                        column: None,
                    });
                }
            }
        }
        if has_subtasks && !has_subtasks_section {
            findings.push(Finding {
                severity: Severity::Warning,
                code: code::MISSING_SUBTASKS_SECTION.to_owned(),
                message: "problem declares [[subtask]] but statement is missing a subtasks section (e.g. ## 子任务 / ## Subtasks)".to_owned(),
                file: Some(file_rel.to_owned()),
                line: None,
                column: None,
            });
        }
    }

    Ok(())
}

fn check_image(
    ws: &Workspace,
    problem_dir: &str,
    source_file: &str,
    source_text: &str,
    offset: usize,
    url: &str,
    findings: &mut Vec<Finding>,
) -> Result<(), Error> {
    let (line, column) = line_column(source_text, offset);
    let ws_rel = match resolve_image_path(problem_dir, url) {
        Ok(rel) => rel,
        Err(message) => {
            findings.push(Finding {
                severity: Severity::Error,
                code: code::MISSING_IMAGE.to_owned(),
                message,
                file: Some(source_file.to_owned()),
                line: Some(line),
                column: Some(column),
            });
            return Ok(());
        }
    };

    let resolved = match ws.resolve(&ws_rel) {
        Ok(path) => path,
        Err(Error::PathEscape(_)) => {
            findings.push(Finding {
                severity: Severity::Error,
                code: code::MISSING_IMAGE.to_owned(),
                message: format!("picture {url:?} is outside the workspace"),
                file: Some(source_file.to_owned()),
                line: Some(line),
                column: Some(column),
            });
            return Ok(());
        }
        Err(e) => return Err(e),
    };

    if !resolved.is_file() {
        findings.push(Finding {
            severity: Severity::Error,
            code: code::MISSING_IMAGE.to_owned(),
            message: format!("picture {url:?} not found ({ws_rel})"),
            file: Some(source_file.to_owned()),
            line: Some(line),
            column: Some(column),
        });
        return Ok(());
    }

    // Also guard against symlinks inside the problem directory targeting another directory
    // inside the workspace.
    if let (Ok(real_file), Ok(dir_path)) = (fs::canonicalize(&resolved), ws.resolve(problem_dir))
        && let Ok(real_dir) = fs::canonicalize(&dir_path)
        && !real_file.starts_with(&real_dir)
    {
        findings.push(Finding {
            severity: Severity::Error,
            code: code::MISSING_IMAGE.to_owned(),
            message: format!("picture {url:?} is outside the problem's directory"),
            file: Some(source_file.to_owned()),
            line: Some(line),
            column: Some(column),
        });
    }

    Ok(())
}

fn line_column(text: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(text.len());
    let before = &text[..text.floor_char_boundary(offset)];
    let line = before.matches('\n').count() + 1;
    let column = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
    (line, column)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_ws() -> (tempfile::TempDir, Workspace) {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("workspace.toml"), "").unwrap();
        let ws = Workspace::open(dir.path(), false).unwrap();
        (dir, ws)
    }

    fn write_problem(dir: &std::path::Path, id: &str, statement: &str, editorial: Option<&str>) {
        let p = dir.join("problems").join(id);
        fs::create_dir_all(p.join("samples")).unwrap();
        fs::write(p.join("problem.toml"), format!("title = \"{id}\"\n")).unwrap();
        fs::write(p.join("statement.md"), statement).unwrap();
        if let Some(ed) = editorial {
            fs::write(p.join("editorial.md"), ed).unwrap();
        }
    }

    #[test]
    fn clean_problem_has_no_findings() {
        let (dir, ws) = scratch_ws();
        write_problem(
            dir.path(),
            "p",
            "描述。\n\n## 输入格式\n\n一个数。\n\n## 输出格式\n\n答案。\n",
            Some("## 做法\n\n直接做。\n"),
        );
        fs::write(dir.path().join("problems/p/samples/01.in"), "1\n").unwrap();
        fs::write(dir.path().join("problems/p/samples/01.ans"), "1\n").unwrap();

        let findings = lint_problem(
            &ws,
            "p",
            &LintOptions {
                check_editorial: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(findings.is_empty(), "{findings:?}");
    }

    #[test]
    fn missing_target_returns_not_found() {
        let (_dir, ws) = scratch_ws();
        assert!(matches!(
            lint_problem(&ws, "nope", &LintOptions::default()),
            Err(Error::NotFound(_))
        ));
        assert!(matches!(
            lint_contest(&ws, "nope", &LintOptions::default()),
            Err(Error::NotFound(_))
        ));
    }

    #[test]
    fn missing_image_checks_disk_external_escape_and_editorial_with_exact_position() {
        let (dir, ws) = scratch_ws();
        let stmt = "第一行。\n\n## 输入\n\n图1：![ok](assets/ok.png) ![miss](assets/miss.png)\n![url](https://example.com/a.png)\n![esc](../other/x.png)\n![abs](/etc/passwd)\n\n```md\n![in-code](assets/ignored.png)\n```\n\n## 输出\n\n一个数。\n";
        let ed = "## 做法\n\n![ed-miss](assets/ed-miss.png)\n";
        write_problem(dir.path(), "p", stmt, Some(ed));
        fs::create_dir_all(dir.path().join("problems/p/assets")).unwrap();
        fs::write(dir.path().join("problems/p/assets/ok.png"), b"png").unwrap();

        let findings = lint_problem(&ws, "p", &LintOptions::default()).unwrap();
        assert_eq!(findings.len(), 5, "{findings:#?}");
        assert!(
            findings
                .iter()
                .all(|f| f.is_error() && f.code == code::MISSING_IMAGE)
        );

        // Verify exact line and column for each image.
        let locs: Vec<_> = findings
            .iter()
            .map(|f| (f.file.as_deref(), f.line, f.column))
            .collect();
        assert_eq!(
            locs,
            vec![
                (Some("problems/p/statement.md"), Some(5), Some(25)),
                (Some("problems/p/statement.md"), Some(6), Some(1)),
                (Some("problems/p/statement.md"), Some(7), Some(1)),
                (Some("problems/p/statement.md"), Some(8), Some(1)),
                (Some("problems/p/editorial.md"), Some(3), Some(1)),
            ]
        );
    }

    #[test]
    fn resolve_image_path_edge_cases() {
        assert_eq!(
            resolve_image_path("problems/a", "assets/fig.png").unwrap(),
            "problems/a/assets/fig.png"
        );
        assert_eq!(
            resolve_image_path("problems/a", "./assets/../sub/fig.svg").unwrap(),
            "problems/a/sub/fig.svg"
        );
        for bad in [
            "",
            "   ",
            ".",
            "./",
            "assets/..",
            "../other/a.png",
            "assets/../../other/a.png",
            "/etc/passwd",
            r"assets\a.png",
            "https://example.com/a.png",
            "data:image/png;base64,AAA",
        ] {
            assert!(resolve_image_path("problems/a", bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn bilingual_statements_and_reference_images_are_checked() {
        let (dir, ws) = scratch_ws();
        let p = dir.path().join("problems/bi");
        fs::create_dir_all(&p).unwrap();
        fs::write(p.join("problem.toml"), "title = \"Bi\"\n").unwrap();
        fs::write(
            p.join("statement.zh.md"),
            "## **输入格式**\n\n见图 ![图][r1]。\n\n## 输出格式\n\n无。\n\n[r1]: assets/ref-miss.png\n",
        )
        .unwrap();
        fs::write(p.join("statement.en.md"), "## Input\n\nMissing output.\n").unwrap();

        let findings = lint_problem(&ws, "bi", &LintOptions::default()).unwrap();
        assert_eq!(findings.len(), 2, "{findings:#?}");
        assert!(findings.iter().any(|f| {
            f.is_error()
                && f.code == code::MISSING_IMAGE
                && f.file.as_deref() == Some("problems/bi/statement.zh.md")
                && f.line == Some(3)
                && f.column == Some(4)
        }));
        assert!(findings.iter().any(|f| {
            !f.is_error()
                && f.code == code::MISSING_IO_SECTION
                && f.file.as_deref() == Some("problems/bi/statement.en.md")
        }));
    }

    #[test]
    fn missing_image_catches_symlink_escaping_problem_dir() {
        let (dir, ws) = scratch_ws();
        write_problem(dir.path(), "other", "## Input\nx\n## Output\ny\n", None);
        fs::create_dir_all(dir.path().join("problems/other/assets")).unwrap();
        fs::write(dir.path().join("problems/other/assets/secret.png"), b"png").unwrap();

        write_problem(
            dir.path(),
            "p",
            "![link](assets/link.png)\n\n## Input\nx\n## Output\ny\n",
            None,
        );
        fs::create_dir_all(dir.path().join("problems/p/assets")).unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(
                dir.path().join("problems/other/assets/secret.png"),
                dir.path().join("problems/p/assets/link.png"),
            )
            .unwrap();
            let findings = lint_problem(&ws, "p", &LintOptions::default()).unwrap();
            assert_eq!(findings.len(), 1, "{findings:?}");
            assert_eq!(findings[0].code, code::MISSING_IMAGE);
            assert!(findings[0].is_error());
        }
    }

    #[test]
    fn sample_unpaired_and_non_contiguous_warnings() {
        let (dir, ws) = scratch_ws();
        write_problem(dir.path(), "p", "## Input\nx\n## Output\ny\n", None);
        let s = dir.path().join("problems/p/samples");
        fs::write(s.join("01.in"), "1\n").unwrap();
        fs::write(s.join("01.ans"), "1\n").unwrap();
        fs::write(s.join("03.in"), "3\n").unwrap();
        fs::write(s.join("03.out"), "3\n").unwrap();
        fs::write(s.join("04.in"), "4\n").unwrap();
        fs::write(s.join("05.ans"), "5\n").unwrap();

        let findings = lint_problem(&ws, "p", &LintOptions::default()).unwrap();
        assert_eq!(findings.len(), 3, "{findings:#?}");
        assert!(findings.iter().all(|f| !f.is_error()));

        let unpaired: Vec<_> = findings
            .iter()
            .filter(|f| f.code == code::SAMPLE_UNPAIRED)
            .collect();
        assert_eq!(unpaired.len(), 2);
        assert_eq!(
            unpaired[0].file.as_deref(),
            Some("problems/p/samples/04.in")
        );
        assert_eq!(
            unpaired[1].file.as_deref(),
            Some("problems/p/samples/05.ans")
        );

        let non_contig: Vec<_> = findings
            .iter()
            .filter(|f| f.code == code::SAMPLE_NON_CONTIGUOUS)
            .collect();
        assert_eq!(non_contig.len(), 1);
        assert_eq!(non_contig[0].file.as_deref(), Some("problems/p/samples"));
    }

    #[test]
    fn missing_io_section_and_interaction_exemption() {
        let (dir, ws) = scratch_ws();
        // Missing both input and output -> 2 warnings.
        write_problem(
            dir.path(),
            "both",
            "只有描述。\n\n### 输入\n\n三级标题不算。\n",
            None,
        );
        let findings = lint_problem(&ws, "both", &LintOptions::default()).unwrap();
        assert_eq!(findings.len(), 2, "{findings:?}");
        assert!(
            findings
                .iter()
                .all(|f| !f.is_error() && f.code == code::MISSING_IO_SECTION)
        );

        // Missing output only -> 1 warning.
        write_problem(dir.path(), "no-out", "## 输入格式：\n\nx\n", None);
        let findings = lint_problem(&ws, "no-out", &LintOptions::default()).unwrap();
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].message.contains("output"));

        // H1 heading recognized too.
        write_problem(dir.path(), "h1", "# Input\nx\n# Output\ny\n", None);
        assert!(
            lint_problem(&ws, "h1", &LintOptions::default())
                .unwrap()
                .is_empty()
        );

        // Interactive problem is exempt from missing input/output warnings.
        write_problem(
            dir.path(),
            "interactive",
            "交互题。\n\n## 交互协议\n\n询问。\n",
            None,
        );
        assert!(
            lint_problem(&ws, "interactive", &LintOptions::default())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn missing_editorial_only_when_enabled() {
        let (dir, ws) = scratch_ws();
        write_problem(dir.path(), "p", "## Input\nx\n## Output\ny\n", None);

        let off = lint_problem(&ws, "p", &LintOptions::default()).unwrap();
        assert!(off.is_empty(), "{off:?}");

        let on = lint_problem(
            &ws,
            "p",
            &LintOptions {
                check_editorial: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(on.len(), 1, "{on:?}");
        assert!(!on[0].is_error());
        assert_eq!(on[0].code, code::MISSING_EDITORIAL);
        assert_eq!(on[0].file.as_deref(), Some("problems/p/editorial.md"));
    }

    #[test]
    fn unsaved_overlay_updates_image_and_io_diagnostics_live() {
        let (dir, ws) = scratch_ws();
        write_problem(dir.path(), "p", "## Input\nx\n## Output\ny\n", None);

        // Unsaved introduces a missing image and drops Output section.
        let findings = lint_problem(
            &ws,
            "p",
            &LintOptions {
                unsaved: Some((
                    "problems/p/statement.md",
                    "## Input\n\n![live](assets/live-miss.png)\n",
                )),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(findings.len(), 2, "{findings:#?}");
        assert!(
            findings
                .iter()
                .any(|f| f.is_error() && f.code == code::MISSING_IMAGE && f.line == Some(3))
        );
        assert!(
            findings
                .iter()
                .any(|f| !f.is_error() && f.code == code::MISSING_IO_SECTION)
        );

        // Unsaved new editorial suppresses missing_editorial and checks its images.
        let ed_findings = lint_problem(
            &ws,
            "p",
            &LintOptions {
                check_editorial: true,
                unsaved: Some(("problems/p/editorial.md", "## 做法\n![x](assets/no.png)\n")),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(ed_findings.len(), 1, "{ed_findings:?}");
        assert_eq!(ed_findings[0].code, code::MISSING_IMAGE);
        assert_eq!(
            ed_findings[0].file.as_deref(),
            Some("problems/p/editorial.md")
        );
    }

    #[test]
    fn contest_missing_problem_unfrozen_and_frozen_still_lints_other_problems() {
        let (dir, ws) = scratch_ws();
        write_problem(dir.path(), "a", "## Input\nx\n", None); // missing output
        write_problem(
            dir.path(),
            "c",
            "## Input\n![m](assets/nope.png)\n## Output\ny\n",
            None,
        );

        let cdir = dir.path().join("contests/c1");
        fs::create_dir_all(&cdir).unwrap();
        fs::write(
            cdir.join("contest.toml"),
            "name = \"C1\"\n\
             [[problem]]\nid = \"a\"\nlabel = \"A\"\n\
             [[problem]]\nid = \"ghost\"\nlabel = \"B\"\n\
             [[problem]]\nid = \"c\"\nlabel = \"C\"\n",
        )
        .unwrap();

        let findings = lint_contest(
            &ws,
            "c1",
            &LintOptions {
                check_editorial: true,
                ..Default::default()
            },
        )
        .unwrap();

        assert!(
            findings.iter().any(|f| f.is_error()
                && f.code == code::MISSING_PROBLEM
                && f.message.contains("ghost")),
            "{findings:#?}"
        );
        assert!(
            findings.iter().any(|f| !f.is_error()
                && f.code == code::MISSING_IO_SECTION
                && f.file.as_deref() == Some("problems/a/statement.md")),
            "{findings:#?}"
        );
        assert!(
            findings.iter().any(|f| f.is_error()
                && f.code == code::MISSING_IMAGE
                && f.file.as_deref() == Some("problems/c/statement.md")),
            "{findings:#?}"
        );
        // Both existing problems lack editorial.md.
        assert_eq!(
            findings
                .iter()
                .filter(|f| f.code == code::MISSING_EDITORIAL)
                .count(),
            2
        );

        // Frozen contest checks snapshot/<label>, NOT problems/<id>.
        let fdir = dir.path().join("contests/c2");
        fs::create_dir_all(fdir.join("snapshot/A")).unwrap();
        fs::write(fdir.join("snapshot/A/problem.toml"), "title = \"A\"\n").unwrap();
        fs::write(
            fdir.join("snapshot/A/statement.md"),
            "## Input\nx\n## Output\ny\n",
        )
        .unwrap();
        fs::write(
            fdir.join("contest.toml"),
            "name = \"C2\"\nfrozen = true\n\
             [[problem]]\nid = \"deleted-from-lib\"\nlabel = \"A\"\n\
             [[problem]]\nid = \"a\"\nlabel = \"B\"\n",
        )
        .unwrap();

        let frozen_findings = lint_contest(&ws, "c2", &LintOptions::default()).unwrap();
        // A exists in snapshot (even though deleted from library) -> ok.
        // B exists in library (`problems/a`) but is missing in `snapshot/B` -> missing_problem!
        assert_eq!(frozen_findings.len(), 1, "{frozen_findings:#?}");
        assert!(frozen_findings[0].is_error());
        assert_eq!(frozen_findings[0].code, code::MISSING_PROBLEM);
        assert!(frozen_findings[0].message.contains("snapshot/B"));
    }

    fn write_synthetic_report(
        ws: &Workspace,
        slug: &str,
        package_hash: &str,
        ok: bool,
        verified: bool,
        filename: &str,
    ) {
        let dir = xcpc_judge::build_dir_for(ws, slug);
        fs::create_dir_all(&dir).unwrap();
        let report = xcpc_judge::BuildReport {
            problem_id: slug.to_owned(),
            package_hash: package_hash.to_owned(),
            mode: if verified {
                xcpc_judge::BuildMode::Verify
            } else {
                xcpc_judge::BuildMode::Debug
            },
            verified,
            ok,
            subset: false,
            built_at_epoch_secs: 1_700_000_000,
            compiler: None,
            checker: "wcmp".to_owned(),
            interactive: false,
            runs: 1,
            subtasks: Vec::new(),
            total_score: None,
            compile_failures: Vec::new(),
            dirty_inputs: Vec::new(),
            self_tests: Vec::new(),
            tests: Vec::new(),
            solutions: Vec::new(),
            matrix: Vec::new(),
            gates: Vec::new(),
            safety_margin: xcpc_judge::SafetyMargin::default(),
            diagnostics: Vec::new(),
            repro_commands: Vec::new(),
            next_steps: Vec::new(),
        };
        fs::write(
            dir.join(filename),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn static_package_freshness_diagnostics_for_problem_and_contest() {
        let (dir, ws) = scratch_ws();
        // Pure publishing problem -> NotAPackage -> no findings.
        write_problem(dir.path(), "pure", "## Input\nx\n## Output\ny\n", None);
        assert!(
            lint_problem(&ws, "pure", &LintOptions::default())
                .unwrap()
                .is_empty()
        );

        // Package problem without verified report -> PACKAGE_UNVERIFIED error.
        write_problem(dir.path(), "pkg", "## Input\nx\n## Output\ny\n", None);
        fs::create_dir_all(dir.path().join("problems/pkg/solutions")).unwrap();
        fs::write(
            dir.path().join("problems/pkg/solutions/std.cpp"),
            "int main() { return 0; }\n",
        )
        .unwrap();

        let unverified = lint_problem(&ws, "pkg", &LintOptions::default()).unwrap();
        assert_eq!(unverified.len(), 1, "{unverified:#?}");
        assert!(unverified[0].is_error());
        assert_eq!(unverified[0].code, code::PACKAGE_UNVERIFIED);
        assert_eq!(
            unverified[0].file.as_deref(),
            Some("problems/pkg/problem.toml")
        );
        assert!(unverified[0].message.contains("题目「pkg」"));
        assert!(unverified[0].message.contains("还没检查"));

        // Contest referencing unverified package problem also gets PACKAGE_UNVERIFIED.
        let cdir = dir.path().join("contests/c-pkg");
        fs::create_dir_all(&cdir).unwrap();
        fs::write(
            cdir.join("contest.toml"),
            "name = \"C\"\n[[problem]]\nid = \"pure\"\nlabel = \"A\"\n[[problem]]\nid = \"pkg\"\nlabel = \"B\"\n",
        )
        .unwrap();
        let contest_unverified = lint_contest(&ws, "c-pkg", &LintOptions::default()).unwrap();
        assert_eq!(contest_unverified.len(), 1, "{contest_unverified:#?}");
        assert_eq!(contest_unverified[0].code, code::PACKAGE_UNVERIFIED);
        assert_eq!(
            contest_unverified[0].file.as_deref(),
            Some("problems/pkg/problem.toml")
        );
        assert!(contest_unverified[0].message.contains("题目 B（pkg）"));

        // Latest failed build report for current hash -> PACKAGE_FAILED error.
        let hash1 = xcpc_judge::compute_package_hash(&ws, "pkg").unwrap();
        write_synthetic_report(&ws, "pkg", &hash1, false, false, "report.json");
        let failed = lint_problem(&ws, "pkg", &LintOptions::default()).unwrap();
        assert_eq!(failed.len(), 1, "{failed:#?}");
        assert_eq!(failed[0].code, code::PACKAGE_FAILED);

        // Fresh verified report matching current hash -> clean!
        write_synthetic_report(&ws, "pkg", &hash1, true, true, "verified_report.json");
        write_synthetic_report(&ws, "pkg", &hash1, true, true, "report.json");
        assert!(
            lint_problem(&ws, "pkg", &LintOptions::default())
                .unwrap()
                .is_empty()
        );
        assert!(
            lint_contest(&ws, "c-pkg", &LintOptions::default())
                .unwrap()
                .is_empty()
        );

        // Modifying a package file makes verified report stale -> PACKAGE_STALE error.
        fs::write(
            dir.path().join("problems/pkg/solutions/std.cpp"),
            "int main() { return 1; }\n",
        )
        .unwrap();
        let stale = lint_problem(&ws, "pkg", &LintOptions::default()).unwrap();
        assert_eq!(stale.len(), 1, "{stale:#?}");
        assert!(stale[0].is_error());
        assert_eq!(stale[0].code, code::PACKAGE_STALE);
        assert_eq!(stale[0].file.as_deref(), Some("problems/pkg/problem.toml"));
        assert!(stale[0].message.contains("题目「pkg」"));
        assert!(stale[0].message.contains("重新检查"));
    }

    #[test]
    fn phase_3c_interactive_and_subtask_section_warnings() {
        let (dir, ws) = scratch_ws();

        // 1. Interactive problem missing `## 交互方式` -> non-blocking warning.
        write_problem(
            dir.path(),
            "inter",
            "描述。\n\n## 输入格式\n\n无。\n\n## 输出格式\n\n无。\n",
            None,
        );
        fs::write(
            dir.path().join("problems/inter/interactor.cpp"),
            "int main() { return 0; }\n",
        )
        .unwrap();
        let f1 = lint_problem(&ws, "inter", &LintOptions::default()).unwrap();
        assert!(
            f1.iter()
                .any(|f| !f.is_error() && f.code == code::MISSING_INTERACTION_SECTION),
            "{f1:#?}"
        );

        // Adding `## 交互方式` clears the warning.
        fs::write(
            dir.path().join("problems/inter/statement.md"),
            "描述。\n\n## 交互方式\n\n按行交互。\n",
        )
        .unwrap();
        let f2 = lint_problem(&ws, "inter", &LintOptions::default()).unwrap();
        assert!(
            !f2.iter()
                .any(|f| f.code == code::MISSING_INTERACTION_SECTION),
            "{f2:#?}"
        );

        // 2. OI subtask problem missing `## 子任务` -> non-blocking warning.
        write_problem(
            dir.path(),
            "oi",
            "描述。\n\n## 输入格式\n\n整数。\n\n## 输出格式\n\n答案。\n",
            None,
        );
        fs::write(
            dir.path().join("problems/oi/problem.toml"),
            "title = \"OI\"\n\n[[subtask]]\nid = \"1\"\nscore = 100\n",
        )
        .unwrap();
        let f3 = lint_problem(&ws, "oi", &LintOptions::default()).unwrap();
        assert!(
            f3.iter()
                .any(|f| !f.is_error() && f.code == code::MISSING_SUBTASKS_SECTION),
            "{f3:#?}"
        );

        // Adding `## 子任务` clears the warning.
        fs::write(
            dir.path().join("problems/oi/statement.md"),
            "描述。\n\n## 输入格式\n\n整数。\n\n## 输出格式\n\n答案。\n\n## 子任务\n\n子任务 1：100 分。\n",
        )
        .unwrap();
        let f4 = lint_problem(&ws, "oi", &LintOptions::default()).unwrap();
        assert!(
            !f4.iter().any(|f| f.code == code::MISSING_SUBTASKS_SECTION),
            "{f4:#?}"
        );
    }
}
