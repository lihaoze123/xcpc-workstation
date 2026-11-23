//! Codeforces Polygon Standard Package ZIP importer (single problem & contest).

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use xcpc_domain::{
    ContestItem, ContestMeta, ExpectVerdict, Language, Limits, PackageConfig, ProblemMeta,
    SolutionSpec, Source, TestSpec, check_id, label_for_index,
};
use xcpc_judge::is_standard_checker;
use xcpc_storage::Workspace;

use crate::error::{PackageError, Result};
use crate::latex::{
    PolygonTexSections, assemble_statement_markdown, clean_tex_title, convert_latex_fragment,
    parse_polygon_problem_tex,
};
use crate::types::{
    ImportedProblemSummary, PackageWarning, PolygonImportKind, PolygonImportOptions,
    PolygonImportResult,
};
use crate::zip_util::{DEFAULT_MAX_ENTRY_COUNT, DEFAULT_MAX_UNCOMPRESSED_BYTES, SafeZipContents};

/// Imports a Codeforces Polygon ZIP archive (single problem or contest) into `ws`.
///
/// Enforces Zip Slip protection, size/entry limits, conflict refusal, and atomic staging
/// (no workspace files are modified if validation or parsing fails).
pub fn import_polygon(
    ws: &Workspace,
    zip_path: &Path,
    opts: &PolygonImportOptions,
) -> Result<PolygonImportResult> {
    if ws.is_read_only() {
        return Err(xcpc_storage::Error::ReadOnly.into());
    }

    let max_bytes = opts
        .max_uncompressed_bytes
        .unwrap_or(DEFAULT_MAX_UNCOMPRESSED_BYTES);
    let max_entries = opts.max_entry_count.unwrap_or(DEFAULT_MAX_ENTRY_COUNT);

    let raw_zip = SafeZipContents::read_from_path(zip_path, max_bytes, max_entries)?;
    let zip = raw_zip.strip_single_root_prefix();

    let fallback_stem = zip_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("imported");

    if let Some(contest_xml) = zip.get_text("contest.xml") {
        import_polygon_contest(ws, &zip, &contest_xml, fallback_stem, opts)
    } else if let Some(problem_xml) = zip.get_text("problem.xml") {
        import_polygon_single_problem(ws, &zip, &problem_xml, fallback_stem, opts)
    } else {
        Err(PackageError::InvalidFormat(
            "archive contains neither problem.xml nor contest.xml".to_owned(),
        ))
    }
}

fn import_polygon_single_problem(
    ws: &Workspace,
    zip: &SafeZipContents,
    problem_xml: &str,
    fallback_stem: &str,
    opts: &PolygonImportOptions,
) -> Result<PolygonImportResult> {
    let parsed = parse_problem_xml(problem_xml)?;
    let slug = resolve_target_id(
        opts.target_id.as_deref(),
        parsed.short_name.as_deref(),
        fallback_stem,
    )?;

    ensure_problem_not_exists(ws, &slug)?;

    let mut staged: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut warnings = Vec::new();
    let summary = stage_polygon_problem(zip, &parsed, &slug, None, &mut staged, &mut warnings)?;

    commit_staged_files(ws, &staged)?;

    Ok(PolygonImportResult {
        kind: PolygonImportKind::Problem,
        contest_id: None,
        problems: vec![summary],
        warnings,
    })
}

fn import_polygon_contest(
    ws: &Workspace,
    zip: &SafeZipContents,
    contest_xml: &str,
    fallback_stem: &str,
    opts: &PolygonImportOptions,
) -> Result<PolygonImportResult> {
    let parsed_contest = parse_contest_xml(contest_xml)?;
    if parsed_contest.problems.is_empty() {
        return Err(PackageError::InvalidFormat(
            "contest.xml contains no <problem> entries".to_owned(),
        ));
    }

    let contest_name = pick_preferred_name(&parsed_contest.names)
        .or_else(|| parsed_contest.name_attr.clone())
        .unwrap_or_else(|| fallback_stem.to_owned());

    let contest_id = resolve_target_id(
        opts.target_id.as_deref(),
        Some(&contest_name),
        fallback_stem,
    )?;
    ensure_contest_not_exists(ws, &contest_id)?;

    let mut staged: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut warnings = Vec::new();
    let mut summaries = Vec::new();
    let mut contest_items = Vec::new();
    let mut used_slugs = HashSet::new();
    let mut detected_lang: Option<Language> = None;

    for (idx, pref) in parsed_contest.problems.iter().enumerate() {
        let label = pref
            .index
            .clone()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| label_for_index(idx))
            .unwrap_or_else(|| format!("P{}", idx + 1));

        let sub_prefix = locate_contest_subproblem_dir(zip, pref, &label).ok_or_else(|| {
            PackageError::InvalidFormat(format!(
                "contest.xml references problem {label:?} but its problem.xml was not found in archive"
            ))
        })?;

        let sub_zip = zip.subtree(&sub_prefix);
        let sub_xml_text = sub_zip.get_text("problem.xml").ok_or_else(|| {
            PackageError::InvalidFormat(format!("missing {sub_prefix}/problem.xml in archive"))
        })?;
        let sub_parsed = parse_problem_xml(&sub_xml_text)?;

        let base_slug = slugify_id(
            pref.short_name
                .as_deref()
                .or(sub_parsed.short_name.as_deref())
                .unwrap_or(&label),
            &format!("{contest_id}-{}", label.to_ascii_lowercase()),
        );

        let mut slug = base_slug.clone();
        if !used_slugs.insert(slug.clone()) {
            slug = slugify_id(
                &format!("{contest_id}-{base_slug}"),
                &format!("{contest_id}-p{}", idx + 1),
            );
            used_slugs.insert(slug.clone());
        }

        ensure_problem_not_exists(ws, &slug)?;

        let summary = stage_polygon_problem(
            &sub_zip,
            &sub_parsed,
            &slug,
            Some(label.clone()),
            &mut staged,
            &mut warnings,
        )?;

        if detected_lang.is_none() {
            if summary.languages.iter().any(|l| l == "zh") {
                detected_lang = Some(Language::Chinese);
            } else if summary.languages.iter().any(|l| l == "en") {
                detected_lang = Some(Language::English);
            }
        }

        contest_items.push(ContestItem {
            id: slug,
            label,
            ..Default::default()
        });
        summaries.push(summary);
    }

    let contest_meta = ContestMeta {
        name: contest_name,
        language: detected_lang.unwrap_or(Language::Chinese),
        show_metadata: true,
        problems: contest_items,
        ..Default::default()
    };
    let contest_toml = toml::to_string_pretty(&contest_meta)
        .map_err(|e| PackageError::InvalidFormat(format!("serialize contest.toml: {e}")))?;
    staged.insert(
        format!("contests/{contest_id}/contest.toml"),
        contest_toml.into_bytes(),
    );

    commit_staged_files(ws, &staged)?;

    Ok(PolygonImportResult {
        kind: PolygonImportKind::Contest,
        contest_id: Some(contest_id),
        problems: summaries,
        warnings,
    })
}

