//! The pipeline end to end: workspace → Typst → pages, with the statement fonts
//! (run in the dev shell, which sets XCPC_FONT_DIRS).

use std::fs;
use std::path::Path;

use typst::layout::{Abs, Frame, FrameItem, Point, Transform};
use xcpc_convert::publish::{self, Kind, Report, SourceLocation, Target};
use xcpc_engine::{Compiled, FontOptions};
use xcpc_storage::{Error as StorageError, Workspace};

fn fixture(name: &str) -> Workspace {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/olymp-ref")
        .join(name)
        .join("workspace");
    Workspace::open(root, true).unwrap()
}

fn build(ws: &Workspace, target: Target, kind: Kind) -> (Option<xcpc_engine::Compiled>, Report) {
    let doc = publish::document(ws, &target, kind, None).unwrap();
    let mut engine = publish::engine(ws, &FontOptions::from_env());
    publish::compile(&mut engine, &doc)
}

/// A scratch workspace with one problem.
fn scratch(statement: &str) -> (tempfile::TempDir, Workspace) {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("problems/p");
    fs::create_dir_all(p.join("samples")).unwrap();
    fs::write(dir.path().join("workspace.toml"), "").unwrap();
    fs::write(
        p.join("problem.toml"),
        "title = \"P\"\n[limits]\ntime = \"1s\"\nmemory = \"256MB\"\n",
    )
    .unwrap();
    fs::write(p.join("statement.md"), statement).unwrap();
    fs::write(p.join("samples/01.in"), "1\n").unwrap();
    fs::write(p.join("samples/01.ans"), "2\n").unwrap();
    let ws = Workspace::open(dir.path(), false).unwrap();
    (dir, ws)
}

#[test]
fn a_contest_book_has_its_cover_and_problem_pages() {
    let ws = fixture("contest-zh");
    let (compiled, report) = build(&ws, Target::Contest("contest-zh".into()), Kind::Statements);
    assert!(report.ok, "{:?}", report.diagnostics);
    assert_eq!(compiled.unwrap().page_count(), 5);
    let pages: Vec<_> = report
        .problems
        .iter()
        .map(|p| (p.label.as_str(), p.first, p.count))
        .collect();
    assert_eq!(pages, [("A", 2, 1), ("B", 3, 1), ("C", 4, 2)]);
}

#[test]
fn a_single_problem_has_no_cover() {
    let ws = fixture("contest-zh");
    let (compiled, report) = build(
        &ws,
        Target::Problem("contest-zh-a".into()),
        Kind::Statements,
    );
    assert!(report.ok, "{:?}", report.diagnostics);
    assert_eq!(compiled.unwrap().page_count(), 1);
    assert_eq!(report.problems[0].first, 1);
}

#[test]
fn errors_point_into_the_markdown() {
    // `\left(` without `\right` converts, but Typst cannot evaluate it.
    let (_dir, ws) = scratch("第一段。\n\n## 输入\n\n一行，$\\left( x$。\n");
    let (compiled, report) = build(&ws, Target::Problem("p".into()), Kind::Statements);
    assert!(compiled.is_none() && !report.ok);
    let d = &report.diagnostics[0];
    assert_eq!(
        (d.file.as_deref(), d.line, d.column),
        (Some("problems/p/statement.md"), Some(5), Some(4)),
        "{d:?}"
    );
}

#[test]
fn overflows_are_warnings_with_their_page() {
    let long = "x".repeat(200);
    let (_dir, ws) = scratch(&format!("看这一行：`{long}`\n"));
    let (compiled, report) = build(&ws, Target::Problem("p".into()), Kind::Statements);
    assert!(compiled.is_some());
    let d = report
        .diagnostics
        .iter()
        .find(|d| d.message.contains("right edge"))
        .expect("an overflow");
    assert_eq!((d.severity, d.page), ("warning", Some(1)));
    assert_eq!(d.file.as_deref(), Some("problems/p/statement.md"));
    // The fixtures fit.
    let ws = fixture("contest-en");
    let (_, report) = build(&ws, Target::Contest("contest-en".into()), Kind::Statements);
    assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
}

#[test]
fn pictures_from_the_problem() {
    let (dir, ws) = scratch("图：![一个点](assets/dot.svg)\n");
    fs::create_dir_all(dir.path().join("problems/p/assets")).unwrap();
    fs::write(
        dir.path().join("problems/p/assets/dot.svg"),
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><circle cx="5" cy="5" r="4"/></svg>"#,
    )
    .unwrap();
    let (_, report) = build(&ws, Target::Problem("p".into()), Kind::Statements);
    assert!(report.ok, "{:?}", report.diagnostics);
}

#[test]
fn the_editorial_book() {
    let (dir, ws) = scratch("描述。\n");
    fs::write(
        dir.path().join("problems/p/editorial.md"),
        "## 做法\n\n贪心，复杂度 $O(n)$。\n",
    )
    .unwrap();
    let (compiled, report) = build(&ws, Target::Problem("p".into()), Kind::Editorial);
    assert!(report.ok, "{:?}", report.diagnostics);
    assert_eq!(compiled.unwrap().page_count(), 1);
    assert_eq!(
        (report.problems[0].label.as_str(), report.problems[0].first),
        ("A", 1)
    );
    // A contest whose problems have none: warnings, and an empty book still compiles.
    let ws = fixture("contest-zh");
    let (_, report) = build(&ws, Target::Contest("contest-zh".into()), Kind::Editorial);
    assert_eq!(report.diagnostics.len(), 3);
    assert!(report.diagnostics.iter().all(|d| !d.is_error()));
}

#[test]
fn blank_pages_put_every_problem_on_an_odd_page() {
    let (dir, ws) = scratch("描述。\n");
    let c = dir.path().join("contests/c");
    fs::create_dir_all(&c).unwrap();
    fs::write(
        c.join("contest.toml"),
        "name = \"C\"\nblank-pages = true\n[[problem]]\nid = \"p\"\nlabel = \"A\"\n[[problem]]\nid = \"p\"\nlabel = \"B\"\n",
    )
    .unwrap();
    let (compiled, report) = build(&ws, Target::Contest("c".into()), Kind::Statements);
    assert!(report.ok, "{:?}", report.diagnostics);
    // Cover (page 0), A (1), blank (2), B (3), blank (4).
    assert_eq!(compiled.unwrap().page_count(), 5);
    let firsts: Vec<_> = report.problems.iter().map(|p| p.first).collect();
    assert_eq!(firsts, [2, 4], "{:?}", report.problems);
}

