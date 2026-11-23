//! Minimal-set OJ package exporter for DOMjudge (`Polygon2DOMjudge`-aligned) and Hydro OJ.
//!
//! Enforces strict static/dynamic separation ("动静分离"): this module NEVER compiles or
//! executes C++ code. It checks the latest strict verification report from `xcpc-judge`
//! and packages pre-built testdata from `build/problems/<slug>/tests/`.

use std::fs;
use std::path::{Path, PathBuf};

use xcpc_convert::publish;
use xcpc_convert::{Book, Entry};
use xcpc_domain::{ContestItem, Language, Problem, ProblemMeta};
use xcpc_engine::{Engine, FontOptions};
use xcpc_judge::{
    CheckerSpec, PackageFreshness, RunLimits, TESTLIB_H, build_dir_for, check_package_freshness,
    load_latest_report, load_verified_report, resolve_checker_spec, standard_checker_source,
};
use xcpc_storage::Workspace;

use crate::error::{PackageError, Result, io_err};
use crate::types::{
    ExportOjOptions, ExportOjResult, ExportTarget, ExportedArtifact, OjFormat, PackageWarning,
};
use crate::zip_util::AtomicZipWriter;

/// Exports one problem or all problems in a contest to DOMjudge or Hydro ZIP package(s).
///
/// Never compiles or runs C++ code. Refuses unless the problem's latest strict verification
/// (`verified == true`) matches the current package content hash, unless `opts.force == true`
/// (which still requires built testdata under `build/problems/<slug>/tests/`).
pub fn export_oj_package(
    ws: &Workspace,
    target: &ExportTarget,
    opts: &ExportOjOptions,
) -> Result<ExportOjResult> {
    let mut warnings = Vec::new();

    match target {
        ExportTarget::Problem(slug) => {
            let problem = ws.load_problem(slug)?;
            let prepared = prepare_export_item(ws, &problem, None, None, opts, &mut warnings)?;
            let zip_path = if opts.output_path.is_dir() {
                opts.output_path.join(format!("{}.zip", problem.id))
            } else {
                opts.output_path.clone()
            };
            let mut engine = (opts.format == OjFormat::Domjudge)
                .then(|| Engine::new(&FontOptions::from_env(), Some(ws.root().to_path_buf())));
            let artifact = write_oj_zip(ws, &prepared, &zip_path, opts, engine.as_mut())?;
            Ok(ExportOjResult {
                format: opts.format,
                artifacts: vec![artifact],
                warnings,
            })
        }
        ExportTarget::Contest(contest_id) => {
            let contest = ws.load_contest(contest_id)?;
            let items = ws.contest_problems(&contest)?;
            if items.is_empty() {
                return Err(PackageError::InvalidFormat(format!(
                    "contest {contest_id:?} has no problems to export"
                )));
            }

            // Pre-validate ALL problems before writing any ZIPs so a failing gate never leaves
            // partial contest output on disk.
            let mut prepared_list = Vec::with_capacity(items.len());
            for (item, problem) in &items {
                let effective_meta = item.apply(&problem.meta);
                let lang = opts.language.unwrap_or(contest.meta.language);
                let prepared = prepare_export_item(
                    ws,
                    problem,
                    Some(item.clone()),
                    Some((effective_meta, lang)),
                    opts,
                    &mut warnings,
                )?;
                prepared_list.push(prepared);
            }

            fs::create_dir_all(&opts.output_path).map_err(|e| {
                io_err(
                    format!("create output directory {}", opts.output_path.display()),
                    e,
                )
            })?;

            let mut engine = (opts.format == OjFormat::Domjudge)
                .then(|| Engine::new(&FontOptions::from_env(), Some(ws.root().to_path_buf())));
            let mut artifacts = Vec::with_capacity(prepared_list.len());
            for prepared in &prepared_list {
                let label = prepared.label.as_deref().unwrap_or("P");
                let zip_path = opts
                    .output_path
                    .join(format!("{label}-{}.zip", prepared.problem.id));
                let artifact = write_oj_zip(ws, prepared, &zip_path, opts, engine.as_mut())?;
                artifacts.push(artifact);
            }

            Ok(ExportOjResult {
                format: opts.format,
                artifacts,
                warnings,
            })
        }
    }
}