fn locate_contest_subproblem_dir(
    zip: &SafeZipContents,
    pref: &ContestProblemRef,
    label: &str,
) -> Option<String> {
    let mut candidates = Vec::new();
    if let Some(p) = &pref.path {
        candidates.push(p.trim_matches('/').to_owned());
    }
    if let Some(sn) = &pref.short_name {
        candidates.push(format!("problems/{sn}"));
        candidates.push(sn.clone());
    }
    candidates.push(format!("problems/{}", label.to_ascii_lowercase()));
    candidates.push(format!("problems/{label}"));

    for cand in candidates {
        if zip.files.contains_key(&format!("{cand}/problem.xml")) {
            return Some(cand);
        }
    }
    None
}

fn stage_polygon_problem(
    zip: &SafeZipContents,
    xml: &PolygonProblemXml,
    slug: &str,
    label: Option<String>,
    staged: &mut BTreeMap<String, Vec<u8>>,
    warnings: &mut Vec<PackageWarning>,
) -> Result<ImportedProblemSummary> {
    let prob_dir = format!("problems/{slug}");

    // 1. Resolve title & limits & sources.
    let mut title = pick_preferred_name(&xml.names)
        .or_else(|| xml.short_name.clone())
        .unwrap_or_else(|| slug.to_owned());

    let limits = Limits {
        time: xml.time_limit_ms.map(format_time_limit_ms),
        memory: xml.memory_limit_bytes.map(format_memory_limit_bytes),
        input: xml.input_file.clone().filter(|s| !s.is_empty()),
        output: xml.output_file.clone().filter(|s| !s.is_empty()),
    };

    let mut sources = Vec::new();
    if xml.url.is_some() || xml.short_name.is_some() {
        sources.push(Source {
            platform: "polygon".to_owned(),
            id: xml.short_name.clone().unwrap_or_else(|| slug.to_owned()),
            url: xml.url.clone().filter(|s| !s.is_empty()),
        });
    }

    // 2. Statements & assets.
    let (languages, asset_names, stmt_title) =
        stage_statements_and_assets(zip, xml, &prob_dir, staged, warnings);
    if (title == slug || title.trim().is_empty())
        && let Some(t) = stmt_title
        && !t.trim().is_empty()
    {
        title = t;
    }

    // 3. Generators (`<files><executables>`).
    let mut cpp_generators: HashSet<String> = HashSet::new();
    let mut generators_count = 0usize;
    for exe in &xml.executables {
        let Some(bytes) = zip.get_bytes(&exe.path) else {
            continue;
        };
        let fname = file_name_of(&exe.path);
        let stem = Path::new(&fname)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&fname)
            .to_owned();
        let dest = format!("{prob_dir}/generators/{fname}");
        staged.insert(dest.clone(), bytes.to_vec());
        generators_count += 1;

        if is_cpp_source(&exe.source_type, &exe.path) {
            cpp_generators.insert(stem);
        } else {
            warnings.push(PackageWarning::with_path(
                "non_cpp_program",
                format!(
                    "non-C++ generator `{fname}` (type={:?}) was preserved; tests using it will fall back to static inputs",
                    exe.source_type
                ),
                dest,
            ));
        }
    }

    // 4. Samples and `[[test]]` specs.
    if xml.has_groups {
        warnings.push(PackageWarning::with_path(
            "subtasks_preserved",
            "Polygon <groups>/subtasks metadata is not supported in Phase 3B; all tests were imported as a flat testset",
            format!("{prob_dir}/problem.toml"),
        ));
    }

    let mut samples_count = 0usize;
    let mut test_specs: Vec<TestSpec> = Vec::new();
    for (idx0, test) in xml.tests.iter().enumerate() {
        let idx = idx0 + 1;
        let in_zip = format_printf_pattern(&xml.input_pattern, idx);
        let ans_zip = format_printf_pattern(&xml.answer_pattern, idx);

        if test.sample {
            samples_count += 1;
            let stem = format!("{samples_count:02}");
            if let Some(in_bytes) = zip.get_bytes(&in_zip) {
                staged.insert(format!("{prob_dir}/samples/{stem}.in"), in_bytes.to_vec());
            }
            if let Some(ans_bytes) = zip.get_bytes(&ans_zip) {
                staged.insert(format!("{prob_dir}/samples/{stem}.ans"), ans_bytes.to_vec());
            }
            continue;
        }

        // Non-sample test: prefer declarative `[[test]] gen = ...` when C++ generator exists & no `from-file`.
        let mut used_generator = false;
        if test.method == "generated"
            && test.from_file.is_none()
            && let Some(cmd) = test.cmd.as_deref().map(str::trim).filter(|s| !s.is_empty())
            && let Some(tokens) = shlex::split(cmd)
            && let Some(gen_name) = tokens.first()
            && cpp_generators.contains(gen_name)
        {
            let args = if tokens.len() > 1 {
                Some(cmd[gen_name.len()..].trim().to_owned())
            } else {
                None
            };
            test_specs.push(TestSpec {
                generator: Some(gen_name.clone()),
                args,
                ..Default::default()
            });
            used_generator = true;
        }

        if !used_generator {
            if let Some(in_bytes) = zip.get_bytes(&in_zip) {
                let rel_file = format!("tests/{idx:02}.in");
                staged.insert(format!("{prob_dir}/{rel_file}"), in_bytes.to_vec());
                test_specs.push(TestSpec {
                    file: Some(rel_file.clone()),
                    ..Default::default()
                });
                if test.from_file.is_some() {
                    warnings.push(PackageWarning::with_path(
                        "fallback_static_test",
                        format!(
                            "test #{idx} uses Polygon multi-test `from-file`; imported as static input `{rel_file}`"
                        ),
                        format!("{prob_dir}/{rel_file}"),
                    ));
                } else if test.method == "generated" {
                    warnings.push(PackageWarning::with_path(
                        "fallback_static_test",
                        format!(
                            "test #{idx} generator source not available as C++; imported as static input `{rel_file}`"
                        ),
                        format!("{prob_dir}/{rel_file}"),
                    ));
                }
            } else {
                warnings.push(PackageWarning::new(
                    "missing_test_input",
                    format!("test #{idx} input `{in_zip}` was not found in archive"),
                ));
            }
        }
    }

    // Fallback: if no samples were marked in problem.xml, look for `statement-sections/<lang>/example.NN`.
    if samples_count == 0 {
        samples_count = stage_statement_section_examples(zip, &prob_dir, staged);
    }

    // 5. Checker + checker self-tests.
    let checker_name = stage_checker(zip, xml, &prob_dir, staged, warnings);

    // 6. Validator + validator self-tests.
    let has_validator = stage_validator(zip, xml, &prob_dir, staged, warnings);

    // 7. Interactor (unsupported in 3B: preserve file + warning, do not abort).
    if let Some(ref interactor) = xml.interactor {
        let fname = file_name_of(&interactor.path);
        let dest = format!("{prob_dir}/{fname}");
        if let Some(bytes) = zip.get_bytes(&interactor.path) {
            staged.insert(dest.clone(), bytes.to_vec());
        }
        warnings.push(PackageWarning::with_path(
            "interactive_not_supported",
            format!(
                "interactive problem (<interactor> `{fname}`) is not supported for execution in Phase 3B; file preserved"
            ),
            dest,
        ));
    }

    // 8. Solutions (`<assets><solutions>`).
    let mut solutions_map: BTreeMap<String, SolutionSpec> = BTreeMap::new();
    let mut solutions_count = 0usize;
    let mut has_main = false;
    for sol in &xml.solutions {
        let Some(bytes) = zip.get_bytes(&sol.path) else {
            continue;
        };
        let fname = file_name_of(&sol.path);
        let dest = format!("{prob_dir}/solutions/{fname}");
        staged.insert(dest.clone(), bytes.to_vec());
        solutions_count += 1;

        if !is_cpp_source(&sol.source_type, &sol.path) {
            warnings.push(PackageWarning::with_path(
                "non_cpp_program",
                format!(
                    "non-C++ solution `{fname}` (type={:?}) was preserved in solutions/ but excluded from C++ [solutions.*] verification",
                    sol.source_type
                ),
                dest,
            ));
            continue;
        }

        let logical_stem = xcpc_judge::SourceLanguage::logical_stem(Path::new(&fname))
            .unwrap_or_else(|| {
                Path::new(&fname)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or(&fname)
                    .to_owned()
            });
        let (is_main, expect) = map_polygon_solution_tag(&sol.tag);
        let main_flag = if is_main && !has_main {
            has_main = true;
            Some(true)
        } else {
            None
        };
        solutions_map.insert(
            logical_stem,
            SolutionSpec {
                main: main_flag,
                expect: Some(expect),
                tags: None,
            },
        );
    }

    // 9. Build and serialize `problem.toml`.
    let meta = ProblemMeta {
        title: title.clone(),
        limits,
        sources,
        package: Some(PackageConfig {
            checker: Some(checker_name.clone()),
        }),
        test: test_specs.clone(),
        solutions: solutions_map,
        ..Default::default()
    };

    let toml_text = toml::to_string_pretty(&meta)
        .map_err(|e| PackageError::InvalidFormat(format!("serialize problem.toml: {e}")))?;
    staged.insert(format!("{prob_dir}/problem.toml"), toml_text.into_bytes());

    Ok(ImportedProblemSummary {
        id: slug.to_owned(),
        label,
        title,
        languages,
        samples_count,
        tests_count: test_specs.len(),
        generators_count,
        solutions_count,
        has_validator,
        checker: checker_name,
        assets_count: asset_names.len(),
    })
}