/// Every symbol of the bundled MiTeX scope, through the converter, evaluates in the
/// Typst we use: catches symbols Typst renamed (MiTeX's names are older).
#[test]
fn every_mitex_symbol_evaluates() {
    let spec = include_str!("../../xcpc-engine/templates/mitex/latex/standard.typ");
    let mut commands: Vec<&str> = spec
        .lines()
        .filter(|l| l.contains("define-sym(\""))
        .filter_map(|l| l.trim().split(':').next())
        .map(|k| k.trim_matches('"'))
        .filter(|k| k.chars().all(|c| c.is_ascii_alphabetic()))
        .collect();
    commands.sort();
    commands.dedup();
    assert!(commands.len() > 500, "{}", commands.len());
    let statement: String = commands
        .iter()
        .map(|c| format!("${{\\{c}}}$\n\n"))
        .collect();
    let (_dir, ws) = scratch(&statement);
    let (compiled, report) = build(&ws, Target::Problem("p".into()), Kind::Statements);
    let errors: Vec<_> = report.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(compiled.is_some() && errors.is_empty(), "{errors:#?}");
}

/// A table that fits on a page moves whole to the next one (the reference's way); one
/// taller than a page breaks across pages instead of running off it.
#[test]
fn samples_taller_than_a_page_break_across_pages() {
    let (dir, ws) = scratch("描述。\n\n## 输入格式\n\n很多行。\n\n## 输出格式\n\n很多行。\n");
    let long: String = (1..=120).map(|i| format!("{i} {}\n", i * 7)).collect();
    fs::write(dir.path().join("problems/p/samples/01.in"), &long).unwrap();
    fs::write(dir.path().join("problems/p/samples/01.ans"), &long).unwrap();
    let (compiled, report) = build(&ws, Target::Problem("p".into()), Kind::Statements);
    assert!(report.ok, "{:?}", report.diagnostics);
    let pages = compiled.unwrap().page_count();
    assert!(pages >= 3, "{pages}");
    assert!(
        report.diagnostics.is_empty(),
        "nothing runs off a page: {:?}",
        report.diagnostics
    );
}

/// A contest's editorial book: a title page with the contents, each editorial from a
/// new page, problems without one left out.
#[test]
fn a_contests_editorial_book_has_a_title_page_and_a_page_per_problem() {
    let (dir, ws) = scratch("描述。\n");
    for (id, editorial) in [
        ("q", Some("## 做法\n\n```cpp\nint main() {}\n```\n")),
        ("r", None),
        ("s", Some("> 引用。\n")),
    ] {
        let p = dir.path().join("problems").join(id);
        fs::create_dir_all(&p).unwrap();
        fs::write(
            p.join("problem.toml"),
            format!("title = \"{id}\"\ntags = [\"dp\"]\n"),
        )
        .unwrap();
        fs::write(p.join("statement.md"), "描述。\n").unwrap();
        if let Some(text) = editorial {
            fs::write(p.join("editorial.md"), text).unwrap();
        }
    }
    let c = dir.path().join("contests/c");
    fs::create_dir_all(&c).unwrap();
    fs::write(
        c.join("contest.toml"),
        "name = \"C\"\n[[problem]]\nid = \"q\"\nlabel = \"A\"\n[[problem]]\nid = \"r\"\nlabel = \"B\"\n[[problem]]\nid = \"s\"\nlabel = \"C\"\n",
    )
    .unwrap();
    let (compiled, report) = build(&ws, Target::Contest("c".into()), Kind::Editorial);
    assert!(compiled.is_some());
    assert_eq!(report.pages, Some(3), "{:?}", report.diagnostics);
    let pages: Vec<_> = report
        .problems
        .iter()
        .map(|p| (p.label.as_str(), p.first))
        .collect();
    assert_eq!(pages, [("A", 2), ("C", 3)]);
    assert_eq!(
        report.diagnostics.len(),
        1,
        "B has none: {:?}",
        report.diagnostics
    );
}

// ---- Phase 2 Wave 2A: lint + check + locate_point + freeze_contest ---------------------

const TINY_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24"><rect width="24" height="24"/></svg>"#;

fn write_problem(
    root: &Path,
    id: &str,
    title: &str,
    statement: &str,
    editorial: Option<&str>,
    samples: &[(&str, &str)],
) {
    let p = root.join("problems").join(id);
    fs::create_dir_all(p.join("samples")).unwrap();
    fs::write(
        p.join("problem.toml"),
        format!("title = \"{title}\"\n[limits]\ntime = \"1s\"\nmemory = \"256MB\"\n"),
    )
    .unwrap();
    fs::write(p.join("statement.md"), statement).unwrap();
    if let Some(ed) = editorial {
        fs::write(p.join("editorial.md"), ed).unwrap();
    }
    for (name, content) in samples {
        fs::write(p.join("samples").join(name), content).unwrap();
    }
}

fn empty_ws() -> (tempfile::TempDir, Workspace) {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("workspace.toml"), "").unwrap();
    let ws = Workspace::open(dir.path(), false).unwrap();
    (dir, ws)
}