struct PreparedTest {
    is_sample: bool,
    input_bytes: Vec<u8>,
    answer_bytes: Vec<u8>,
}

struct PreparedExport {
    problem: Problem,
    effective_meta: ProblemMeta,
    contest_item: Option<ContestItem>,
    label: Option<String>,
    language: Language,
    prob_dir: PathBuf,
    checker_spec: CheckerSpec,
    limits: RunLimits,
    tests: Vec<PreparedTest>,
    verified: bool,
    forced: bool,
}

fn prepare_export_item(
    ws: &Workspace,
    problem: &Problem,
    contest_item: Option<ContestItem>,
    override_meta_lang: Option<(ProblemMeta, Language)>,
    opts: &ExportOjOptions,
    warnings: &mut Vec<PackageWarning>,
) -> Result<PreparedExport> {
    let slug = &problem.id;
    let prob_dir = ws.resolve(&problem.dir)?;

    let (effective_meta, language) = match override_meta_lang {
        Some((m, l)) => (m, l),
        None => (
            problem.meta.clone(),
            opts.language.unwrap_or_else(|| problem.default_language()),
        ),
    };

    // 1. Strict gate check (pure static, no execution).
    let freshness = check_package_freshness(ws, slug)?;
    let (verified, forced) = match freshness {
        PackageFreshness::VerifiedFresh { .. } => (true, false),
        other => {
            let reason = match other {
                PackageFreshness::NotAPackage => "problem has no local Polygon package".to_owned(),
                PackageFreshness::Stale { .. } => {
                    "package files changed since last strict verification".to_owned()
                }
                PackageFreshness::Unverified { .. } => {
                    "no strict verification report found".to_owned()
                }
                PackageFreshness::Failed { summary, .. } => {
                    format!("latest build failed: {summary}")
                }
                PackageFreshness::VerifiedFresh { .. } => unreachable!(),
            };
            if !opts.force {
                return Err(PackageError::NotVerified {
                    slug: slug.clone(),
                    reason,
                });
            }
            warnings.push(PackageWarning::with_path(
                "forced_unverified_export",
                format!("exported unverified package `{slug}` with --force ({reason})"),
                problem.dir.clone(),
            ));
            (false, true)
        }
    };

    // 2. Load built testdata from `build/problems/<slug>/tests/` (required even with `force`).
    let report = if verified {
        load_verified_report(ws, slug).or_else(|| load_latest_report(ws, slug))
    } else {
        load_latest_report(ws, slug).or_else(|| load_verified_report(ws, slug))
    }
    .ok_or_else(|| PackageError::MissingTestdata {
        slug: slug.clone(),
        detail: "build report not found under build/problems/<slug>/".to_owned(),
    })?;

    if report.tests.is_empty() {
        return Err(PackageError::MissingTestdata {
            slug: slug.clone(),
            detail: "build report contains 0 tests".to_owned(),
        });
    }

    let tests_dir = build_dir_for(ws, slug).join("tests");
    let mut tests = Vec::with_capacity(report.tests.len());
    for t in &report.tests {
        let in_path = tests_dir.join(format!("{:03}.in", t.index));
        let ans_path = tests_dir.join(format!("{:03}.ans", t.index));
        let input_bytes = fs::read(&in_path).map_err(|_| PackageError::MissingTestdata {
            slug: slug.clone(),
            detail: format!("missing built input {}", in_path.display()),
        })?;
        let answer_bytes = fs::read(&ans_path).map_err(|_| PackageError::MissingTestdata {
            slug: slug.clone(),
            detail: format!("missing built answer {}", ans_path.display()),
        })?;
        tests.push(PreparedTest {
            is_sample: t.is_sample,
            input_bytes,
            answer_bytes,
        });
    }

    let checker_spec = resolve_checker_spec(&prob_dir, &effective_meta)?;
    let limits = RunLimits::from_problem_limits(
        effective_meta.limits.time.as_deref(),
        effective_meta.limits.memory.as_deref(),
    )?;
    let label = contest_item.as_ref().map(|c| c.label.clone());

    Ok(PreparedExport {
        problem: problem.clone(),
        effective_meta,
        contest_item,
        label,
        language,
        prob_dir,
        checker_spec,
        limits,
        tests,
        verified,
        forced,
    })
}

