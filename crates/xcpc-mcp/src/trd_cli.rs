//! Phase 5 CLI subcommands: `xcpc snippet`, `xcpc bundle`, and `xcpc trd`.

use std::io::{IsTerminal, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, ExitCode, Stdio};

use clap::Subcommand;
use serde_json::json;
use xcpc_convert::publish;
use xcpc_domain::{
    BundleOptions, Patch, SnippetMeta, SnippetVerifyStatus, TrdChapter, TrdMetaPatch,
};
use xcpc_judge::SnippetVerifyOptions;

use crate::{
    Cli, Fatal, engine_with_plugins, fonts, open_workspace, write_explicit_file, write_output,
};

#[derive(Subcommand)]
pub(crate) enum SnippetCommand {
    /// List algorithm snippets in `snippets/` with verification status.
    List {
        /// Filter by tag (repeatable; snippet must contain all specified tags).
        #[arg(long = "tag")]
        tags: Vec<String>,
        /// Filter by verification status (`unverified`, `passed`, `failed`, `stale`).
        #[arg(long)]
        status: Option<String>,
        /// Only show pure formula/reference cards (snippets without `code.hpp`).
        #[arg(long)]
        formula_only: bool,
    },
    /// Show detailed metadata, dependencies, reverse references, printed hash, and verify status.
    Show {
        /// Snippet slug under `snippets/<slug>/`.
        slug: String,
    },
    /// Create a new algorithm snippet under `snippets/<slug>/`.
    New {
        /// Snippet slug (immutable directory name under `snippets/`).
        slug: String,
        /// Display title (defaults to slug).
        #[arg(long)]
        title: Option<String>,
        /// Tag(s) to assign (repeatable).
        #[arg(long = "tag")]
        tags: Vec<String>,
        /// Time complexity string (e.g. "O(n log n)").
        #[arg(long)]
        time: Option<String>,
        /// Space complexity string (e.g. "O(n)").
        #[arg(long)]
        space: Option<String>,
        /// Short description.
        #[arg(long)]
        description: Option<String>,
        /// Create a pure formula/reference card (no `code.hpp`).
        #[arg(long)]
        formula_only: bool,
    },
    /// Delete a snippet directory `snippets/<slug>/`.
    Delete {
        /// Snippet slug to delete.
        slug: String,
        /// Skip interactive confirmation.
        #[arg(long)]
        yes: bool,
    },
    /// Verify one snippet, its downstream dependents (`--cascade`), or all snippets (`--all`).
    Verify {
        /// Snippet slug to verify (omit when using `--all`).
        slug: Option<String>,
        /// Also verify all downstream snippets that depend on `<slug>` in topological order.
        #[arg(long)]
        cascade: bool,
        /// Verify every snippet in the workspace.
        #[arg(long)]
        all: bool,
        /// Maximum parallel jobs (passed through to `xcpc-judge`).
        #[arg(long, short = 'j')]
        jobs: Option<usize>,
        /// Ignore cached verification reports and re-run.
        #[arg(long)]
        force: bool,
    },
}

#[derive(Subcommand)]
pub(crate) enum TrdCommand {
    /// List all Team Reference Documents in `trds/`.
    List,
    /// Show metadata, chapters, preflight issues, and verification statuses for a TRD.
    Show {
        /// TRD ID under `trds/<id>/`.
        id: String,
    },
    /// Create or update a TRD's chapters and metadata.
    Compose {
        /// TRD ID (slug under `trds/`).
        id: String,
        /// Chapter specification in `Title:slug1,slug2` format (repeatable).
        #[arg(long = "chapter")]
        chapters: Vec<String>,
        /// TRD title.
        #[arg(long)]
        name: Option<String>,
        /// Author / team name.
        #[arg(long)]
        author: Option<String>,
        /// Date string.
        #[arg(long)]
        date: Option<String>,
        /// Template name (`trd-compact`, `trd-book`, or custom `templates/<name>.typ`).
        #[arg(long)]
        template: Option<String>,
        /// Optional flat list of snippet slugs (used if `--chapter` is omitted).
        #[arg(value_name = "SLUG")]
        items: Vec<String>,
    },
    /// Validate, export snapshot PDF, and freeze a TRD into `trds/<id>/snapshot/`.
    Freeze {
        /// TRD ID.
        id: String,
        /// Block freezing if any code snippet is unverified, stale, or failed.
        #[arg(long)]
        strict: bool,
    },
    /// Show snapshot diff and unfreeze a TRD (requires confirmation or `--yes`).
    Unfreeze {
        /// TRD ID.
        id: String,
        /// Skip interactive confirmation.
        #[arg(long)]
        yes: bool,
    },
    /// Run TRD preflight (missing deps, cycles, verify status) and Typst compilation check.
    Check {
        /// TRD ID.
        id: String,
    },
    /// Compile a TRD and export its PDF.
    Build {
        /// TRD ID.
        id: String,
        /// Output PDF path (default: `build/<name>.pdf` in workspace).
        #[arg(long = "output", alias = "out", short = 'o')]
        out: Option<PathBuf>,
        /// Also write the generated Typst source to this path.
        #[arg(long)]
        emit_typst: Option<PathBuf>,
    },
    /// Compile a TRD and render page(s) to PNG.
    Render {
        /// TRD ID.
        id: String,
        /// Output directory (default: `build/<id>/` in workspace).
        #[arg(long, short)]
        out: Option<PathBuf>,
        /// Pixels per point (default 2.0 ≈ 144 dpi).
        #[arg(long, default_value_t = 2.0)]
        scale: f32,
        /// Render only this 1-based page.
        #[arg(long)]
        page: Option<usize>,
    },
}