#[test]
fn check_merges_lint_and_compile_without_duplicates_and_supports_unsaved() {
    let (dir, ws) = empty_ws();
    // Problem has:
    // - missing image on disk (line 3, col 1)
    // - external URL image (line 4, col 1)
    // - escaping image path (line 5, col 1)
    // - missing output section (warning)
    // - unpaired sample 02.in + non-contiguous samples 01, 03 (warnings)
    let stmt = "描述。\n\n![miss](assets/miss.svg)\n![ext](https://example.com/x.png)\n![esc](../other/x.png)\n\n## 输入格式\n\n给定整数 $n$。\n";
    write_problem(
        dir.path(),
        "p",
        "P",
        stmt,
        None,
        &[
            ("01.in", "1\n"),
            ("01.ans", "1\n"),
            ("03.in", "3\n"),
            ("03.ans", "3\n"),
            ("04.in", "4\n"),
        ],
    );

    let mut engine = publish::engine(&ws, &FontOptions::from_env());
    let (compiled, doc, report) = publish::check(
        &ws,
        &mut engine,
        &Target::Problem("p".into()),
        Kind::Statements,
        None,
        None,
    )
    .unwrap();
    assert!(compiled.is_none());
    assert!(doc.is_some());
    assert!(!report.ok);

    // Exactly 3 missing_image errors (no duplicates from markdown.rs or Typst file-not-found).
    let img_errs: Vec<_> = report
        .diagnostics
        .iter()
        .filter(|d| d.is_error() && d.code.as_deref() == Some(xcpc_lint::code::MISSING_IMAGE))
        .collect();
    assert_eq!(img_errs.len(), 3, "{:#?}", report.diagnostics);
    let mut locs: Vec<_> = img_errs
        .iter()
        .map(|d| (d.file.as_deref(), d.line, d.column))
        .collect();
    locs.sort();
    assert_eq!(
        locs,
        vec![
            (Some("problems/p/statement.md"), Some(3), Some(1)),
            (Some("problems/p/statement.md"), Some(4), Some(1)),
            (Some("problems/p/statement.md"), Some(5), Some(1)),
        ]
    );
    // No duplicate raw Typst `file not found` errors alongside `missing_image`.
    assert!(
        !report
            .diagnostics
            .iter()
            .any(|d| d.code.is_none() && d.message.contains("file not found")),
        "{:#?}",
        report.diagnostics
    );

    // Warnings: missing output section (1), unpaired 04.in (1), non-contiguous (1),
    // and NO missing_editorial when checking Statements!
    assert_eq!(
        report
            .diagnostics
            .iter()
            .filter(|d| d.code.as_deref() == Some(xcpc_lint::code::MISSING_IO_SECTION))
            .count(),
        1
    );
    assert_eq!(
        report
            .diagnostics
            .iter()
            .filter(|d| d.code.as_deref() == Some(xcpc_lint::code::SAMPLE_UNPAIRED))
            .count(),
        1
    );
    assert_eq!(
        report
            .diagnostics
            .iter()
            .filter(|d| d.code.as_deref() == Some(xcpc_lint::code::SAMPLE_NON_CONTIGUOUS))
            .count(),
        1
    );
    assert_eq!(
        report
            .diagnostics
            .iter()
            .filter(|d| d.code.as_deref() == Some(xcpc_lint::code::MISSING_EDITORIAL))
            .count(),
        0
    );

    // JSON serialization includes `code` for lint diagnostics and omits it when `None`.
    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["ok"], false);
    assert!(
        json["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "missing_image")
    );

    // Unsaved overlay fixes all image errors and adds `## 交互协议` (exempting missing I/O).
    let (compiled_live, _doc_live, report_live) = publish::check(
        &ws,
        &mut engine,
        &Target::Problem("p".into()),
        Kind::Statements,
        None,
        Some((
            "problems/p/statement.md",
            "描述。\n\n## 交互协议\n\n交互。\n",
        )),
    )
    .unwrap();
    assert!(compiled_live.is_some());
    assert!(report_live.ok, "{:#?}", report_live.diagnostics);
    assert!(
        !report_live
            .diagnostics
            .iter()
            .any(|d| d.code.as_deref() == Some(xcpc_lint::code::MISSING_IO_SECTION)),
        "{:#?}",
        report_live.diagnostics
    );
    assert!(
        !report_live
            .diagnostics
            .iter()
            .any(|d| d.code.as_deref() == Some(xcpc_lint::code::MISSING_IMAGE))
    );

    // Unsaved overlay introducing a missing image on a clean problem triggers live error.
    let (_c2, _d2, report_bad_live) = publish::check(
        &ws,
        &mut engine,
        &Target::Problem("p".into()),
        Kind::Statements,
        None,
        Some((
            "problems/p/statement.md",
            "## 输入\nx\n## 输出\n![live](assets/not-there.png)\n",
        )),
    )
    .unwrap();
    assert!(!report_bad_live.ok);
    let live_img: Vec<_> = report_bad_live
        .diagnostics
        .iter()
        .filter(|d| d.code.as_deref() == Some(xcpc_lint::code::MISSING_IMAGE))
        .collect();
    assert_eq!(live_img.len(), 1, "{:#?}", report_bad_live.diagnostics);
    assert_eq!((live_img[0].line, live_img[0].column), (Some(4), Some(1)));

    // Checking Kind::Editorial reports missing_editorial ONCE (not duplicated with editorials()).
    let (_ce, _de, report_ed) = publish::check(
        &ws,
        &mut engine,
        &Target::Problem("p".into()),
        Kind::Editorial,
        None,
        Some(("problems/p/statement.md", "## 输入\nx\n## 输出\ny\n")),
    )
    .unwrap();
    let missing_ed: Vec<_> = report_ed
        .diagnostics
        .iter()
        .filter(|d| d.code.as_deref() == Some(xcpc_lint::code::MISSING_EDITORIAL))
        .collect();
    assert_eq!(missing_ed.len(), 1, "{:#?}", report_ed.diagnostics);
    assert!(!missing_ed[0].is_error());
}

