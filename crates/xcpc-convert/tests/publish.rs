//! The pipeline end to end: workspace → Typst → pages, with the statement fonts
//! (run in the dev shell, which sets XCPC_FONT_DIRS).

use std::fs;
use std::path::Path;

use xcpc_convert::publish::{self, Kind, Report, Target};
use xcpc_engine::FontOptions;
use xcpc_storage::Workspace;

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