pub(crate) fn run_snippet(cli: &Cli, action: &SnippetCommand) -> Result<ExitCode, Fatal> {
    let ws = open_workspace(cli)?;
    match action {
        SnippetCommand::List {
            tags,
            status,
            formula_only,
        } => {
            let summaries = ws.list_snippet_summaries()?;
            let statuses = xcpc_judge::all_snippet_verify_statuses(&ws).unwrap_or_default();
            let want_status = match status.as_deref() {
                None => None,
                Some(s) => Some(parse_verify_status(s)?),
            };
            let filtered: Vec<_> = summaries
                .into_iter()
                .filter(|s| {
                    if *formula_only && s.has_code {
                        return false;
                    }
                    if !tags.iter().all(|t| s.tags.iter().any(|st| st == t)) {
                        return false;
                    }
                    let st = statuses
                        .get(&s.slug)
                        .copied()
                        .unwrap_or(SnippetVerifyStatus::Unverified);
                    if let Some(want) = want_status
                        && st != want
                    {
                        return false;
                    }
                    true
                })
                .map(|s| {
                    let st = statuses
                        .get(&s.slug)
                        .copied()
                        .unwrap_or(SnippetVerifyStatus::Unverified);
                    (s, st)
                })
                .collect();

            if cli.json {
                let items: Vec<_> = filtered
                    .iter()
                    .map(|(s, st)| {
                        let mut val = serde_json::to_value(s).expect("serializable");
                        if let Some(obj) = val.as_object_mut() {
                            obj.insert("verify_status".into(), json!(st.as_str()));
                        }
                        val
                    })
                    .collect();
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({ "ok": true, "snippets": items }))
                        .expect("serializable")
                );
            } else if filtered.is_empty() {
                println!("(no snippets)");
            } else {
                for (s, st) in &filtered {
                    let kind = if s.has_code {
                        format!("{}L", s.code_lines)
                    } else {
                        "formula".into()
                    };
                    let hash = s.code_hash.as_deref().unwrap_or("-");
                    println!(
                        "{:<24} {:<10} {:<8} {:<7} {}",
                        s.slug,
                        st.as_str(),
                        kind,
                        hash,
                        s.title
                    );
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        SnippetCommand::Show { slug } => {
            let detail = ws.load_snippet(slug)?;
            let status = xcpc_judge::snippet_verify_status(&ws, slug);
            let report = xcpc_judge::load_snippet_verify_report(&ws, slug);
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "ok": true,
                        "snippet": detail,
                        "verify_status": status.as_str(),
                        "verify_report": report,
                    }))
                    .expect("serializable")
                );
            } else {
                println!("{} — {}", detail.slug, detail.meta.title);
                if !detail.meta.tags.is_empty() {
                    println!("tags: {}", detail.meta.tags.join(", "));
                }
                if let Some(tc) = &detail.meta.time_complexity {
                    println!("time: {tc}");
                }
                if let Some(sc) = &detail.meta.space_complexity {
                    println!("space: {sc}");
                }
                println!("verify: {}", status.as_str());
                if !detail.reverse_refs.deps.is_empty() {
                    println!("deps: {}", detail.reverse_refs.deps.join(", "));
                }
                if !detail.reverse_refs.reverse_deps.is_empty() {
                    println!(
                        "used by snippets: {}",
                        detail.reverse_refs.reverse_deps.join(", ")
                    );
                }
                if !detail.reverse_refs.related_problems.is_empty() {
                    println!(
                        "related problems: {}",
                        detail.reverse_refs.related_problems.join(", ")
                    );
                }
                if let Some(p) = &detail.printed_code {
                    println!("code: {} lines, hash {}", p.lines.len(), p.total_hash);
                } else {
                    println!("code: none (formula card)");
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        SnippetCommand::New {
            slug,
            title,
            tags,
            time,
            space,
            description,
            formula_only,
        } => {
            let meta = SnippetMeta {
                title: title.clone().unwrap_or_else(|| slug.clone()),
                tags: tags.clone(),
                time_complexity: time.clone(),
                space_complexity: space.clone(),
                description: description.clone(),
                source_problem: None,
                verify: Vec::new(),
            };
            let starter_code = if *formula_only {
                None
            } else {
                Some("#pragma once\n")
            };
            let starter_notes = Some("");
            let detail = ws.create_snippet(slug, &meta, starter_code, starter_notes)?;
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "ok": true,
                        "snippet": detail,
                    }))
                    .expect("serializable")
                );
            } else {
                println!("created snippet `{}` at {}/", detail.slug, detail.dir);
            }
            Ok(ExitCode::SUCCESS)
        }
        SnippetCommand::Delete { slug, yes } => {
            if !*yes {
                confirm_or_fail(slug, "snippet")?;
            }
            ws.delete_snippet(slug)?;
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "ok": true,
                        "deleted": slug,
                    }))
                    .expect("serializable")
                );
            } else {
                println!("deleted snippet `{slug}`");
            }
            Ok(ExitCode::SUCCESS)
        }
        SnippetCommand::Verify {
            slug,
            cascade,
            all,
            jobs,
            force,
        } => {
            let opts = SnippetVerifyOptions {
                jobs: *jobs,
                force: *force,
                ..Default::default()
            };
            let reports = if *all {
                xcpc_judge::verify_all_snippets(&ws, &opts).map_err(judge_fatal)?
            } else if let Some(s) = slug {
                if *cascade {
                    xcpc_judge::verify_snippet_cascade(&ws, s, &opts).map_err(judge_fatal)?
                } else {
                    vec![xcpc_judge::verify_snippet(&ws, s, &opts).map_err(judge_fatal)?]
                }
            } else {
                return Err(Fatal {
                    code: "invalid_arguments",
                    message: "provide a snippet `<slug>` or pass `--all`".into(),
                    human: Some("请指定算法条目 `<slug>`，或使用 `--all` 验证全库。".into()),
                    exit_code: 2,
                });
            };

            let all_ok = reports
                .iter()
                .all(|r| r.status != SnippetVerifyStatus::Failed);

            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "ok": all_ok,
                        "reports": reports,
                    }))
                    .expect("serializable")
                );
            } else {
                for r in &reports {
                    println!(
                        "{:<24} {:<10} {}ms  ({} cases)",
                        r.slug,
                        r.status.as_str(),
                        r.total_time_ms,
                        r.cases.len()
                    );
                    for c in &r.cases {
                        if !c.passed {
                            eprintln!(
                                "  [FAIL] {} {}: {} ({})",
                                c.kind,
                                c.target,
                                c.verdict,
                                c.message.as_deref().unwrap_or("")
                            );
                        }
                    }
                }
            }
            Ok(if all_ok {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            })
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_bundle(
    cli: &Cli,
    file: Option<&Path>,
    snippet: Option<&str>,
    include_dirs: &[PathBuf],
    copy: bool,
    out: Option<&Path>,
    compact: bool,
) -> Result<ExitCode, Fatal> {
    if file.is_some() == snippet.is_some() {
        return Err(Fatal {
            code: "invalid_arguments",
            message: "specify either `<file.cpp>` or `--snippet <slug>` (not both)".into(),
            human: Some("请提供 `<file.cpp>` 或 `--snippet <slug>`（二选一）。".into()),
            exit_code: 2,
        });
    }

    let ws = open_workspace(cli)?;
    let opts = BundleOptions {
        include_dirs: include_dirs
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect(),
        compact,
        include_boundary_comments: !compact,
    };

    let result = if let Some(slug) = snippet {
        ws.bundle_snippet(slug, &opts)?
    } else if let Some(path) = file {
        ws.bundle_local_file(path, &opts)?
    } else {
        unreachable!()
    };

    let mut written_to: Option<String> = None;
    if let Some(out_path) = out {
        if let Some(parent) = out_path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|e| Fatal {
                code: "io",
                message: format!("failed to create directory {}: {e}", parent.display()),
                human: None,
                exit_code: 2,
            })?;
        }
        std::fs::write(out_path, result.code.as_bytes()).map_err(|e| Fatal {
            code: "io",
            message: format!("failed to write {}: {e}", out_path.display()),
            human: None,
            exit_code: 2,
        })?;
        written_to = Some(out_path.display().to_string());
    }

    let mut copied = false;
    if copy {
        copy_to_clipboard(&result.code)?;
        copied = true;
    }

    if cli.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "ok": true,
                "code": result.code,
                "system_includes": result.system_includes,
                "inlined_files": result.inlined_files,
                "inlined_snippets": result.inlined_snippets,
                "edges": result.edges,
                "written_to": written_to,
                "copied": copied,
            }))
            .expect("serializable")
        );
    } else if out.is_none() && !copy {
        print!("{}", result.code);
    } else {
        if let Some(path) = &written_to {
            eprintln!("bundled {} file(s) -> {path}", result.inlined_files.len());
        }
        if copied {
            eprintln!("copied bundled code to clipboard");
        }
    }
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn run_trd(cli: &Cli, action: &TrdCommand) -> Result<ExitCode, Fatal> {
    let ws = open_workspace(cli)?;
    match action {
        TrdCommand::List => {
            let list = ws.list_trd_summaries()?;
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({ "ok": true, "trds": list }))
                        .expect("serializable")
                );
            } else if list.is_empty() {
                println!("(no trds)");
            } else {
                for t in &list {
                    let state = if t.frozen { "frozen" } else { "live" };
                    let tpl = t.template.as_deref().unwrap_or("trd-compact");
                    println!(
                        "{:<24} {:<8} {:<14} {}ch/{}items  {}",
                        t.id, state, tpl, t.chapter_count, t.item_count, t.name
                    );
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        TrdCommand::Show { id } => {
            let trd = ws.load_trd(id)?;
            let statuses = xcpc_judge::all_snippet_verify_statuses(&ws).unwrap_or_default();
            let preflight = if trd.meta.frozen {
                Vec::new()
            } else {
                ws.preflight_trd(id, &statuses)?
            };
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "ok": true,
                        "trd": trd,
                        "preflight": preflight,
                    }))
                    .expect("serializable")
                );
            } else {
                println!("{} — {}", trd.id, trd.meta.name);
                println!(
                    "template: {}, frozen: {}, archived: {}",
                    trd.meta.template.as_deref().unwrap_or("trd-compact"),
                    trd.meta.frozen,
                    trd.meta.archived
                );
                for ch in &trd.meta.chapters {
                    println!("  [{}] {}", ch.title, ch.items.join(", "));
                }
                for issue in &preflight {
                    eprintln!("  {:?}: {}", issue.severity, issue.message);
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        TrdCommand::Compose {
            id,
            chapters,
            name,
            author,
            date,
            template,
            items,
        } => {
            let parsed_chapters = parse_cli_chapters(chapters, items)?;
            let patch = TrdMetaPatch {
                name: name.clone(),
                author: author.clone().map_or(Patch::Keep, Patch::Set),
                date: date.clone().map_or(Patch::Keep, Patch::Set),
                template: template.clone().map_or(Patch::Keep, Patch::Set),
                ..Default::default()
            };
            let detail = ws.compose_trd(id, &parsed_chapters, Some(&patch))?;
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({ "ok": true, "trd": detail }))
                        .expect("serializable")
                );
            } else {
                println!(
                    "composed trd `{}` ({} chapters)",
                    detail.id,
                    detail.meta.chapters.len()
                );
            }
            Ok(ExitCode::SUCCESS)
        }
        TrdCommand::Freeze { id, strict } => {
            let mut engine = engine_with_plugins(&ws, &fonts(cli));
            let statuses = xcpc_judge::all_snippet_verify_statuses(&ws).unwrap_or_default();
            let (result, report) =
                publish::freeze_trd_with_pdf(&ws, &mut engine, id, *strict, &statuses)?;
            let frozen = result.is_some();
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "ok": frozen && report.ok,
                        "frozen": frozen,
                        "result": result,
                        "report": report,
                    }))
                    .expect("serializable")
                );
            } else if let Some(res) = &result {
                println!(
                    "froze trd `{}` ({} snippets) -> {}",
                    res.id, res.snippet_count, res.snapshot_dir
                );
                for w in &res.warnings {
                    eprintln!("warning: {}", w.message);
                }
            } else {
                for d in &report.diagnostics {
                    eprintln!("{}", d.display());
                }
            }
            Ok(if frozen && report.ok {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            })
        }
        TrdCommand::Unfreeze { id, yes } => {
            let diff = ws.diff_trd_for_unfreeze(id)?;
            if !*yes {
                if !cli.json && diff.has_changes {
                    eprintln!("Changes between snapshot and live snippets:");
                    for d in &diff.diffs {
                        eprintln!("  - {}: {:?}", d.slug, d.kinds);
                    }
                }
                confirm_or_fail(id, "trd")?;
            }
            let detail = ws.unfreeze_trd(id)?;
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "ok": true,
                        "trd": detail,
                        "diff": diff,
                    }))
                    .expect("serializable")
                );
            } else {
                println!("unfroze trd `{}`", detail.id);
            }
            Ok(ExitCode::SUCCESS)
        }
        TrdCommand::Check { id } => {
            let mut engine = engine_with_plugins(&ws, &fonts(cli));
            let statuses = xcpc_judge::all_snippet_verify_statuses(&ws).unwrap_or_default();
            let (_, _, report) = publish::check_trd(&ws, &mut engine, id, &statuses)?;
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report).expect("serializable")
                );
            } else {
                for d in &report.diagnostics {
                    eprintln!("{}", d.display());
                }
                if report.ok {
                    println!(
                        "ok: {} page(s) ({} ms)",
                        report.pages.unwrap_or(0),
                        report.compile_ms
                    );
                }
            }
            Ok(if report.ok {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            })
        }
        TrdCommand::Build {
            id,
            out,
            emit_typst,
        } => {
            let mut engine = engine_with_plugins(&ws, &fonts(cli));
            let (pdf_bytes, doc, report) = publish::compile_trd_pdf_bytes(&ws, &mut engine, id)?;
            let mut outputs = Vec::new();
            if let Some(typst_path) = emit_typst {
                let written = write_explicit_file(typst_path, doc.source.as_bytes())?;
                outputs.push(written);
            }
            if let Some(bytes) = pdf_bytes {
                let default_rel = publish::default_trd_output(&ws, id);
                let written = write_output(&ws, out.as_ref(), &default_rel, &bytes)?;
                outputs.push(written);
            }
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "ok": report.ok,
                        "pages": report.pages,
                        "problems": report.problems,
                        "diagnostics": report.diagnostics,
                        "compile_ms": report.compile_ms,
                        "outputs": outputs,
                    }))
                    .expect("serializable")
                );
            } else {
                for d in &report.diagnostics {
                    eprintln!("{}", d.display());
                }
                for o in &outputs {
                    println!("wrote {o}");
                }
            }
            Ok(if report.ok {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            })
        }
        TrdCommand::Render {
            id,
            out,
            scale,
            page,
        } => {
            let mut engine = engine_with_plugins(&ws, &fonts(cli));
            let (compiled, _doc, report) = publish::compile_trd(&ws, &mut engine, id)?;
            let mut outputs = Vec::new();
            if let Some(compiled) = compiled {
                let total = compiled.page_count();
                let pages_to_render: Vec<usize> = match page {
                    Some(p) => {
                        if *p == 0 || *p > total {
                            return Err(Fatal {
                                code: "no_such_page",
                                message: format!("page {p} out of range (1..={total})"),
                                human: Some(format!("页码 {p} 超出范围（共 {total} 页）。")),
                                exit_code: 2,
                            });
                        }
                        vec![*p]
                    }
                    None => (1..=total).collect(),
                };
                for p in pages_to_render {
                    if let Some(png) = compiled.render_png(p - 1, *scale) {
                        let written = match out {
                            Some(dir) => {
                                write_explicit_file(&dir.join(format!("page-{p}.png")), &png)?
                            }
                            None => {
                                let rel = format!("build/{id}/page-{p}.png");
                                write_output(&ws, None, &rel, &png)?
                            }
                        };
                        outputs.push(written);
                    }
                }
            }
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "ok": report.ok,
                        "pages": report.pages,
                        "diagnostics": report.diagnostics,
                        "compile_ms": report.compile_ms,
                        "outputs": outputs,
                    }))
                    .expect("serializable")
                );
            } else {
                for d in &report.diagnostics {
                    eprintln!("{}", d.display());
                }
                for o in &outputs {
                    println!("wrote {o}");
                }
            }
            Ok(if report.ok {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            })
        }
    }
}