#[test]
fn check_handles_missing_target_and_contest_missing_problem_gracefully() {
    let (dir, ws) = empty_ws();
    write_problem(dir.path(), "a", "A", "## Input\nx\n", None, &[]); // missing output
    write_problem(
        dir.path(),
        "c",
        "C",
        "## Input\n![m](assets/missing.png)\n## Output\ny\n",
        None,
        &[],
    );
    let cdir = dir.path().join("contests/broken");
    fs::create_dir_all(&cdir).unwrap();
    fs::write(
        cdir.join("contest.toml"),
        "name = \"Broken\"\n\
         [[problem]]\nid = \"a\"\nlabel = \"A\"\n\
         [[problem]]\nid = \"missing-one\"\nlabel = \"B\"\n\
         [[problem]]\nid = \"c\"\nlabel = \"C\"\n",
    )
    .unwrap();

    let mut engine = publish::engine(&ws, &FontOptions::from_env());

    // Missing target itself still returns Err(NotFound).
    assert!(matches!(
        publish::check(
            &ws,
            &mut engine,
            &Target::Problem("does-not-exist".into()),
            Kind::Statements,
            None,
            None,
        ),
        Err(StorageError::NotFound(_))
    ));
    assert!(matches!(
        publish::check(
            &ws,
            &mut engine,
            &Target::Contest("does-not-exist".into()),
            Kind::Statements,
            None,
            None,
        ),
        Err(StorageError::NotFound(_))
    ));

    // Contest with an internal missing problem does NOT fail with NotFound:
    // returns Ok((None, None, report)) with missing_problem AND lint results for A & C.
    let (compiled, doc, report) = publish::check(
        &ws,
        &mut engine,
        &Target::Contest("broken".into()),
        Kind::Statements,
        None,
        None,
    )
    .unwrap();
    assert!(compiled.is_none());
    assert!(doc.is_none());
    assert!(!report.ok);
    assert!(report.diagnostics.iter().any(|d| {
        d.is_error()
            && d.code.as_deref() == Some(xcpc_lint::code::MISSING_PROBLEM)
            && d.message.contains("missing-one")
    }));
    assert!(report.diagnostics.iter().any(|d| {
        !d.is_error()
            && d.code.as_deref() == Some(xcpc_lint::code::MISSING_IO_SECTION)
            && d.file.as_deref() == Some("problems/a/statement.md")
    }));
    assert!(report.diagnostics.iter().any(|d| {
        d.is_error()
            && d.code.as_deref() == Some(xcpc_lint::code::MISSING_IMAGE)
            && d.file.as_deref() == Some("problems/c/statement.md")
    }));
}

// ---- Frame coordinate helpers for publish::locate_point tests --------------------------

struct RenderedText {
    text: String,
    bounds: [f64; 4],
}

fn transformed_rect(r: [f64; 4], ts: Transform) -> [f64; 4] {
    let mut out = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for (x, y) in [(r[0], r[1]), (r[2], r[1]), (r[0], r[3]), (r[2], r[3])] {
        let p = Point::new(Abs::pt(x), Abs::pt(y)).transform_inf(ts);
        out = [
            out[0].min(p.x.to_pt()),
            out[1].min(p.y.to_pt()),
            out[2].max(p.x.to_pt()),
            out[3].max(p.y.to_pt()),
        ];
    }
    out
}

fn collect_texts(frame: &Frame, ts: Transform, out: &mut Vec<RenderedText>) {
    for (pos, item) in frame.items() {
        let at = ts.pre_concat(Transform::translate(pos.x, pos.y));
        match item {
            FrameItem::Group(group) => {
                collect_texts(&group.frame, at.pre_concat(group.transform), out);
            }
            FrameItem::Text(text) => {
                let size = text.size;
                let width: Abs = text.glyphs.iter().map(|g| g.x_advance.at(size)).sum();
                let metrics = text.font.metrics();
                let asc = metrics.ascender.at(size).to_pt();
                let desc = metrics.descender.at(size).to_pt();
                let local = [0.0, -asc, width.to_pt(), -desc];
                out.push(RenderedText {
                    text: text.text.to_string(),
                    bounds: transformed_rect(local, at),
                });
            }
            _ => {}
        }
    }
}

fn collect_images(frame: &Frame, ts: Transform, out: &mut Vec<[f64; 4]>) {
    for (pos, item) in frame.items() {
        let at = ts.pre_concat(Transform::translate(pos.x, pos.y));
        match item {
            FrameItem::Group(group) => {
                collect_images(&group.frame, at.pre_concat(group.transform), out);
            }
            FrameItem::Image(_, size, _) => {
                out.push(transformed_rect(
                    [0.0, 0.0, size.x.to_pt(), size.y.to_pt()],
                    at,
                ));
            }
            _ => {}
        }
    }
}

fn find_text_center(compiled: &Compiled, page: usize, needle: &str) -> (f64, f64) {
    let mut texts = Vec::new();
    collect_texts(
        &compiled.document.pages()[page].frame,
        Transform::identity(),
        &mut texts,
    );
    let hit = texts
        .into_iter()
        .find(|t| t.text.contains(needle))
        .unwrap_or_else(|| panic!("text {needle:?} not found on page {page}"));
    (
        0.5 * (hit.bounds[0] + hit.bounds[2]),
        0.5 * (hit.bounds[1] + hit.bounds[3]),
    )
}