fn stage_statements_and_assets(
    zip: &SafeZipContents,
    xml: &PolygonProblemXml,
    prob_dir: &str,
    staged: &mut BTreeMap<String, Vec<u8>>,
    warnings: &mut Vec<PackageWarning>,
) -> (Vec<String>, HashSet<String>, Option<String>) {
    // Collect available languages from both `statement-sections/<lang>/` and `<statements>`.
    let mut raw_langs: Vec<String> = Vec::new();
    for key in zip.files.keys() {
        if let Some(rest) = key.strip_prefix("statement-sections/")
            && let Some((lang, _)) = rest.split_once('/')
            && !raw_langs.iter().any(|l| l == lang)
        {
            raw_langs.push(lang.to_owned());
        }
    }
    for st in &xml.statements {
        if !raw_langs.iter().any(|l| l == &st.language) {
            raw_langs.push(st.language.clone());
        }
    }

    let mut written_langs = Vec::new();
    let mut copied_assets: HashSet<String> = HashSet::new();
    let mut first_title: Option<String> = None;
    let mut wrote_editorial = false;

    for raw_lang in &raw_langs {
        let mapped_lang = map_polygon_language(raw_lang);
        let (target_file, render_lang, lang_code) = match mapped_lang {
            Some(Language::Chinese) => ("statement.zh.md", Language::Chinese, "zh"),
            Some(Language::English) => ("statement.en.md", Language::English, "en"),
            None => {
                if raw_langs.len() > 1 {
                    warnings.push(PackageWarning::new(
                        "unsupported_statement_language",
                        format!("skipped statement in unsupported language `{raw_lang}`"),
                    ));
                    continue;
                }
                warnings.push(PackageWarning::new(
                    "unsupported_statement_language",
                    format!(
                        "statement language `{raw_lang}` is not Chinese/English; imported as `statement.md`"
                    ),
                ));
                ("statement.md", Language::English, "default")
            }
        };

        let sec_prefix = format!("statement-sections/{raw_lang}");
        let has_sections = zip
            .files
            .keys()
            .any(|k| k.starts_with(&format!("{sec_prefix}/")) && k.ends_with(".tex"));

        let sections = if has_sections {
            PolygonTexSections {
                title: zip
                    .get_text(&format!("{sec_prefix}/name.tex"))
                    .map(|s| clean_tex_title(&s))
                    .filter(|s| !s.is_empty()),
                legend: zip
                    .get_text(&format!("{sec_prefix}/legend.tex"))
                    .unwrap_or_default(),
                input: zip
                    .get_text(&format!("{sec_prefix}/input.tex"))
                    .unwrap_or_default(),
                output: zip
                    .get_text(&format!("{sec_prefix}/output.tex"))
                    .unwrap_or_default(),
                interaction: zip
                    .get_text(&format!("{sec_prefix}/interaction.tex"))
                    .unwrap_or_default(),
                notes: zip
                    .get_text(&format!("{sec_prefix}/notes.tex"))
                    .unwrap_or_default(),
                scoring: zip
                    .get_text(&format!("{sec_prefix}/scoring.tex"))
                    .unwrap_or_default(),
                tutorial: zip.get_text(&format!("{sec_prefix}/tutorial.tex")),
            }
        } else {
            let tex_path = xml
                .statements
                .iter()
                .find(|s| s.language == *raw_lang)
                .map(|s| s.path.clone())
                .unwrap_or_else(|| format!("statements/{raw_lang}/problem.tex"));
            match zip.get_text(&tex_path) {
                Some(tex) => parse_polygon_problem_tex(&tex),
                None => continue,
            }
        };

        if first_title.is_none() && sections.title.is_some() {
            first_title = sections.title.clone();
        }

        let converted = assemble_statement_markdown(&sections, render_lang, warnings, &sec_prefix);
        if !converted.markdown.trim().is_empty() {
            staged.insert(
                format!("{prob_dir}/{target_file}"),
                converted.markdown.into_bytes(),
            );
            written_langs.push(lang_code.to_owned());
        }

        // Convert optional tutorial.tex -> editorial.md (first language that has one).
        if !wrote_editorial
            && let Some(ref tut) = sections.tutorial
            && !tut.trim().is_empty()
        {
            let conv_tut =
                convert_latex_fragment(tut, warnings, &format!("{sec_prefix}/tutorial.tex"));
            if !conv_tut.markdown.trim().is_empty() {
                staged.insert(
                    format!("{prob_dir}/editorial.md"),
                    format!("{}\n", conv_tut.markdown.trim()).into_bytes(),
                );
                wrote_editorial = true;
                for img in conv_tut.images {
                    copy_statement_asset(
                        zip,
                        raw_lang,
                        &img,
                        prob_dir,
                        staged,
                        &mut copied_assets,
                        warnings,
                    );
                }
            }
        }

        for img in converted.images {
            copy_statement_asset(
                zip,
                raw_lang,
                &img,
                prob_dir,
                staged,
                &mut copied_assets,
                warnings,
            );
        }
    }

    if written_langs.is_empty() {
        warnings.push(PackageWarning::with_path(
            "missing_statement",
            "no TeX statement found in archive; created empty placeholder statement.md",
            format!("{prob_dir}/statement.md"),
        ));
        staged.insert(format!("{prob_dir}/statement.md"), Vec::new());
        written_langs.push("default".to_owned());
    }

    (written_langs, copied_assets, first_title)
}