fn judge_fatal(e: xcpc_judge::JudgeError) -> Fatal {
    Fatal {
        code: e.code(),
        message: e.to_string(),
        human: None,
        exit_code: 2,
    }
}

fn parse_verify_status(s: &str) -> Result<SnippetVerifyStatus, Fatal> {
    match s.trim().to_ascii_lowercase().as_str() {
        "unverified" => Ok(SnippetVerifyStatus::Unverified),
        "passed" => Ok(SnippetVerifyStatus::Passed),
        "failed" => Ok(SnippetVerifyStatus::Failed),
        "stale" => Ok(SnippetVerifyStatus::Stale),
        other => Err(Fatal {
            code: "invalid_arguments",
            message: format!(
                "unknown verify status {other:?} (expected unverified|passed|failed|stale)"
            ),
            human: None,
            exit_code: 2,
        }),
    }
}

fn parse_cli_chapters(
    chapter_specs: &[String],
    flat_items: &[String],
) -> Result<Vec<TrdChapter>, Fatal> {
    if !chapter_specs.is_empty() {
        let mut out = Vec::with_capacity(chapter_specs.len());
        for spec in chapter_specs {
            let (title, raw_items) = spec
                .split_once(':')
                .or_else(|| spec.split_once('='))
                .ok_or_else(|| Fatal {
                    code: "invalid_arguments",
                    message: format!("invalid --chapter {spec:?}; expected `Title:slug1,slug2`"),
                    human: Some(format!(
                        "`--chapter` 格式应为 `章节名:slug1,slug2`，实际收到「{spec}」。"
                    )),
                    exit_code: 2,
                })?;
            let title = title.trim().to_owned();
            if title.is_empty() {
                return Err(Fatal {
                    code: "invalid_arguments",
                    message: "chapter title cannot be empty".into(),
                    human: None,
                    exit_code: 2,
                });
            }
            let items: Vec<String> = raw_items
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect();
            out.push(TrdChapter { title, items });
        }
        return Ok(out);
    }
    if !flat_items.is_empty() {
        return Ok(vec![TrdChapter {
            title: "Algorithms".into(),
            items: flat_items.to_vec(),
        }]);
    }
    Ok(Vec::new())
}