#[test]
fn locate_point_maps_preview_clicks_back_to_exact_markdown_locations() {
    let (dir, ws) = empty_ws();
    let stmt_a = "\
AlphaLegendLine\n\
\n\
## Input\n\
\n\
AlphaInputLine with $x + y$ inline.\n\
\n\
## Output\n\
\n\
AlphaOutputLine\n\
\n\
![dot](assets/dot.svg)\n";
    let stmt_b = "\
BetaLegendLine\n\
\n\
## Input\n\
\n\
BetaInputLine\n\
\n\
## Output\n\
\n\
BetaOutputLine\n";
    write_problem(dir.path(), "pa", "TitleAlpha", stmt_a, None, &[]);
    write_problem(dir.path(), "pb", "TitleBeta", stmt_b, None, &[]);
    fs::create_dir_all(dir.path().join("problems/pa/assets")).unwrap();
    fs::write(dir.path().join("problems/pa/assets/dot.svg"), TINY_SVG).unwrap();

    let cdir = dir.path().join("contests/c");
    fs::create_dir_all(&cdir).unwrap();
    fs::write(
        cdir.join("contest.toml"),
        "name = \"LocateContest\"\nlanguage = \"english\"\n\
         [[problem]]\nid = \"pa\"\nlabel = \"A\"\n\
         [[problem]]\nid = \"pb\"\nlabel = \"B\"\n",
    )
    .unwrap();

    let mut engine = publish::engine(&ws, &FontOptions::from_env());
    let (compiled, doc, report) = publish::check(
        &ws,
        &mut engine,
        &Target::Contest("c".into()),
        Kind::Statements,
        None,
        None,
    )
    .unwrap();
    assert!(report.ok, "{:#?}", report.diagnostics);
    let compiled = compiled.unwrap();
    let doc = doc.unwrap();
    // Page 0: cover, Page 1: Problem A, Page 2: Problem B.
    assert_eq!(compiled.page_count(), 3);

    // 1. Clicking on Problem A's legend, input line, output line, and image.
    let (x, y) = find_text_center(&compiled, 1, "AlphaLegendLine");
    assert_eq!(
        publish::locate_point(&engine, &compiled, &doc, 1, x, y),
        Some(SourceLocation {
            file: "problems/pa/statement.md".into(),
            line: 1,
            column: 1,
        })
    );
    let (x, y) = find_text_center(&compiled, 1, "AlphaInputLine");
    assert_eq!(
        publish::locate_point(&engine, &compiled, &doc, 1, x, y),
        Some(SourceLocation {
            file: "problems/pa/statement.md".into(),
            line: 5,
            column: 1,
        })
    );
    // Clicking on inline math `$x + y$` on line 5 resolves back to line 5 of `problems/pa/statement.md`.
    let (mx, my) = find_text_center(&compiled, 1, "+");
    let math_loc = publish::locate_point(&engine, &compiled, &doc, 1, mx, my).expect("math hit");
    assert_eq!(math_loc.file, "problems/pa/statement.md");
    assert_eq!(math_loc.line, 5);
    let (x, y) = find_text_center(&compiled, 1, "AlphaOutputLine");
    assert_eq!(
        publish::locate_point(&engine, &compiled, &doc, 1, x, y),
        Some(SourceLocation {
            file: "problems/pa/statement.md".into(),
            line: 9,
            column: 1,
        })
    );

    let mut imgs = Vec::new();
    collect_images(
        &compiled.document.pages()[1].frame,
        Transform::identity(),
        &mut imgs,
    );
    assert_eq!(imgs.len(), 1);
    let img_center = (
        0.5 * (imgs[0][0] + imgs[0][2]),
        0.5 * (imgs[0][1] + imgs[0][3]),
    );
    assert_eq!(
        publish::locate_point(&engine, &compiled, &doc, 1, img_center.0, img_center.1),
        Some(SourceLocation {
            file: "problems/pa/statement.md".into(),
            line: 11,
            column: 1,
        })
    );

    // 2. Clicking on Problem B's text maps to `problems/pb/statement.md`.
    let (x, y) = find_text_center(&compiled, 2, "BetaOutputLine");
    assert_eq!(
        publish::locate_point(&engine, &compiled, &doc, 2, x, y),
        Some(SourceLocation {
            file: "problems/pb/statement.md".into(),
            line: 9,
            column: 1,
        })
    );

    // 3. Clicking on cover text or Problem B's generated title header does NOT leak back to Problem A.
    let (cx, cy) = find_text_center(&compiled, 0, "LocateContest");
    assert_eq!(
        publish::locate_point(&engine, &compiled, &doc, 0, cx, cy),
        None
    );
    let (tx, ty) = find_text_center(&compiled, 2, "TitleBeta");
    assert_eq!(
        publish::locate_point(&engine, &compiled, &doc, 2, tx, ty),
        None
    );
}

#[test]
fn freeze_contest_rejects_errors_without_touching_disk_and_respects_guards() {
    let (dir, ws) = empty_ws();
    write_problem(
        dir.path(),
        "a",
        "A",
        "描述。\n\n## 输入\n\n![miss](assets/miss.svg)\n\n## 输出\n\n答案。\n",
        None,
        &[("01.in", "1\n"), ("01.ans", "1\n")],
    );
    let cdir = dir.path().join("contests/c");
    fs::create_dir_all(&cdir).unwrap();
    fs::write(
        cdir.join("contest.toml"),
        "name = \"FreezeTest\"\n[[problem]]\nid = \"a\"\nlabel = \"A\"\n",
    )
    .unwrap();

    let mut engine = publish::engine(&ws, &FontOptions::from_env());

    // Read-only workspace is rejected.
    let ro_ws = Workspace::open(dir.path(), true).unwrap();
    assert!(matches!(
        publish::freeze_contest(&ro_ws, &mut engine, "c"),
        Err(StorageError::ReadOnly)
    ));
    // Non-existent contest returns NotFound.
    assert!(matches!(
        publish::freeze_contest(&ws, &mut engine, "no-such-contest"),
        Err(StorageError::NotFound(_))
    ));

    // 1. Missing image blocks freeze without writing snapshot/ or flipping frozen.
    let res = publish::freeze_contest(&ws, &mut engine, "c").unwrap();
    assert!(!res.frozen);
    assert!(!res.report.ok);
    assert!(res.outputs.is_empty());
    assert!(
        res.report
            .diagnostics
            .iter()
            .any(|d| d.is_error() && d.code.as_deref() == Some(xcpc_lint::code::MISSING_IMAGE))
    );
    assert!(!cdir.join("snapshot").exists());
    assert!(!ws.load_contest("c").unwrap().meta.frozen);

    // 2. Fix image on disk, introduce statement compile error -> still blocked, no disk write.
    fs::create_dir_all(dir.path().join("problems/a/assets")).unwrap();
    fs::write(dir.path().join("problems/a/assets/miss.svg"), TINY_SVG).unwrap();
    fs::write(
        dir.path().join("problems/a/statement.md"),
        "## 输入\n\n坏公式 $\\left( x$。\n\n## 输出\n\n答案。\n",
    )
    .unwrap();
    let res = publish::freeze_contest(&ws, &mut engine, "c").unwrap();
    assert!(!res.frozen);
    assert!(!res.report.ok);
    assert!(res.outputs.is_empty());
    assert!(!cdir.join("snapshot").exists());

    // 3. Fix statement, add broken editorial.md -> statements pass, editorial error blocks freeze!
    fs::write(
        dir.path().join("problems/a/statement.md"),
        "## 输入\n\n![ok](assets/miss.svg)\n\n## 输出\n\n答案。\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("problems/a/editorial.md"),
        "## 做法\n\n坏题解公式 $\\left( x$。\n",
    )
    .unwrap();
    let res = publish::freeze_contest(&ws, &mut engine, "c").unwrap();
    assert!(!res.frozen);
    assert!(!res.report.ok);
    assert!(res.outputs.is_empty());
    assert!(
        res.report
            .diagnostics
            .iter()
            .any(|d| { d.is_error() && d.file.as_deref() == Some("problems/a/editorial.md") })
    );
    assert!(!cdir.join("snapshot").exists());
    assert!(!ws.load_contest("c").unwrap().meta.frozen);

    // 4. Contest with missing referenced problem blocks freeze cleanly (no NotFound panic/err).
    fs::write(
        cdir.join("contest.toml"),
        "name = \"FreezeTest\"\n[[problem]]\nid = \"a\"\nlabel = \"A\"\n[[problem]]\nid = \"ghost\"\nlabel = \"B\"\n",
    )
    .unwrap();
    let res = publish::freeze_contest(&ws, &mut engine, "c").unwrap();
    assert!(!res.frozen);
    assert!(!res.report.ok);
    assert!(
        res.report.diagnostics.iter().any(|d| {
            d.is_error() && d.code.as_deref() == Some(xcpc_lint::code::MISSING_PROBLEM)
        })
    );
    assert!(!cdir.join("snapshot").exists());

    // 5. Fix editorial & remove ghost -> freezing with the SAME engine succeeds (verifies reset_files).
    fs::write(
        cdir.join("contest.toml"),
        "name = \"FreezeTest\"\n[[problem]]\nid = \"a\"\nlabel = \"A\"\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("problems/a/editorial.md"),
        "## 做法\n\n直接做。\n",
    )
    .unwrap();
    let res = publish::freeze_contest(&ws, &mut engine, "c").unwrap();
    assert!(res.frozen, "{:#?}", res.report.diagnostics);
    assert!(res.report.ok);

    // Freezing again returns AlreadyFrozen.
    assert!(matches!(
        publish::freeze_contest(&ws, &mut engine, "c"),
        Err(StorageError::AlreadyFrozen(_))
    ));
}