fn copy_statement_asset(
    zip: &SafeZipContents,
    raw_lang: &str,
    raw_img: &str,
    prob_dir: &str,
    staged: &mut BTreeMap<String, Vec<u8>>,
    copied_assets: &mut HashSet<String>,
    warnings: &mut Vec<PackageWarning>,
) {
    let fname = file_name_of(raw_img);
    if copied_assets.contains(&fname) {
        return;
    }
    let search_paths = [
        format!("statement-sections/{raw_lang}/{raw_img}"),
        format!("statement-sections/{raw_lang}/{fname}"),
        format!("statements/{raw_lang}/{raw_img}"),
        format!("statements/{raw_lang}/{fname}"),
        format!("files/{fname}"),
        raw_img.to_owned(),
    ];
    for cand in &search_paths {
        if let Some(bytes) = zip.get_bytes(cand) {
            staged.insert(format!("{prob_dir}/assets/{fname}"), bytes.to_vec());
            copied_assets.insert(fname);
            return;
        }
    }
    warnings.push(PackageWarning::with_path(
        "asset_missing",
        format!("referenced statement image `{raw_img}` was not found in archive"),
        format!("{prob_dir}/assets/{fname}"),
    ));
}

fn stage_statement_section_examples(
    zip: &SafeZipContents,
    prob_dir: &str,
    staged: &mut BTreeMap<String, Vec<u8>>,
) -> usize {
    let mut count = 0usize;
    // Find first `statement-sections/<lang>/example.01`
    let lang = zip.files.keys().find_map(|k| {
        let rest = k.strip_prefix("statement-sections/")?;
        let (l, file) = rest.split_once('/')?;
        file.starts_with("example.").then(|| l.to_owned())
    });
    let Some(lang) = lang else {
        return 0;
    };
    for idx in 1..=99 {
        let in_key = format!("statement-sections/{lang}/example.{idx:02}");
        let ans_key = format!("statement-sections/{lang}/example.{idx:02}.a");
        if let (Some(in_b), Some(ans_b)) = (zip.get_bytes(&in_key), zip.get_bytes(&ans_key)) {
            count += 1;
            staged.insert(format!("{prob_dir}/samples/{count:02}.in"), in_b.to_vec());
            staged.insert(format!("{prob_dir}/samples/{count:02}.ans"), ans_b.to_vec());
        } else {
            break;
        }
    }
    count
}