fn write_oj_zip(
    ws: &Workspace,
    prep: &PreparedExport,
    zip_path: &Path,
    opts: &ExportOjOptions,
    engine: Option<&mut Engine>,
) -> Result<ExportedArtifact> {
    let sample_count = prep.tests.iter().filter(|t| t.is_sample).count();
    let secret_count = prep.tests.len() - sample_count;

    let size_bytes = match opts.format {
        OjFormat::Domjudge => write_domjudge_zip(ws, prep, zip_path, engine.expect("engine"))?,
        OjFormat::Hydro => write_hydro_zip(prep, zip_path)?,
    };

    Ok(ExportedArtifact {
        problem_id: prep.problem.id.clone(),
        label: prep.label.clone(),
        zip_path: zip_path.display().to_string(),
        sample_count,
        secret_count,
        size_bytes,
        verified: prep.verified,
        forced: prep.forced,
    })
}

// ---------------------------------------------------------------------------
// DOMjudge Export (`cn-xcpc-tools/Polygon2DOMjudge` aligned)
// ---------------------------------------------------------------------------

const DOMJUDGE_CHECKER_BUILD_SH: &str = "\
#!/bin/sh
set -e
DIR=\"$(cd \"$(dirname \"$0\")\" && pwd)\"
g++ -O2 -std=c++20 \"$DIR/checker.cpp\" -o \"$DIR/checker_bin\"
";