#[test]
fn freeze_contest_succeeds_with_warnings_outputs_pdfs_excludes_package_files_and_isolates_snapshot()
{
    let (dir, ws) = empty_ws();
    // Problem A: has editorial, valid asset, plus solutions/ and tests/ that MUST NOT enter snapshot.
    // Also has warning: unpaired sample `02.in`.
    write_problem(
        dir.path(),
        "a",
        "Original A",
        "原始题面 A。\n\n![ok](assets/fig.svg)\n\n## 输入\n\nx\n\n## 输出\n\ny\n",
        Some("## 做法\n\n贪心。\n"),
        &[("01.in", "1\n"), ("01.ans", "2\n"), ("02.in", "orphan\n")],
    );
    fs::create_dir_all(dir.path().join("problems/a/assets")).unwrap();
    fs::write(dir.path().join("problems/a/assets/fig.svg"), TINY_SVG).unwrap();
    fs::create_dir_all(dir.path().join("problems/a/solutions")).unwrap();
    fs::write(
        dir.path().join("problems/a/solutions/std.cpp"),
        "int main(){}\n",
    )
    .unwrap();
    fs::create_dir_all(dir.path().join("problems/a/tests")).unwrap();
    fs::write(dir.path().join("problems/a/tests/01.in"), "secret\n").unwrap();

    // Problem B: NO editorial (warning `missing_editorial`) and missing output section (warning).
    write_problem(
        dir.path(),
        "b",
        "Original B",
        "原始题面 B。\n\n## 输入\n\n输入。\n",
        None,
        &[("01.in", "3\n"), ("01.ans", "4\n")],
    );

    let cdir = dir.path().join("contests/with-ed");
    fs::create_dir_all(&cdir).unwrap();
    fs::write(
        cdir.join("contest.toml"),
        "name = \"With Editorial\"\n\
         [[problem]]\nid = \"a\"\nlabel = \"A\"\ntitle = \"Overridden A\"\n\
         [[problem]]\nid = \"b\"\nlabel = \"B\"\n",
    )
    .unwrap();

    let mut engine = publish::engine(&ws, &FontOptions::from_env());
    let outcome = publish::freeze_contest(&ws, &mut engine, "with-ed").unwrap();
    assert!(outcome.frozen, "{:#?}", outcome.report.diagnostics);
    assert!(outcome.report.ok);
    assert_eq!(
        outcome.outputs,
        vec![
            "contests/with-ed/snapshot/with-ed.pdf",
            "contests/with-ed/snapshot/with-ed-editorial.pdf",
        ]
    );

    // Warnings were collected (not blocking) and missing_editorial was NOT duplicated!
    assert_eq!(
        outcome
            .report
            .diagnostics
            .iter()
            .filter(|d| d.code.as_deref() == Some(xcpc_lint::code::MISSING_EDITORIAL))
            .count(),
        1,
        "{:#?}",
        outcome.report.diagnostics
    );
    assert!(
        outcome
            .report
            .diagnostics
            .iter()
            .any(|d| d.code.as_deref() == Some(xcpc_lint::code::SAMPLE_UNPAIRED))
    );
    assert!(
        outcome
            .report
            .diagnostics
            .iter()
            .any(|d| d.code.as_deref() == Some(xcpc_lint::code::MISSING_IO_SECTION))
    );

    // Both PDFs exist and are valid PDF files.
    let stmts_pdf = fs::read(cdir.join("snapshot/with-ed.pdf")).unwrap();
    let ed_pdf = fs::read(cdir.join("snapshot/with-ed-editorial.pdf")).unwrap();
    assert!(stmts_pdf.starts_with(b"%PDF-"));
    assert!(ed_pdf.starts_with(b"%PDF-"));

    // Snapshot excludes solutions/ and tests/, includes assets/ and merged problem.toml.
    assert!(cdir.join("snapshot/A/assets/fig.svg").is_file());
    assert!(cdir.join("snapshot/A/editorial.md").is_file());
    assert!(!cdir.join("snapshot/A/solutions").exists());
    assert!(!cdir.join("snapshot/A/tests").exists());
    assert!(
        fs::read_to_string(cdir.join("snapshot/A/problem.toml"))
            .unwrap()
            .contains("Overridden A")
    );

    // Modify library problem A (even introducing a broken image) and delete library problem B!
    fs::write(
        dir.path().join("problems/a/statement.md"),
        "已修改！![broken](assets/broken.png)\n",
    )
    .unwrap();
    ws.delete_problem("b").unwrap();

    // Frozen contest still checks & compiles identically from snapshot/.
    let (compiled_after, doc_after, report_after) = publish::check(
        &ws,
        &mut engine,
        &Target::Contest("with-ed".into()),
        Kind::Statements,
        None,
        None,
    )
    .unwrap();
    assert!(compiled_after.is_some());
    assert!(report_after.ok, "{:#?}", report_after.diagnostics);
    let src = &doc_after.unwrap().source;
    assert!(src.contains("原始题面 A"));
    assert!(src.contains("原始题面 B"));
    assert!(!src.contains("已修改"));

    // A contest where NO problem has `editorial.md` generates ONLY statements PDF.
    write_problem(
        dir.path(),
        "noed",
        "NoEd",
        "## Input\nx\n## Output\ny\n",
        None,
        &[],
    );
    let c2 = dir.path().join("contests/no-ed");
    fs::create_dir_all(&c2).unwrap();
    fs::write(
        c2.join("contest.toml"),
        "name = \"No Editorial\"\n[[problem]]\nid = \"noed\"\nlabel = \"A\"\n",
    )
    .unwrap();
    let outcome2 = publish::freeze_contest(&ws, &mut engine, "no-ed").unwrap();
    assert!(outcome2.frozen);
    assert_eq!(outcome2.outputs, vec!["contests/no-ed/snapshot/no-ed.pdf"]);
    assert!(c2.join("snapshot/no-ed.pdf").is_file());
    assert!(!c2.join("snapshot/no-ed-editorial.pdf").exists());
}