fn stage_checker(
    zip: &SafeZipContents,
    xml: &PolygonProblemXml,
    prob_dir: &str,
    staged: &mut BTreeMap<String, Vec<u8>>,
    warnings: &mut Vec<PackageWarning>,
) -> String {
    let Some(ref checker) = xml.checker else {
        return "wcmp".to_owned();
    };

    // Map `std::wcmp.cpp` or `std::wcmp` -> `wcmp` if in STANDARD_CHECKERS.
    if let Some(ref name) = checker.name {
        let trimmed = name.trim();
        let no_prefix = trimmed.strip_prefix("std::").unwrap_or(trimmed);
        let stripped = no_prefix.strip_suffix(".cpp").unwrap_or(no_prefix);
        if is_standard_checker(stripped) {
            return stripped.to_owned();
        }
    }

    // Custom checker.
    if let Some(ref src_path) = checker.source_path
        && let Some(bytes) = zip.get_bytes(src_path)
    {
        if is_cpp_source(&checker.source_type, src_path) {
            staged.insert(format!("{prob_dir}/checker.cpp"), bytes.to_vec());

            // Import checker self-tests if present.
            if let Some(ref ts) = checker.testset {
                for (i0, verdict) in ts.verdicts.iter().enumerate() {
                    let idx = i0 + 1;
                    let prefix = match verdict.as_str() {
                        "ok" => "ok",
                        "wrong-answer" => "wa",
                        "presentation-error" => "pe",
                        _ => continue,
                    };
                    let in_key = format_printf_pattern(&ts.input_pattern, idx);
                    let out_key = format_printf_pattern(&ts.output_pattern, idx);
                    let ans_key = format_printf_pattern(&ts.answer_pattern, idx);
                    if let (Some(in_b), Some(out_b), Some(ans_b)) = (
                        zip.get_bytes(&in_key),
                        zip.get_bytes(&out_key),
                        zip.get_bytes(&ans_key),
                    ) {
                        let case_dir = format!("{prob_dir}/tests/checker/{prefix}-{idx:02}");
                        staged.insert(format!("{case_dir}/in"), in_b.to_vec());
                        staged.insert(format!("{case_dir}/out"), out_b.to_vec());
                        staged.insert(format!("{case_dir}/ans"), ans_b.to_vec());
                    }
                }
            }
            return "checker.cpp".to_owned();
        } else {
            let fname = file_name_of(src_path);
            let dest = format!("{prob_dir}/{fname}");
            staged.insert(dest.clone(), bytes.to_vec());
            warnings.push(PackageWarning::with_path(
                "non_cpp_program",
                format!(
                    "non-C++ checker `{fname}` (type={:?}) was preserved; falling back to `wcmp` in problem.toml",
                    checker.source_type
                ),
                dest,
            ));
        }
    }

    "wcmp".to_owned()
}

fn stage_validator(
    zip: &SafeZipContents,
    xml: &PolygonProblemXml,
    prob_dir: &str,
    staged: &mut BTreeMap<String, Vec<u8>>,
    warnings: &mut Vec<PackageWarning>,
) -> bool {
    let Some(val) = xml.validators.first() else {
        return false;
    };
    let Some(bytes) = zip.get_bytes(&val.source_path) else {
        return false;
    };

    if !is_cpp_source(&val.source_type, &val.source_path) {
        let fname = file_name_of(&val.source_path);
        let dest = format!("{prob_dir}/{fname}");
        staged.insert(dest.clone(), bytes.to_vec());
        warnings.push(PackageWarning::with_path(
            "non_cpp_program",
            format!(
                "non-C++ validator `{fname}` (type={:?}) was preserved but cannot be executed by xcpc-judge",
                val.source_type
            ),
            dest,
        ));
        return false;
    }

    staged.insert(format!("{prob_dir}/validator.cpp"), bytes.to_vec());

    if let Some(ref ts) = val.testset {
        let mut valid_idx = 0usize;
        let mut invalid_idx = 0usize;
        for (i0, verdict) in ts.verdicts.iter().enumerate() {
            let idx = i0 + 1;
            let in_key = format_printf_pattern(&ts.input_pattern, idx);
            let Some(in_b) = zip.get_bytes(&in_key) else {
                continue;
            };
            match verdict.as_str() {
                "valid" => {
                    valid_idx += 1;
                    staged.insert(
                        format!("{prob_dir}/tests/validator/valid/{valid_idx:02}.in"),
                        in_b.to_vec(),
                    );
                }
                "invalid" => {
                    invalid_idx += 1;
                    staged.insert(
                        format!("{prob_dir}/tests/validator/invalid/{invalid_idx:02}.in"),
                        in_b.to_vec(),
                    );
                }
                _ => {}
            }
        }
    }

    true
}

fn commit_staged_files(ws: &Workspace, staged: &BTreeMap<String, Vec<u8>>) -> Result<()> {
    // Pre-validate every single destination path before writing anything.
    for rel in staged.keys() {
        ws.check_writable(rel)?;
        ws.resolve(rel)?;
    }
    for (rel, bytes) in staged {
        ws.write(rel, bytes)?;
    }
    Ok(())
}

fn ensure_problem_not_exists(ws: &Workspace, slug: &str) -> Result<()> {
    let dir = ws.resolve(&format!("problems/{slug}"))?;
    if dir.exists() {
        return Err(PackageError::TargetExists {
            kind: "problem",
            id: slug.to_owned(),
        });
    }
    Ok(())
}

fn ensure_contest_not_exists(ws: &Workspace, id: &str) -> Result<()> {
    let dir = ws.resolve(&format!("contests/{id}"))?;
    if dir.exists() {
        return Err(PackageError::TargetExists {
            kind: "contest",
            id: id.to_owned(),
        });
    }
    Ok(())
}

fn resolve_target_id(
    explicit: Option<&str>,
    preferred: Option<&str>,
    fallback: &str,
) -> Result<String> {
    if let Some(id) = explicit.map(str::trim).filter(|s| !s.is_empty()) {
        check_id(id).map_err(xcpc_storage::Error::from)?;
        return Ok(id.to_owned());
    }
    let raw = preferred
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(fallback);
    Ok(slugify_id(raw, "imported"))
}

/// Normalizes an arbitrary string into a valid XCPC slug (`[a-z0-9][a-z0-9_-]*`, len <= 64).
pub(crate) fn slugify_id(raw: &str, fallback: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if (ch == '-' || ch == '_' || ch.is_whitespace()) && !out.is_empty() && !prev_dash {
            out.push(if ch == '_' { '_' } else { '-' });
            prev_dash = true;
        }
    }
    let trimmed = out.trim_matches(['-', '_']).to_owned();
    let mut candidate = if trimmed.is_empty() {
        fallback.to_ascii_lowercase()
    } else {
        trimmed
    };
    if candidate.len() > 64 {
        candidate.truncate(64);
        candidate = candidate.trim_matches(['-', '_']).to_owned();
    }
    if check_id(&candidate).is_ok() {
        candidate
    } else {
        "imported".to_owned()
    }
}