const DOMJUDGE_CHECKER_RUN_SH: &str = "\
#!/bin/sh
DIR=\"$(cd \"$(dirname \"$0\")\" && pwd)\"
TEST_IN=\"$1\"
TEST_ANS=\"$2\"
FEEDBACK_DIR=\"$3\"
TEAM_OUT=\"$FEEDBACK_DIR/team_out.tmp\"
cat > \"$TEAM_OUT\"
set +e
\"$DIR/checker_bin\" \"$TEST_IN\" \"$TEAM_OUT\" \"$TEST_ANS\" 2> \"$FEEDBACK_DIR/judgemessage.txt\"
RC=$?
rm -f \"$TEAM_OUT\"
if [ \"$RC\" -eq 0 ]; then
  exit 42
elif [ \"$RC\" -eq 1 ] || [ \"$RC\" -eq 2 ]; then
  exit 43
else
  exit \"$RC\"
fi
";

fn write_domjudge_zip(
    ws: &Workspace,
    prep: &PreparedExport,
    zip_path: &Path,
    engine: &mut Engine,
) -> Result<u64> {
    let mut zip = AtomicZipWriter::create(zip_path)?;

    // 1. `domjudge-problem.ini` (LF, NO `color`)
    let short_name = prep.label.as_deref().unwrap_or(&prep.problem.id);
    let timelimit = format_domjudge_timelimit(prep.limits.time_limit_ms);
    let ini = format!(
        "short-name = {short_name}\nexternalid = {}\ntimelimit = {timelimit}\n",
        prep.problem.id
    );
    zip.add_file("domjudge-problem.ini", ini.as_bytes())?;

    // 2. `problem.yaml`
    let memory_mib = prep.limits.memory_limit_kib.div_ceil(1024).max(1);
    let (validation, validator_flags) = domjudge_validation_config(&prep.checker_spec);
    let mut yaml = String::new();
    yaml.push_str(&format!(
        "name: {}\nlimits:\n  memory: {memory_mib}\nvalidation: {validation}\n",
        yaml_scalar(&prep.effective_meta.title)
    ));
    if let Some(flags) = validator_flags {
        yaml.push_str(&format!("validator_flags: {flags}\n"));
    }
    zip.add_file("problem.yaml", yaml.as_bytes())?;

    // 3. `problem.pdf` compiled via `xcpc-convert` + `xcpc-engine`
    let pdf_bytes = compile_problem_pdf(ws, engine, prep)?;
    zip.add_file("problem.pdf", &pdf_bytes)?;

    // 4. `data/sample/` and `data/secret/` (adaptive zero-padding preserves lexicographical order >= 100)
    let sample_total = prep.tests.iter().filter(|t| t.is_sample).count();
    let secret_total = prep.tests.len() - sample_total;
    let sample_width = sample_total.to_string().len().max(2);
    let secret_width = secret_total.to_string().len().max(2);
    let mut sample_idx = 0usize;
    let mut secret_idx = 0usize;
    for t in &prep.tests {
        if t.is_sample {
            sample_idx += 1;
            zip.add_file(
                &format!("data/sample/{sample_idx:0sample_width$}.in"),
                &t.input_bytes,
            )?;
            zip.add_file(
                &format!("data/sample/{sample_idx:0sample_width$}.ans"),
                &t.answer_bytes,
            )?;
        } else {
            secret_idx += 1;
            zip.add_file(
                &format!("data/secret/{secret_idx:0secret_width$}.in"),
                &t.input_bytes,
            )?;
            zip.add_file(
                &format!("data/secret/{secret_idx:0secret_width$}.ans"),
                &t.answer_bytes,
            )?;
        }
    }

    // 5. `output_validators/checker/` (ONLY when `validation: custom`)
    if let CheckerSpec::Custom { ref path, .. } = prep.checker_spec {
        let checker_bytes = fs::read(path)
            .map_err(|e| io_err(format!("read custom checker {}", path.display()), e))?;
        let testlib_bytes = read_effective_testlib(ws, &prep.prob_dir);

        zip.add_file("output_validators/checker/checker.cpp", &checker_bytes)?;
        zip.add_file("output_validators/checker/testlib.h", &testlib_bytes)?;
        zip.add_file_with_mode(
            "output_validators/checker/build",
            DOMJUDGE_CHECKER_BUILD_SH.as_bytes(),
            0o755,
        )?;
        zip.add_file_with_mode(
            "output_validators/checker/run",
            DOMJUDGE_CHECKER_RUN_SH.as_bytes(),
            0o755,
        )?;
    }

    zip.finish()
}

fn domjudge_validation_config(spec: &CheckerSpec) -> (&'static str, Option<&'static str>) {
    match spec {
        CheckerSpec::Builtin(name) => match name.as_str() {
            "wcmp" | "ncmp" | "lcmp" => ("default", Some("case_sensitive")),
            "fcmp" => ("default", Some("space_change_sensitive case_sensitive")),
            "rcmp4" => ("default", Some("float_tolerance 1e-4")),
            "rcmp6" => ("default", Some("float_tolerance 1e-6")),
            "rcmp9" => ("default", Some("float_tolerance 1e-9")),
            "yesno" | "nyesno" => ("default", None),
            _ => ("default", Some("case_sensitive")),
        },
        CheckerSpec::Custom { .. } => ("custom", None),
    }
}

fn format_domjudge_timelimit(ms: u64) -> String {
    let secs = ms as f64 / 1000.0;
    if ms.is_multiple_of(1000) {
        format!("{}.0", ms / 1000)
    } else {
        let s = format!("{secs:.3}");
        let trimmed = s.trim_end_matches('0');
        if trimmed.ends_with('.') {
            format!("{trimmed}0")
        } else {
            trimmed.to_owned()
        }
    }
}

fn compile_problem_pdf(
    ws: &Workspace,
    engine: &mut Engine,
    prep: &PreparedExport,
) -> Result<Vec<u8>> {
    let mut book = Book::single(prep.language);
    let tpl_name = ws
        .default_template()
        .unwrap_or_else(|| "olymp-classic".to_owned());
    if ws.has_custom_template(&tpl_name) {
        book.template_name = tpl_name.clone();
        book.template_import = Some(format!("/templates/{tpl_name}.typ"));
        book.template_options = ws
            .workspace_template_options()
            .remove(&tpl_name)
            .unwrap_or_default();
    } else {
        // Fallback to bundled `/xcpc/olymp-classic.typ` if workspace has no custom template file.
        book.template_name = "olymp-classic".to_owned();
        book.template_import = None;
    }
    book.preamble = ws.load_preamble();

    let entry = match &prep.contest_item {
        Some(item) => Entry {
            item: item.clone(),
            problem: &prep.problem,
        },
        None => Entry::single(&prep.problem),
    };

    let doc = xcpc_convert::statements(&book, &[entry]);
    engine.reset_files();
    let (compiled, report) = publish::compile(engine, &doc);
    let Some(compiled) = compiled else {
        let msg = report
            .diagnostics
            .iter()
            .filter(|d| d.is_error())
            .map(|d| d.display())
            .collect::<Vec<_>>()
            .join("; ");
        return Err(PackageError::PdfCompileFailed {
            slug: prep.problem.id.clone(),
            detail: if msg.is_empty() {
                "unknown Typst compilation error".to_owned()
            } else {
                msg
            },
        });
    };

    compiled.pdf().map_err(|errs| {
        let msg = errs
            .iter()
            .map(|d| d.message.as_str())
            .collect::<Vec<_>>()
            .join("; ");
        PackageError::PdfCompileFailed {
            slug: prep.problem.id.clone(),
            detail: msg,
        }
    })
}

fn read_effective_testlib(ws: &Workspace, prob_dir: &Path) -> Vec<u8> {
    for candidate in [prob_dir.join("testlib.h"), ws.root().join("testlib.h")] {
        if candidate.is_file()
            && let Ok(bytes) = fs::read(&candidate)
        {
            return bytes;
        }
    }
    TESTLIB_H.as_bytes().to_vec()
}

// ---------------------------------------------------------------------------
// Hydro OJ Export
// ---------------------------------------------------------------------------

fn write_hydro_zip(prep: &PreparedExport, zip_path: &Path) -> Result<u64> {
    let mut zip = AtomicZipWriter::create(zip_path)?;

    // 1. `problem.yaml`
    let mut prob_yaml = format!("title: {}\n", yaml_scalar(&prep.effective_meta.title));
    if !prep.effective_meta.tags.is_empty() {
        prob_yaml.push_str("tag:\n");
        for tag in &prep.effective_meta.tags {
            prob_yaml.push_str(&format!("  - {}\n", yaml_scalar(tag)));
        }
    }
    zip.add_file("problem.yaml", prob_yaml.as_bytes())?;

    // 2. Statements (`problem_zh.md` / `problem_en.md` / `problem.md`)
    let sample_pairs: Vec<(String, String)> = if !prep.problem.samples.is_empty() {
        prep.problem
            .samples
            .iter()
            .map(|s| (s.input.clone(), s.output.clone()))
            .collect()
    } else {
        prep.tests
            .iter()
            .filter(|t| t.is_sample)
            .map(|t| {
                (
                    String::from_utf8_lossy(&t.input_bytes).into_owned(),
                    String::from_utf8_lossy(&t.answer_bytes).into_owned(),
                )
            })
            .collect()
    };

    if prep.problem.statements.is_empty() {
        let md = render_hydro_statement("", &sample_pairs, prep.language);
        zip.add_file("problem.md", md.as_bytes())?;
    } else {
        for stmt in &prep.problem.statements {
            let (filename, lang) = match stmt.language {
                Some(Language::Chinese) => ("problem_zh.md", Language::Chinese),
                Some(Language::English) => ("problem_en.md", Language::English),
                None => ("problem.md", prep.language),
            };
            let md = render_hydro_statement(&stmt.file.text, &sample_pairs, lang);
            zip.add_file(filename, md.as_bytes())?;
        }
    }

    // 3. `additional_file/` (ONLY if `assets/` has files)
    let assets_dir = prep.prob_dir.join("assets");
    if assets_dir.is_dir() {
        let mut entries: Vec<PathBuf> = fs::read_dir(&assets_dir)
            .map_err(|e| io_err(format!("read {}", assets_dir.display()), e))?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file())
            .collect();
        entries.sort();
        for p in entries {
            if let Some(name) = p.file_name().and_then(|n| n.to_str())
                && !name.starts_with('.')
            {
                let bytes = fs::read(&p).map_err(|e| io_err(format!("read {}", p.display()), e))?;
                zip.add_file(&format!("additional_file/{name}"), &bytes)?;
            }
        }
    }

    // 4. `testdata/config.yaml` and optional `testdata/checker.cpp` (NEVER bundle `testlib.h`)
    let memory_mib = prep.limits.memory_limit_kib.div_ceil(1024).max(1);
    let mut cfg = format!(
        "type: default\ntime: {}ms\nmemory: {memory_mib}m\n",
        prep.limits.time_limit_ms
    );

    match &prep.checker_spec {
        CheckerSpec::Builtin(name) if matches!(name.as_str(), "wcmp" | "ncmp" | "lcmp") => {
            cfg.push_str("checker_type: default\n");
        }
        CheckerSpec::Builtin(name) => {
            cfg.push_str("checker_type: testlib\nchecker: checker.cpp\n");
            let src = standard_checker_source(name)
                .ok_or_else(|| PackageError::Unsupported(format!("unknown checker {name:?}")))?;
            zip.add_file("testdata/checker.cpp", src.as_bytes())?;
        }
        CheckerSpec::Custom { path, .. } => {
            cfg.push_str("checker_type: testlib\nchecker: checker.cpp\n");
            let bytes = fs::read(path)
                .map_err(|e| io_err(format!("read custom checker {}", path.display()), e))?;
            zip.add_file("testdata/checker.cpp", &bytes)?;
        }
    }
    zip.add_file("testdata/config.yaml", cfg.as_bytes())?;

    // 5. `testdata/01.in`, `testdata/01.out`, ... (all built tests: samples + secret)
    let width = prep.tests.len().to_string().len().max(2);
    for (i, t) in prep.tests.iter().enumerate() {
        let idx = i + 1;
        zip.add_file(&format!("testdata/{idx:0width$}.in"), &t.input_bytes)?;
        zip.add_file(&format!("testdata/{idx:0width$}.out"), &t.answer_bytes)?;
    }

    zip.finish()
}

/// Rewrites `assets/<file>` links to `file://<file>` and embeds Hydro ` ```inputN / ```outputN ` sample blocks.
fn render_hydro_statement(markdown: &str, samples: &[(String, String)], lang: Language) -> String {
    let rewritten = rewrite_assets_to_file_uri(markdown);
    if samples.is_empty() {
        return rewritten;
    }

    let mut samples_block = String::new();
    for (idx, (inp, out)) in samples.iter().enumerate() {
        let n = idx + 1;
        if idx > 0 {
            samples_block.push('\n');
        }
        samples_block.push_str(&format!("```input{n}\n"));
        samples_block.push_str(inp.trim_end_matches('\n'));
        samples_block.push_str("\n```\n\n");
        samples_block.push_str(&format!("```output{n}\n"));
        samples_block.push_str(out.trim_end_matches('\n'));
        samples_block.push_str("\n```\n");
    }

    let heading = match lang {
        Language::Chinese => "## 样例",
        Language::English => "## Samples",
    };

    // If the statement already has an explicit `## 样例` / `## Samples` heading, inject right after it.
    let lines: Vec<&str> = rewritten.lines().collect();
    for (idx, line) in lines.iter().enumerate() {
        if let Some(title) = line.strip_prefix("## ")
            && xcpc_domain::recognize_section(title) == Some(xcpc_domain::SectionKind::Samples)
        {
            let before = lines[..=idx].join("\n");
            let after = lines[idx + 1..].join("\n");
            let mut result = format!("{before}\n\n{samples_block}");
            if !after.trim().is_empty() {
                result.push('\n');
                result.push_str(after.trim_start_matches('\n'));
                result.push('\n');
            }
            return result;
        }
    }

    // Otherwise append a `## 样例` / `## Samples` section at the end.
    let mut result = rewritten.trim_end().to_owned();
    if !result.is_empty() {
        result.push_str("\n\n");
    }
    result.push_str(heading);
    result.push_str("\n\n");
    result.push_str(&samples_block);
    result
}

fn rewrite_assets_to_file_uri(markdown: &str) -> String {
    // Replace Markdown image/link targets `(assets/<file>)` and `(./assets/<file>)` with `(file://<file>)`.
    markdown
        .replace("(./assets/", "(file://")
        .replace("(assets/", "(file://")
}

/// Formats a YAML scalar safely (double-quoted with proper escaping if it contains special chars).
fn yaml_scalar(s: &str) -> String {
    let needs_quote = s.is_empty()
        || s.starts_with([
            ' ', '-', '?', ':', '#', '&', '*', '!', '|', '>', '\'', '"', '%', '@', '`', '{', '}',
            '[', ']',
        ])
        || s.ends_with(' ')
        || s.contains(": ")
        || s.contains(" #")
        || s.contains('\n')
        || s.contains('"')
        || matches!(
            s.to_ascii_lowercase().as_str(),
            "true" | "false" | "null" | "yes" | "no" | "on" | "off" | "~"
        );
    if !needs_quote {
        return s.to_owned();
    }
    let escaped = s
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");
    format!("\"{escaped}\"")
}