#[test]
fn pdf_metadata_sets_title_author_creator_and_preserves_producer() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("workspace.toml"), "").unwrap();

    let p = dir.path().join("problems/alpha");
    fs::create_dir_all(&p).unwrap();
    fs::write(
        p.join("problem.toml"),
        "title = \"灯笼小径\"\nauthor = \"chumeng\"\n",
    )
    .unwrap();
    fs::write(
        p.join("statement.md"),
        "正文。\n\n## 输入格式\n\n一个数。\n\n## 输出格式\n\n一个数。\n",
    )
    .unwrap();
    fs::write(p.join("editorial.md"), "## 思路\n\n直接输出。\n").unwrap();

    let c = dir.path().join("contests/spring");
    fs::create_dir_all(&c).unwrap();
    fs::write(
        c.join("contest.toml"),
        "name = \"春季训练赛\"\nauthor = \"命题组\"\nlanguage = \"chinese\"\n[[problem]]\nid = \"alpha\"\nlabel = \"A\"\n",
    )
    .unwrap();

    let ws = Workspace::open(dir.path(), false).unwrap();
    let mut engine = publish::engine(&ws, &FontOptions::from_env());

    // 1. Contest statements PDF
    let doc = publish::document(
        &ws,
        &Target::Contest("spring".into()),
        Kind::Statements,
        None,
    )
    .unwrap();
    let (compiled, report) = publish::compile(&mut engine, &doc);
    assert!(report.ok, "{:#?}", report.diagnostics);
    let pdf = compiled.unwrap().pdf().unwrap();
    let text = String::from_utf8_lossy(&pdf);
    assert!(
        text.contains("春季训练赛 试题册"),
        "missing contest statements title in PDF"
    );
    assert!(text.contains("命题组"), "missing contest author in PDF");
    assert!(
        text.contains(&format!("XCPC Workstation {}", env!("CARGO_PKG_VERSION"))),
        "missing Creator in PDF"
    );
    assert!(
        text.contains("dc:title") && text.contains("dc:creator"),
        "expected XMP title/author metadata in PDF"
    );

    // 2. Contest editorial PDF
    let ed_doc = publish::document(
        &ws,
        &Target::Contest("spring".into()),
        Kind::Editorial,
        None,
    )
    .unwrap();
    let (ed_compiled, ed_report) = publish::compile(&mut engine, &ed_doc);
    assert!(ed_report.ok, "{:#?}", ed_report.diagnostics);
    let ed_pdf = ed_compiled.unwrap().pdf().unwrap();
    let ed_text = String::from_utf8_lossy(&ed_pdf);
    assert!(
        ed_text.contains("春季训练赛 题解"),
        "missing contest editorial title in PDF"
    );
    assert!(
        ed_text.contains("命题组"),
        "missing contest author in editorial PDF"
    );

    // 3. Single problem statements PDF
    let p_doc = publish::document(
        &ws,
        &Target::Problem("alpha".into()),
        Kind::Statements,
        None,
    )
    .unwrap();
    let (p_compiled, p_report) = publish::compile(&mut engine, &p_doc);
    assert!(p_report.ok, "{:#?}", p_report.diagnostics);
    let p_pdf = p_compiled.unwrap().pdf().unwrap();
    let p_text = String::from_utf8_lossy(&p_pdf);
    assert!(
        p_text.contains("A. 灯笼小径"),
        "missing problem title in PDF"
    );
    assert!(p_text.contains("chumeng"), "missing problem author in PDF");
}

#[test]
fn default_output_uses_sanitized_titles_with_id_fallback() {
    assert_eq!(
        publish::sanitize_filename("  A / B: *?\"<>|  "),
        "A - B- ------"
    );
    assert_eq!(publish::sanitize_filename("   ...  "), "");
    assert_eq!(publish::sanitize_filename("CON"), "");
    assert_eq!(publish::sanitize_filename("nul.txt"), "");
    assert_eq!(publish::sanitize_filename("LPT1"), "");
    assert!(publish::sanitize_filename(&"a".repeat(300)).len() <= 160);
    assert!(publish::sanitize_filename(&"测".repeat(200)).len() <= 160);

    assert_eq!(
        publish::default_output_for_title(
            &Target::Contest("spring".into()),
            Kind::Statements,
            Some("春季训练赛"),
        ),
        "build/春季训练赛-试题册.pdf"
    );
    assert_eq!(
        publish::default_output_for_title(
            &Target::Contest("spring".into()),
            Kind::Editorial,
            Some("春季训练赛"),
        ),
        "build/春季训练赛-题解.pdf"
    );
    assert_eq!(
        publish::default_output_for_title(
            &Target::Problem("alpha".into()),
            Kind::Statements,
            Some("灯笼小径"),
        ),
        "build/灯笼小径.pdf"
    );
    assert_eq!(
        publish::default_output_for_title(
            &Target::Problem("alpha".into()),
            Kind::Statements,
            Some("   "),
        ),
        "build/alpha.pdf"
    );
    assert_eq!(
        publish::default_output_for_title(
            &Target::Contest("empty-name".into()),
            Kind::Statements,
            Some("///"),
        ),
        "build/empty-name-试题册.pdf"
    );
}