fn map_polygon_language(raw: &str) -> Option<Language> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "chinese" | "zh" | "zh-cn" | "zh_cn" | "zh-hans" => Some(Language::Chinese),
        "english" | "en" | "en-us" | "en_us" => Some(Language::English),
        _ => None,
    }
}

fn pick_preferred_name(names: &[(String, String)]) -> Option<String> {
    for want in ["chinese", "zh", "english", "en"] {
        if let Some((_, val)) = names
            .iter()
            .find(|(l, v)| l.eq_ignore_ascii_case(want) && !v.trim().is_empty())
        {
            return Some(val.trim().to_owned());
        }
    }
    names
        .iter()
        .find(|(_, v)| !v.trim().is_empty())
        .map(|(_, v)| v.trim().to_owned())
}

fn map_polygon_solution_tag(tag: &str) -> (bool, ExpectVerdict) {
    match tag.trim().to_ascii_lowercase().as_str() {
        "main" => (true, ExpectVerdict::Single("AC".to_owned())),
        "accepted" => (false, ExpectVerdict::Single("AC".to_owned())),
        "wrong-answer" => (false, ExpectVerdict::Single("WA".to_owned())),
        "time-limit-exceeded" => (false, ExpectVerdict::Single("TLE".to_owned())),
        "time-limit-exceeded-or-accepted" => (
            false,
            ExpectVerdict::Multiple(vec!["TLE".to_owned(), "AC".to_owned()]),
        ),
        "time-limit-exceeded-or-memory-limit-exceeded" => (
            false,
            ExpectVerdict::Multiple(vec!["TLE".to_owned(), "MLE".to_owned()]),
        ),
        "memory-limit-exceeded" => (false, ExpectVerdict::Single("MLE".to_owned())),
        "presentation-error" => (false, ExpectVerdict::Single("PE".to_owned())),
        "rejected" | "failed" => (
            false,
            ExpectVerdict::Multiple(vec![
                "WA".to_owned(),
                "TLE".to_owned(),
                "MLE".to_owned(),
                "RE".to_owned(),
                "PE".to_owned(),
            ]),
        ),
        _ => (false, ExpectVerdict::Single("ANY".to_owned())),
    }
}

fn is_cpp_source(source_type: &str, path: &str) -> bool {
    let t = source_type.trim().to_ascii_lowercase();
    if t.starts_with("cpp") || t.contains("g++") || t.contains("c++") {
        return true;
    }
    if !t.is_empty() {
        return false;
    }
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(ext.as_str(), "cpp" | "cc" | "cxx")
}

fn format_time_limit_ms(ms: u64) -> String {
    if ms.is_multiple_of(1000) {
        format!("{}s", ms / 1000)
    } else if ms >= 1000 && ms.is_multiple_of(100) {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        format!("{ms}ms")
    }
}

fn format_memory_limit_bytes(bytes: u64) -> String {
    const MIB: u64 = 1024 * 1024;
    if bytes.is_multiple_of(MIB) {
        format!("{}MB", bytes / MIB)
    } else {
        format!("{}KB", bytes.div_ceil(1024))
    }
}

/// Formats a simple Polygon printf-style path pattern like `tests/%02d` or `tests/%03d.a`.
fn format_printf_pattern(pattern: &str, idx: usize) -> String {
    if let Some(pct) = pattern.find('%') {
        let rest = &pattern[pct + 1..];
        if let Some(d_pos) = rest.find('d') {
            let spec = &rest[..d_pos];
            let width: usize = spec.trim_start_matches('0').parse().unwrap_or(0);
            let zero_pad = spec.starts_with('0');
            let formatted = if zero_pad && width > 0 {
                format!("{idx:0width$}")
            } else if width > 0 {
                format!("{idx:width$}")
            } else {
                idx.to_string()
            };
            return format!("{}{}{}", &pattern[..pct], formatted, &rest[d_pos + 1..]);
        }
    }
    pattern.to_owned()
}

fn file_name_of(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path)
        .to_owned()
}

// ---------------------------------------------------------------------------
// Polygon XML models & quick-xml parsers
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
struct PolygonProblemXml {
    short_name: Option<String>,
    url: Option<String>,
    names: Vec<(String, String)>,
    statements: Vec<PolygonStatementRef>,
    input_file: Option<String>,
    output_file: Option<String>,
    time_limit_ms: Option<u64>,
    memory_limit_bytes: Option<u64>,
    input_pattern: String,
    answer_pattern: String,
    has_groups: bool,
    tests: Vec<PolygonTestEntry>,
    executables: Vec<PolygonSourceFile>,
    checker: Option<PolygonChecker>,
    validators: Vec<PolygonValidator>,
    solutions: Vec<PolygonSolution>,
    interactor: Option<PolygonSourceFile>,
}

#[derive(Debug)]
struct PolygonStatementRef {
    language: String,
    path: String,
}

#[derive(Debug)]
struct PolygonTestEntry {
    method: String,
    cmd: Option<String>,
    sample: bool,
    from_file: Option<String>,
}

#[derive(Debug)]
struct PolygonSourceFile {
    path: String,
    source_type: String,
}

#[derive(Debug, Default)]
struct PolygonChecker {
    name: Option<String>,
    source_path: Option<String>,
    source_type: String,
    testset: Option<PolygonCheckerTestset>,
}

#[derive(Debug, Default)]
struct PolygonCheckerTestset {
    input_pattern: String,
    output_pattern: String,
    answer_pattern: String,
    verdicts: Vec<String>,
}

#[derive(Debug, Default)]
struct PolygonValidator {
    source_path: String,
    source_type: String,
    testset: Option<PolygonValidatorTestset>,
}

#[derive(Debug, Default)]
struct PolygonValidatorTestset {
    input_pattern: String,
    verdicts: Vec<String>,
}

#[derive(Debug)]
struct PolygonSolution {
    tag: String,
    path: String,
    source_type: String,
}

#[derive(Debug, Default)]
struct PolygonContestXml {
    name_attr: Option<String>,
    names: Vec<(String, String)>,
    problems: Vec<ContestProblemRef>,
}