fn confirm_or_fail(id: &str, kind: &str) -> Result<(), Fatal> {
    if !std::io::stdin().is_terminal() {
        return Err(Fatal {
            code: "unconfirmed",
            message: format!("pass `--yes` to confirm {kind} `{id}` in non-interactive mode"),
            human: Some(format!(
                "非交互模式下请使用 `--yes` 确认操作 {kind}「{id}」。"
            )),
            exit_code: 2,
        });
    }
    eprint!("Type `{id}` to confirm: ");
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).map_err(|e| Fatal {
        code: "io",
        message: e.to_string(),
        human: None,
        exit_code: 2,
    })?;
    if line.trim() != id {
        return Err(Fatal {
            code: "unconfirmed",
            message: "confirmation did not match".into(),
            human: Some("输入与目标 ID 不匹配，已取消操作。".into()),
            exit_code: 2,
        });
    }
    Ok(())
}

fn copy_to_clipboard(text: &str) -> Result<(), Fatal> {
    let candidates: &[(&str, &[&str])] = &[
        ("wl-copy", &[]),
        ("xclip", &["-selection", "clipboard"]),
        ("xsel", &["--clipboard", "--input"]),
        ("pbcopy", &[]),
    ];
    for &(cmd, args) in candidates {
        let Ok(mut child) = ProcessCommand::new(cmd)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            continue;
        };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        if let Ok(status) = child.wait()
            && status.success()
        {
            return Ok(());
        }
    }
    Err(Fatal {
        code: "clipboard_unavailable",
        message: "no working clipboard utility found (tried wl-copy, xclip, xsel, pbcopy); use `-o <file>` or redirect stdout instead".into(),
        human: Some("未找到可用的系统剪贴板工具（wl-copy / xclip / xsel / pbcopy），请改用 `-o <文件>` 或重定向标准输出。".into()),
        exit_code: 2,
    })
}