#[test]
fn raw_typst_block_passthrough_and_image_width_attribute() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("workspace.toml"), "").unwrap();

    let p = dir.path().join("problems/diagram");
    fs::create_dir_all(p.join("assets")).unwrap();
    fs::write(p.join("problem.toml"), "title = \"图示测试\"\n").unwrap();
    fs::write(
        p.join("assets/box.svg"),
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="40"><rect width="120" height="40" fill="#ddd"/></svg>"##,
    )
    .unwrap();
    let statement = concat!(
        "请看下图：\n\n",
        "![图示](assets/box.svg){width=60%}\n\n",
        "```typst\n",
        "#align(center, rect(width: 40%, stroke: 0.5pt)[Typst 直出块])\n",
        "```\n\n",
        "非法范围不吞掉：![原样](assets/box.svg){width=5%}\n\n",
        "## 输入格式\n\n无。\n\n## 输出格式\n\n无。\n",
    );
    fs::write(p.join("statement.md"), statement).unwrap();

    let ws = Workspace::open(dir.path(), false).unwrap();
    let doc = publish::document(
        &ws,
        &Target::Problem("diagram".into()),
        Kind::Statements,
        None,
    )
    .unwrap();

    assert!(
        doc.source.contains(
            "#align(center, image(\"/problems/diagram/assets/box.svg\", alt: \"图示\", width: 60%))"
        ),
        "expected #align(center, image(..., width: 60%)) in:\n{}",
        doc.source
    );
    assert!(
        doc.source
            .contains("#align(center, rect(width: 40%, stroke: 0.5pt)[Typst 直出块])"),
        "expected raw typst block passthrough in:\n{}",
        doc.source
    );
    assert!(!doc.source.contains("lang: \"typst\""));
    // Out-of-range `{width=5%}` falls back to inline `#box(image(...));` + literal text.
    assert!(
        doc.source
            .contains("#box(image(\"/problems/diagram/assets/box.svg\", alt: \"原样\"));")
    );

    // SourceMap points lines inside the `typst` block back to their exact line in `statement.md`.
    let typst_offset = doc.source.find("#align(center, rect").unwrap();
    let (file, line, _) = doc.map.locate(typst_offset).unwrap();
    assert_eq!(file, "problems/diagram/statement.md");
    assert_eq!(line, 6);

    let mut engine = publish::engine(&ws, &FontOptions::from_env());
    let (compiled, report) = publish::compile(&mut engine, &doc);
    assert!(compiled.is_some());
    assert!(report.ok, "{:#?}", report.diagnostics);
}

#[test]
fn embedded_example_workspace_compiles_cleanly_for_statements_and_editorial() {
    let dir = tempfile::tempdir().unwrap();
    let ws = xcpc_storage::init_example_workspace(dir.path()).unwrap();
    assert_eq!(
        ws.list_problems().unwrap(),
        vec!["lantern-walk", "star-grid"]
    );
    assert_eq!(ws.list_contests().unwrap(), vec!["welcome-round"]);

    let mut engine = publish::engine(&ws, &FontOptions::from_env());
    for (target, kind) in [
        (Target::Contest("welcome-round".into()), Kind::Statements),
        (Target::Contest("welcome-round".into()), Kind::Editorial),
        (Target::Problem("lantern-walk".into()), Kind::Statements),
        (Target::Problem("star-grid".into()), Kind::Statements),
    ] {
        let (compiled, _, report) =
            publish::check(&ws, &mut engine, &target, kind, None, None).unwrap();
        assert!(compiled.is_some());
        assert!(
            report.ok && report.diagnostics.is_empty(),
            "example workspace check had diagnostics for {target:?} {kind:?}: {:#?}",
            report.diagnostics
        );
    }
}

#[test]
fn custom_template_cnoi_and_workspace_preamble_compile_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let ws = xcpc_storage::init_example_workspace(dir.path()).unwrap();

    // 1. Switch workspace default template to built-in `cnoi-classic`.
    fs::write(
        dir.path().join("workspace.toml"),
        "template = \"cnoi-classic\"\n",
    )
    .unwrap();

    // 2. Add `.xcpc/preamble.typ` defining a custom macro & style tweak.
    fs::create_dir_all(dir.path().join(".xcpc")).unwrap();
    fs::write(
        dir.path().join(".xcpc/preamble.typ"),
        "#let xcpc-custom-badge(s) = strong(s)\n#set text(tracking: 0pt)\n",
    )
    .unwrap();

    let mut engine = publish::engine(&ws, &FontOptions::from_env());
    let (compiled, doc, report) = publish::check(
        &ws,
        &mut engine,
        &Target::Contest("welcome-round".into()),
        Kind::Statements,
        None,
        None,
    )
    .unwrap();
    assert!(report.ok, "cnoi-classic failed: {:#?}", report.diagnostics);
    assert!(compiled.is_some());
    let doc = doc.unwrap();
    assert!(doc.source.contains("\"/xcpc/cnoi-classic.typ\""));
    assert!(doc.source.contains("xcpc-custom-badge"));

    // 3. Custom workspace `templates/my-oi.typ` takes precedence when selected.
    fs::create_dir_all(dir.path().join("templates")).unwrap();
    fs::write(
        dir.path().join("templates/my-oi.typ"),
        "#import \"/xcpc/cnoi-classic.typ\": *\n",
    )
    .unwrap();
    fs::write(dir.path().join("workspace.toml"), "template = \"my-oi\"\n").unwrap();
    let (compiled, doc, report) = publish::check(
        &ws,
        &mut engine,
        &Target::Problem("lantern-walk".into()),
        Kind::Statements,
        None,
        None,
    )
    .unwrap();
    assert!(compiled.is_some());
    assert!(report.ok, "custom template failed: {:#?}", report.diagnostics);
    assert!(doc.unwrap().source.contains("\"/templates/my-oi.typ\""));
}