#[derive(Debug)]
struct ContestProblemRef {
    index: Option<String>,
    path: Option<String>,
    short_name: Option<String>,
}

fn attr_str(e: &BytesStart<'_>, name: &[u8]) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.as_ref() == name)
        .and_then(|a| a.unescape_value().ok())
        .map(|v| v.into_owned())
}

fn parse_problem_xml(xml: &str) -> Result<PolygonProblemXml> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut out = PolygonProblemXml {
        input_pattern: "tests/%02d".to_owned(),
        answer_pattern: "tests/%02d.a".to_owned(),
        ..Default::default()
    };

    let mut stack: Vec<String> = Vec::new();
    let mut current_text = String::new();
    let mut in_primary_testset = false;
    let mut saw_primary_testset = false;
    let mut current_solution_tag: Option<String> = None;
    let mut current_name_lang: Option<String> = None;
    let mut current_name_has_value = false;

    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                handle_problem_open_tag(
                    &tag,
                    e,
                    &stack,
                    &mut out,
                    &mut in_primary_testset,
                    &mut saw_primary_testset,
                    &mut current_solution_tag,
                    &mut current_name_lang,
                    &mut current_name_has_value,
                );
                stack.push(tag);
                current_text.clear();
            }
            Ok(Event::Empty(ref e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                handle_problem_open_tag(
                    &tag,
                    e,
                    &stack,
                    &mut out,
                    &mut in_primary_testset,
                    &mut saw_primary_testset,
                    &mut current_solution_tag,
                    &mut current_name_lang,
                    &mut current_name_has_value,
                );
                if tag == "solution" {
                    current_solution_tag = None;
                }
            }
            Ok(Event::Text(ref e)) => {
                if let Ok(t) = e.xml_content() {
                    current_text.push_str(&t);
                }
            }
            Ok(Event::End(ref e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                let parent = stack
                    .len()
                    .checked_sub(2)
                    .and_then(|i| stack.get(i))
                    .map(String::as_str);

                if tag == "name"
                    && parent == Some("names")
                    && !current_name_has_value
                    && let Some(lang) = current_name_lang.take()
                    && !current_text.trim().is_empty()
                {
                    out.names.push((lang, current_text.trim().to_owned()));
                } else if in_primary_testset {
                    match tag.as_str() {
                        "time-limit" => {
                            if let Ok(v) = current_text.trim().parse::<u64>() {
                                out.time_limit_ms = Some(v);
                            }
                        }
                        "memory-limit" => {
                            if let Ok(v) = current_text.trim().parse::<u64>() {
                                out.memory_limit_bytes = Some(v);
                            }
                        }
                        "input-path-pattern" if !current_text.trim().is_empty() => {
                            out.input_pattern = current_text.trim().to_owned();
                        }
                        "answer-path-pattern" if !current_text.trim().is_empty() => {
                            out.answer_pattern = current_text.trim().to_owned();
                        }
                        "testset" => {
                            in_primary_testset = false;
                        }
                        _ => {}
                    }
                } else if stack.iter().any(|s| s == "checker")
                    && stack.iter().any(|s| s == "testset")
                    && let Some(ref mut chk) = out.checker
                    && let Some(ref mut ts) = chk.testset
                {
                    match tag.as_str() {
                        "input-path-pattern" if !current_text.trim().is_empty() => {
                            ts.input_pattern = current_text.trim().to_owned();
                        }
                        "output-path-pattern" if !current_text.trim().is_empty() => {
                            ts.output_pattern = current_text.trim().to_owned();
                        }
                        "answer-path-pattern" if !current_text.trim().is_empty() => {
                            ts.answer_pattern = current_text.trim().to_owned();
                        }
                        _ => {}
                    }
                } else if stack.iter().any(|s| s == "validator")
                    && stack.iter().any(|s| s == "testset")
                    && let Some(val) = out.validators.last_mut()
                    && let Some(ref mut ts) = val.testset
                    && tag == "input-path-pattern"
                    && !current_text.trim().is_empty()
                {
                    ts.input_pattern = current_text.trim().to_owned();
                }

                if tag == "solution" {
                    current_solution_tag = None;
                }
                stack.pop();
                current_text.clear();
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(PackageError::InvalidFormat(format!(
                    "failed to parse problem.xml: {e}"
                )));
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(out)
}

#[allow(clippy::too_many_arguments)]
fn handle_problem_open_tag(
    tag: &str,
    e: &BytesStart<'_>,
    stack: &[String],
    out: &mut PolygonProblemXml,
    in_primary_testset: &mut bool,
    saw_primary_testset: &mut bool,
    current_solution_tag: &mut Option<String>,
    current_name_lang: &mut Option<String>,
    current_name_has_value: &mut bool,
) {
    let parent = stack.last().map(String::as_str);

    match tag {
        "problem" if stack.is_empty() => {
            out.short_name = attr_str(e, b"short-name");
            out.url = attr_str(e, b"url");
        }
        "name" if parent == Some("names") => {
            let lang = attr_str(e, b"language").unwrap_or_default();
            if let Some(val) = attr_str(e, b"value") {
                out.names.push((lang, val));
                *current_name_has_value = true;
                *current_name_lang = None;
            } else {
                *current_name_has_value = false;
                *current_name_lang = Some(lang);
            }
        }
        "statement" if parent == Some("statements") => {
            let ty = attr_str(e, b"type").unwrap_or_default();
            if (ty.is_empty() || ty == "application/x-tex")
                && let (Some(lang), Some(path)) = (attr_str(e, b"language"), attr_str(e, b"path"))
            {
                out.statements.push(PolygonStatementRef {
                    language: lang,
                    path,
                });
            }
        }
        "judging" => {
            out.input_file = attr_str(e, b"input-file");
            out.output_file = attr_str(e, b"output-file");
        }
        "testset" if parent == Some("judging") => {
            let name = attr_str(e, b"name").unwrap_or_default();
            if name == "tests" || !*saw_primary_testset {
                if name == "tests" && *saw_primary_testset {
                    // Reset if an earlier non-"tests" testset was parsed first.
                    out.tests.clear();
                    out.has_groups = false;
                }
                *in_primary_testset = true;
                *saw_primary_testset = true;
            }
        }
        "groups" if *in_primary_testset => {
            out.has_groups = true;
        }
        "test" if *in_primary_testset && parent == Some("tests") => {
            let method = attr_str(e, b"method").unwrap_or_else(|| "manual".to_owned());
            let cmd = attr_str(e, b"cmd");
            let sample = attr_str(e, b"sample").is_some_and(|v| v == "true");
            let from_file = attr_str(e, b"from-file");
            if attr_str(e, b"group").is_some() {
                out.has_groups = true;
            }
            out.tests.push(PolygonTestEntry {
                method,
                cmd,
                sample,
                from_file,
            });
        }
        "source" if parent == Some("executable") => {
            if let Some(path) = attr_str(e, b"path") {
                out.executables.push(PolygonSourceFile {
                    path,
                    source_type: attr_str(e, b"type").unwrap_or_default(),
                });
            }
        }
        "checker" if parent == Some("assets") => {
            out.checker = Some(PolygonChecker {
                name: attr_str(e, b"name"),
                ..Default::default()
            });
        }
        "source" if parent == Some("checker") => {
            if let Some(ref mut chk) = out.checker {
                chk.source_path = attr_str(e, b"path");
                chk.source_type = attr_str(e, b"type").unwrap_or_default();
            }
        }
        "testset" if parent == Some("checker") => {
            if let Some(ref mut chk) = out.checker {
                chk.testset = Some(PolygonCheckerTestset {
                    input_pattern: "files/tests/checker-tests/%02d".to_owned(),
                    output_pattern: "files/tests/checker-tests/%02d.o".to_owned(),
                    answer_pattern: "files/tests/checker-tests/%02d.a".to_owned(),
                    verdicts: Vec::new(),
                });
            }
        }
        "test"
            if parent == Some("tests")
                && stack.iter().any(|s| s == "checker")
                && let Some(ref mut chk) = out.checker
                && let Some(ref mut ts) = chk.testset =>
        {
            if let Some(v) = attr_str(e, b"verdict") {
                ts.verdicts.push(v);
            }
        }
        "validator" if parent == Some("validators") => {
            out.validators.push(PolygonValidator::default());
        }
        "source" if parent == Some("validator") => {
            if let Some(val) = out.validators.last_mut() {
                val.source_path = attr_str(e, b"path").unwrap_or_default();
                val.source_type = attr_str(e, b"type").unwrap_or_default();
            }
        }
        "testset" if parent == Some("validator") => {
            if let Some(val) = out.validators.last_mut() {
                val.testset = Some(PolygonValidatorTestset {
                    input_pattern: "files/tests/validator-tests/%02d".to_owned(),
                    verdicts: Vec::new(),
                });
            }
        }
        "test"
            if parent == Some("tests")
                && stack.iter().any(|s| s == "validator")
                && let Some(val) = out.validators.last_mut()
                && let Some(ref mut ts) = val.testset =>
        {
            if let Some(v) = attr_str(e, b"verdict") {
                ts.verdicts.push(v);
            }
        }
        "solution" if parent == Some("solutions") => {
            *current_solution_tag = Some(attr_str(e, b"tag").unwrap_or_default());
        }
        "source" if parent == Some("solution") => {
            if let Some(path) = attr_str(e, b"path") {
                out.solutions.push(PolygonSolution {
                    tag: current_solution_tag.clone().unwrap_or_default(),
                    path,
                    source_type: attr_str(e, b"type").unwrap_or_default(),
                });
            }
        }
        "source" if parent == Some("interactor") => {
            if let Some(path) = attr_str(e, b"path") {
                out.interactor = Some(PolygonSourceFile {
                    path,
                    source_type: attr_str(e, b"type").unwrap_or_default(),
                });
            }
        }
        _ => {}
    }
}

fn parse_contest_xml(xml: &str) -> Result<PolygonContestXml> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut out = PolygonContestXml::default();
    let mut stack: Vec<String> = Vec::new();
    let mut current_text = String::new();
    let mut current_name_lang: Option<String> = None;
    let mut current_name_has_value = false;

    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                handle_contest_open_tag(
                    &tag,
                    e,
                    &stack,
                    &mut out,
                    &mut current_name_lang,
                    &mut current_name_has_value,
                );
                stack.push(tag);
                current_text.clear();
            }
            Ok(Event::Empty(ref e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                handle_contest_open_tag(
                    &tag,
                    e,
                    &stack,
                    &mut out,
                    &mut current_name_lang,
                    &mut current_name_has_value,
                );
            }
            Ok(Event::Text(ref e)) => {
                if let Ok(t) = e.xml_content() {
                    current_text.push_str(&t);
                }
            }
            Ok(Event::End(ref e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                let parent = stack
                    .len()
                    .checked_sub(2)
                    .and_then(|i| stack.get(i))
                    .map(String::as_str);
                if tag == "name"
                    && parent == Some("names")
                    && !current_name_has_value
                    && let Some(lang) = current_name_lang.take()
                    && !current_text.trim().is_empty()
                {
                    out.names.push((lang, current_text.trim().to_owned()));
                }
                stack.pop();
                current_text.clear();
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(PackageError::InvalidFormat(format!(
                    "failed to parse contest.xml: {e}"
                )));
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(out)
}

fn handle_contest_open_tag(
    tag: &str,
    e: &BytesStart<'_>,
    stack: &[String],
    out: &mut PolygonContestXml,
    current_name_lang: &mut Option<String>,
    current_name_has_value: &mut bool,
) {
    let parent = stack.last().map(String::as_str);
    match tag {
        "contest" if stack.is_empty() => {
            out.name_attr = attr_str(e, b"name");
        }
        "name" if parent == Some("names") => {
            let lang = attr_str(e, b"language").unwrap_or_default();
            if let Some(val) = attr_str(e, b"value") {
                out.names.push((lang, val));
                *current_name_has_value = true;
                *current_name_lang = None;
            } else {
                *current_name_has_value = false;
                *current_name_lang = Some(lang);
            }
        }
        "problem" if parent == Some("problems") => {
            out.problems.push(ContestProblemRef {
                index: attr_str(e, b"index"),
                path: attr_str(e, b"path"),
                short_name: attr_str(e, b"short-name"),
            });
        }
        _ => {}
    }
}
