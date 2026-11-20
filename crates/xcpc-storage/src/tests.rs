use super::*;

fn workspace() -> (tempfile::TempDir, Workspace) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join(WORKSPACE_FILE), "").unwrap();
    let p = root.join("problems/two-sum");
    fs::create_dir_all(p.join("samples")).unwrap();
    fs::write(
        p.join("problem.toml"),
        "title = \"Two Sum\"\n[limits]\ntime = \"1s\"\n[[source]]\nplatform = \"codeforces\"\nid = \"1999A\"\n",
    )
    .unwrap();
    fs::write(p.join("statement.md"), "给定 $n$。\n").unwrap();
    fs::write(p.join("statement.en.md"), "Given $n$.\n").unwrap();
    fs::write(p.join("samples/02.in"), "2\n").unwrap();
    fs::write(p.join("samples/02.out"), "4\n").unwrap();
    fs::write(p.join("samples/01.in"), "1\n").unwrap();
    fs::write(p.join("samples/01.ans"), "2\n").unwrap();
    let c = root.join("contests/live");
    fs::create_dir_all(&c).unwrap();
    fs::write(
        c.join("contest.toml"),
        "name = \"Live\"\n[[problem]]\nid = \"two-sum\"\nlabel = \"A\"\ntitle = \"Renamed\"\n",
    )
    .unwrap();
    let ws = Workspace::open(root, false).unwrap();
    (dir, ws)
}

fn freeze(ws: &Workspace) {
    let c = ws.root().join("contests/old");
    fs::create_dir_all(c.join("snapshot/A")).unwrap();
    fs::write(
        c.join("contest.toml"),
        "name = \"Old\"\nfrozen = true\n[[problem]]\nid = \"two-sum\"\nlabel = \"A\"\n",
    )
    .unwrap();
    fs::write(
        c.join("snapshot/A/problem.toml"),
        "title = \"Frozen Sum\"\n",
    )
    .unwrap();
    fs::write(c.join("snapshot/A/statement.md"), "旧题面\n").unwrap();
}

#[test]
fn loads_a_problem() {
    let (_dir, ws) = workspace();
    assert_eq!(ws.list_problems().unwrap(), ["two-sum"]);
    let p = ws.load_problem("two-sum").unwrap();
    assert_eq!(p.meta.sources[0].id, "1999A");
    assert_eq!(p.statements.len(), 2);
    assert_eq!(
        p.statement(Language::English).unwrap().file.text,
        "Given $n$.\n"
    );
    assert_eq!(
        p.statement(Language::Chinese).unwrap().file.path,
        "problems/two-sum/statement.md"
    );
    let names: Vec<_> = p
        .samples
        .iter()
        .map(|s| (s.name.as_str(), s.output.as_str()))
        .collect();
    assert_eq!(names, [("01", "2\n"), ("02", "4\n")]);
    assert!(matches!(ws.load_problem("nope"), Err(Error::NotFound(_))));
    assert!(matches!(ws.load_problem("../x"), Err(Error::InvalidId(_))));
}

#[test]
fn contest_items_are_live_references() {
    let (_dir, ws) = workspace();
    let contest = ws.load_contest("live").unwrap();
    let problems = ws.contest_problems(&contest).unwrap();
    assert_eq!(problems[0].1.dir, "problems/two-sum");
    assert_eq!(problems[0].0.apply(&problems[0].1.meta).title, "Renamed");
}

#[test]
fn frozen_contests_render_from_their_snapshot_and_refuse_writes() {
    let (_dir, ws) = workspace();
    freeze(&ws);
    let contest = ws.load_contest("old").unwrap();
    let problems = ws.contest_problems(&contest).unwrap();
    assert_eq!(problems[0].1.meta.title, "Frozen Sum");
    assert_eq!(problems[0].1.dir, "contests/old/snapshot/A");
    for rel in [
        "contests/old/contest.toml",
        "contests/old/snapshot/A/statement.md",
        "contests/old/new.txt",
    ] {
        let err = ws.write(rel, b"x").unwrap_err();
        assert_eq!(err.code(), "contest_frozen", "{rel}");
    }
    let mut meta = contest.meta.clone();
    meta.frozen = false;
    assert!(matches!(
        ws.save_contest_meta("old", &meta),
        Err(Error::Frozen(_))
    ));
    // The problem library stays writable.
    ws.write("problems/two-sum/statement.md", "新题面\n".as_bytes())
        .unwrap();
    ws.write("contests/live/contest.toml", b"name = \"Live\"\n")
        .unwrap();
}

#[test]
fn paths_stay_inside() {
    let (dir, ws) = workspace();
    for rel in ["../x", "/etc/passwd", "problems/../../x"] {
        assert_eq!(ws.resolve(rel).unwrap_err().code(), "path_escape", "{rel}");
    }
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), dir.path().join("link")).unwrap();
    assert_eq!(
        ws.write("link/x.txt", b"x").unwrap_err().code(),
        "path_escape"
    );
    assert!(!outside.path().join("x.txt").exists());
    assert!(ws.resolve("problems/new/deep/file").is_ok());
}

#[test]
fn writes_are_atomic_and_leave_no_temporary_files() {
    let (_dir, ws) = workspace();
    ws.write("build/out/a.pdf", b"one").unwrap();
    ws.write("build/out/a.pdf", b"two").unwrap();
    let dir = ws.resolve("build/out").unwrap();
    let names: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, ["a.pdf"]);
    assert_eq!(fs::read(dir.join("a.pdf")).unwrap(), b"two");
}

#[test]
fn read_only_refuses_writes() {
    let (dir, _) = workspace();
    let ws = Workspace::open(dir.path(), true).unwrap();
    assert_eq!(ws.write("x.txt", b"x").unwrap_err().code(), "read_only");
    assert!(ws.load_problem("two-sum").is_ok());
}

#[test]
fn discovers_the_root_from_inside() {
    let (dir, _) = workspace();
    let ws = Workspace::discover(dir.path().join("problems/two-sum"), false).unwrap();
    assert_eq!(ws.root(), dir.path().canonicalize().unwrap());
}

#[test]
fn meta_round_trips_through_disk() {
    let (_dir, ws) = workspace();
    let mut p = ws.load_problem("two-sum").unwrap();
    p.meta.tags.push("math".into());
    ws.save_problem_meta("two-sum", &p.meta).unwrap();
    assert_eq!(ws.load_problem("two-sum").unwrap().meta, p.meta);
}

#[test]
fn bad_toml_names_the_file() {
    let (_dir, ws) = workspace();
    ws.write("problems/two-sum/problem.toml", b"title = 1")
        .unwrap();
    let err = ws.load_problem("two-sum").unwrap_err();
    assert_eq!(err.code(), "invalid_file");
    assert!(
        err.to_string().starts_with("problems/two-sum/problem.toml"),
        "{err}"
    );
}
