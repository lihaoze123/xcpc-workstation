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

// ========================================================================================
// Phase 2 tests
// ========================================================================================

use std::collections::BTreeMap;
use std::time::Duration;

use xcpc_domain::{
    ContestMetaPatch, DiffKind, FuzzyMatchKind, LimitsPatch, OverridePatch, Patch,
    ProblemMetaPatch, RatingTier, SearchQuery, Source,
};

#[test]
fn content_hash_and_write_checked_detect_conflicts() {
    let (_dir, ws) = workspace();
    let (text, hash) = ws
        .read_text_with_hash("problems/two-sum/statement.md")
        .unwrap();
    assert_eq!(text, "给定 $n$。\n");
    assert_eq!(hash.len(), 64);
    assert!(
        hash.chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    );
    assert_eq!(hash, content_hash(text.as_bytes()));

    // Matching hash succeeds and returns the new hash.
    let new_hash = ws
        .write_checked(
            "problems/two-sum/statement.md",
            "更新后的题面。\n".as_bytes(),
            Some(&hash),
        )
        .unwrap();
    assert_ne!(new_hash, hash);
    assert_eq!(new_hash, content_hash("更新后的题面。\n".as_bytes()));

    // Stale hash fails with `conflict` and preserves the newer file on disk.
    let err = ws
        .write_checked(
            "problems/two-sum/statement.md",
            b"stale overwrite",
            Some(&hash),
        )
        .unwrap_err();
    assert_eq!(err.code(), "conflict");
    assert!(matches!(
        err,
        Error::Conflict { ref expected, ref actual, .. }
            if expected == &hash && actual == &new_hash
    ));
    assert_eq!(
        ws.read_text("problems/two-sum/statement.md").unwrap(),
        "更新后的题面。\n"
    );

    // Expecting non-existent file ("") works for new files, conflicts if it already exists.
    let created_hash = ws
        .write_checked("problems/two-sum/editorial.md", b"editorial", Some(""))
        .unwrap();
    assert_eq!(created_hash, content_hash(b"editorial"));
    let err = ws
        .write_checked("problems/two-sum/editorial.md", b"again", Some(""))
        .unwrap_err();
    assert_eq!(err.code(), "conflict");
}

#[test]
fn load_samples_skips_orphans_and_inspect_samples_reports_issues() {
    let (_dir, ws) = workspace();
    // Add an unpaired `.in` and unpaired `.ans` / `.out`.
    ws.write("problems/two-sum/samples/03.in", b"3\n").unwrap();
    ws.write("problems/two-sum/samples/04.ans", b"8\n").unwrap();
    ws.write("problems/two-sum/samples/05.out", b"10\n")
        .unwrap();

    // `load_problem` must NOT fail on orphan `.in`; it gracefully skips 03.
    let p = ws.load_problem("two-sum").unwrap();
    let names: Vec<&str> = p.samples.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["01", "02"]);

    let inv = ws.inspect_samples("problems/two-sum/samples").unwrap();
    assert_eq!(inv.pairs, vec!["01", "02"]);
    assert_eq!(inv.orphan_inputs, vec!["03.in"]);
    assert_eq!(inv.orphan_outputs, vec!["04.ans", "05.out"]);
    assert!(!inv.non_contiguous);

    // Create a gap: pair `01` and `03` (missing `02`).
    ws.write("problems/two-sum/samples/03.ans", b"6\n").unwrap();
    fs::remove_file(ws.root().join("problems/two-sum/samples/02.in")).unwrap();
    let inv2 = ws.inspect_samples("problems/two-sum/samples").unwrap();
    assert_eq!(inv2.pairs, vec!["01", "03"]);
    assert!(inv2.non_contiguous);
    assert!(inv2.orphan_outputs.contains(&"02.out".to_owned()));
}

#[test]
fn save_samples_cleans_bom_cr_reorders_safely_and_prunes_surplus() {
    let (_dir, ws) = workspace();
    ws.write("problems/two-sum/samples/03.in", b"old3\n")
        .unwrap();
    ws.write("problems/two-sum/samples/03.ans", b"old3ans\n")
        .unwrap();

    // Swap 01 and 02, include UTF-8 BOM + CRLF, drop 03.
    ws.save_samples(
        "two-sum",
        &[
            ("\u{FEFF}2\r\n".to_owned(), "\u{FEFF}4\r\n".to_owned()),
            ("1\r\n10\r".to_owned(), "2\r\n".to_owned()),
        ],
    )
    .unwrap();

    let p = ws.load_problem("two-sum").unwrap();
    assert_eq!(p.samples.len(), 2);
    assert_eq!(p.samples[0].name, "01");
    assert_eq!(p.samples[0].input, "2\n");
    assert_eq!(p.samples[0].output, "4\n");
    assert_eq!(p.samples[1].name, "02");
    assert_eq!(p.samples[1].input, "1\n10");
    assert_eq!(p.samples[1].output, "2\n");

    // Old 02.out, 03.in, 03.ans must all be removed.
    let inv = ws.inspect_samples("problems/two-sum/samples").unwrap();
    assert_eq!(inv.pairs, vec!["01", "02"]);
    assert!(inv.orphan_inputs.is_empty());
    assert!(inv.orphan_outputs.is_empty());
    assert!(!inv.non_contiguous);
    assert!(!ws.root().join("problems/two-sum/samples/02.out").exists());
    assert!(!ws.root().join("problems/two-sum/samples/03.in").exists());

    // Saving empty samples clears all sample files.
    ws.save_samples("two-sum", &[]).unwrap();
    let inv_empty = ws.inspect_samples("problems/two-sum/samples").unwrap();
    assert!(inv_empty.pairs.is_empty());
    assert!(inv_empty.orphan_inputs.is_empty());
    assert!(inv_empty.orphan_outputs.is_empty());
}

#[test]
fn init_workspace_creates_structure_and_never_overwrites_agents_md() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("fresh-ws");

    let ws = init_workspace(&target).unwrap();
    assert!(ws.root().join("workspace.toml").is_file());
    assert!(ws.root().join("problems").is_dir());
    assert!(ws.root().join("contests").is_dir());
    let gi = fs::read_to_string(ws.root().join(".gitignore")).unwrap();
    assert!(gi.contains("build/"));
    assert!(gi.contains(".xcpc/"));
    let agents = fs::read_to_string(ws.root().join("AGENTS.md")).unwrap();
    assert!(agents.contains("XCPC Workstation"));

    // User customizes AGENTS.md, workspace.toml, and .gitignore.
    fs::write(ws.root().join("AGENTS.md"), "# My Custom Agent Guide\n").unwrap();
    fs::write(ws.root().join("workspace.toml"), "# user config\n").unwrap();
    fs::write(ws.root().join(".gitignore"), "*.log\nbuild/\n").unwrap();

    // Re-running init_workspace must NOT overwrite AGENTS.md or workspace.toml,
    // and appends only the missing `.xcpc/` to `.gitignore`.
    let _ws2 = init_workspace(&target).unwrap();
    assert_eq!(
        fs::read_to_string(target.join("AGENTS.md")).unwrap(),
        "# My Custom Agent Guide\n"
    );
    assert_eq!(
        fs::read_to_string(target.join("workspace.toml")).unwrap(),
        "# user config\n"
    );
    let gi2 = fs::read_to_string(target.join(".gitignore")).unwrap();
    assert!(gi2.contains("*.log"));
    assert!(gi2.contains("build/"));
    assert!(gi2.contains(".xcpc/"));
    assert_eq!(gi2.matches("build/").count(), 1);
}

#[test]
fn create_and_update_problem_meta_preserve_comments_order_and_unknown_fields() {
    let (_dir, ws) = workspace();

    let created = ws.create_problem("new-prob", "线段树").unwrap();
    assert_eq!(created.id, "new-prob");
    assert_eq!(created.meta.title, "线段树");
    assert_eq!(created.meta.limits.time.as_deref(), Some("1s"));
    assert!(ws.root().join("problems/new-prob/samples").is_dir());
    assert_eq!(created.statements.len(), 1);
    assert!(created.statements[0].file.text.contains("## 输入格式"));

    // Creating the same ID again fails with `conflict`.
    assert_eq!(
        ws.create_problem("new-prob", "Dup").unwrap_err().code(),
        "conflict"
    );

    // Hand-craft a problem.toml with comments, key order, and Phase 3 unknown fields.
    let rich = r#"# Problem header comment
title = "Original Title" # inline title comment
rating = 1600
custom_user_key = "keep-me"

[limits]
time = "1s" # inline time comment
memory = "256MB"
extra_limit_key = 99

[[test]]
gen = "random"
args = ["--n", "100000"]

[solutions.std]
main = true
"#;
    ws.write("problems/new-prob/problem.toml", rich.as_bytes())
        .unwrap();

    let updated = ws
        .update_problem_meta(
            "new-prob",
            &ProblemMetaPatch {
                title: Some("Updated Title".into()),
                rating: Patch::Set(1900),
                tags: Some(vec!["dp".into(), "trees".into()]),
                author: Patch::Set("Alice".into()),
                limits: LimitsPatch {
                    time: Patch::Set("2s".into()),
                    ..Default::default()
                },
                sources: None,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(updated.title, "Updated Title");
    assert_eq!(updated.rating, Some(1900));
    assert_eq!(updated.tags, vec!["dp", "trees"]);
    assert_eq!(updated.author.as_deref(), Some("Alice"));
    assert_eq!(updated.limits.time.as_deref(), Some("2s"));
    assert_eq!(updated.limits.memory.as_deref(), Some("256MB"));

    let raw = ws.read_text("problems/new-prob/problem.toml").unwrap();
    assert!(raw.contains("# Problem header comment"), "{raw}");
    assert!(raw.contains("# inline title comment"), "{raw}");
    assert!(raw.contains("# inline time comment"), "{raw}");
    assert!(raw.contains("custom_user_key = \"keep-me\""), "{raw}");
    assert!(raw.contains("extra_limit_key = 99"), "{raw}");
    assert!(raw.contains("[[test]]"), "{raw}");
    assert!(raw.contains("gen = \"random\""), "{raw}");
    assert!(raw.contains("[solutions.std]"), "{raw}");
    // Key order preserved: title before rating before custom_user_key.
    assert!(raw.find("title =").unwrap() < raw.find("rating =").unwrap());
    assert!(raw.find("rating =").unwrap() < raw.find("custom_user_key =").unwrap());
}

#[test]
fn write_problem_files_enforces_whitelist_toml_validation_and_hashes() {
    let (_dir, ws) = workspace();

    // Disallowed paths are rejected before any file is written.
    for bad in [
        "evil.sh",
        "assets/pic.png",
        "../two-sum/statement.md",
        "samples/../statement.md",
        "samples/nested/01.in",
        "samples/.hidden",
        "/etc/passwd",
        "statement.fr.md",
    ] {
        let mut files = BTreeMap::new();
        files.insert("statement.md".into(), "should not write".into());
        files.insert(bad.into(), "bad".into());
        let err = ws.write_problem_files("two-sum", &files, None).unwrap_err();
        assert_eq!(err.code(), "disallowed_path", "bad path: {bad}");
    }
    assert_eq!(
        ws.read_text("problems/two-sum/statement.md").unwrap(),
        "给定 $n$。\n"
    );

    // Invalid problem.toml is rejected before writing sibling files.
    let mut bad_toml = BTreeMap::new();
    bad_toml.insert("problem.toml".into(), "title = [broken".into());
    bad_toml.insert("statement.md".into(), "should not land".into());
    let err = ws
        .write_problem_files("two-sum", &bad_toml, None)
        .unwrap_err();
    assert_eq!(err.code(), "invalid_file");
    assert_eq!(
        ws.read_text("problems/two-sum/statement.md").unwrap(),
        "给定 $n$。\n"
    );

    // Creating a new problem without problem.toml fails.
    let mut no_meta = BTreeMap::new();
    no_meta.insert("statement.md".into(), "hello".into());
    assert_eq!(
        ws.write_problem_files("brand-new", &no_meta, None)
            .unwrap_err()
            .code(),
        "invalid_file"
    );

    // Creating a new problem with whitelisted package files succeeds and returns hashes.
    let mut create = BTreeMap::new();
    create.insert("problem.toml".into(), "title = \"Brand New\"\n".into());
    create.insert("statement.md".into(), "Solve it.\n".into());
    create.insert("editorial.md".into(), "Easy.\n".into());
    create.insert("samples/01.in".into(), "1\n".into());
    create.insert("samples/01.ans".into(), "1\n".into());
    create.insert("solutions/std.cpp".into(), "int main(){}\n".into());
    create.insert("generators/gen.cpp".into(), "int main(){}\n".into());
    create.insert("tests/hand.in".into(), "42\n".into());
    create.insert("checker.cpp".into(), "int main(){}\n".into());
    create.insert("validator.cpp".into(), "int main(){}\n".into());
    create.insert("interactor.cpp".into(), "int main(){}\n".into());
    let hashes = ws.write_problem_files("brand-new", &create, None).unwrap();
    assert_eq!(hashes.len(), create.len());
    assert_eq!(hashes["statement.md"], content_hash(b"Solve it.\n"));

    // Solutions listing and sandboxed read.
    assert_eq!(ws.list_solutions("brand-new").unwrap(), vec!["std.cpp"]);
    assert_eq!(
        ws.read_solution("brand-new", "std.cpp").unwrap(),
        "int main(){}\n"
    );
    assert_eq!(
        ws.read_solution("brand-new", "../problem.toml")
            .unwrap_err()
            .code(),
        "path_escape"
    );

    // Conflict detection via expected_hash.
    let mut update = BTreeMap::new();
    update.insert("statement.md".into(), "Updated.\n".into());
    assert_eq!(
        ws.write_problem_files("brand-new", &update, Some("deadbeef"))
            .unwrap_err()
            .code(),
        "conflict"
    );
    let hashes2 = ws
        .write_problem_files("brand-new", &update, Some(&hashes["statement.md"]))
        .unwrap();
    assert_eq!(hashes2["statement.md"], content_hash(b"Updated.\n"));
}

#[test]
fn compose_reorder_add_remove_and_too_many_problems() {
    let (_dir, ws) = workspace();
    ws.create_problem("p-a", "Alpha").unwrap();
    ws.create_problem("p-b", "Beta").unwrap();
    ws.create_problem("p-c", "Gamma").unwrap();

    // Create a contest via compose_contest with metadata patch.
    let c = ws
        .compose_contest(
            "round-1",
            &["p-a".into(), "p-b".into()],
            Some(&ContestMetaPatch {
                name: Some("Round 1".into()),
                location: Patch::Set("Shanghai".into()),
                ..Default::default()
            }),
        )
        .unwrap();
    assert_eq!(c.meta.name, "Round 1");
    assert_eq!(c.meta.location.as_deref(), Some("Shanghai"));
    assert_eq!(
        c.meta
            .problems
            .iter()
            .map(|p| (p.label.as_str(), p.id.as_str()))
            .collect::<Vec<_>>(),
        vec![("A", "p-a"), ("B", "p-b")]
    );

    // Add comment, unknown keys/tables, and overrides to contest.toml to verify preservation.
    let custom_toml = r#"# Contest header comment
name = "Round 1" # inline name
location = "Shanghai"
custom_contest_key = "keep"

[before_extra]
flag = 1

[[problem]]
# Comment on Alpha
id = "p-a"
label = "A"
title = "Alpha Override"
custom_item_key = "keep-alpha"
[problem.limits]
time = "3s"

[[problem]]
id = "p-b"
label = "B"
show-metadata = false

[after_extra]
flag = 2
"#;
    ws.write("contests/round-1/contest.toml", custom_toml.as_bytes())
        .unwrap();

    // Add p-c -> becomes C.
    let c = ws.add_problem_to_contest("round-1", "p-c").unwrap();
    assert_eq!(c.meta.problems.len(), 3);
    assert_eq!(c.meta.problems[2].label, "C");
    assert_eq!(c.meta.problems[2].id, "p-c");

    // Reorder: [p-c, p-a, p-b] -> labels become A=p-c, B=p-a (with overrides!), C=p-b.
    let c = ws
        .reorder_contest_problems("round-1", &["p-c".into(), "p-a".into(), "p-b".into()])
        .unwrap();
    assert_eq!(
        c.meta
            .problems
            .iter()
            .map(|p| (p.label.as_str(), p.id.as_str()))
            .collect::<Vec<_>>(),
        vec![("A", "p-c"), ("B", "p-a"), ("C", "p-b")]
    );
    assert_eq!(c.meta.problems[1].title.as_deref(), Some("Alpha Override"));
    assert_eq!(
        c.meta.problems[1].limits.as_ref().unwrap().time.as_deref(),
        Some("3s")
    );
    assert_eq!(c.meta.problems[2].show_metadata, Some(false));

    let raw = ws.read_text("contests/round-1/contest.toml").unwrap();
    assert!(raw.contains("# Contest header comment"), "{raw}");
    assert!(raw.contains("# inline name"), "{raw}");
    assert!(raw.contains("custom_contest_key = \"keep\""), "{raw}");
    assert!(raw.contains("custom_item_key = \"keep-alpha\""), "{raw}");
    assert!(raw.contains("[before_extra]"), "{raw}");
    assert!(raw.contains("[after_extra]"), "{raw}");
    assert!(raw.find("[before_extra]").unwrap() < raw.find("[[problem]]").unwrap());
    assert!(raw.rfind("[[problem]]").unwrap() < raw.find("[after_extra]").unwrap());

    // Remove p-b -> drops its override, re-labels A=p-c, B=p-a.
    let c = ws.remove_problem_from_contest("round-1", "p-b").unwrap();
    assert_eq!(
        c.meta
            .problems
            .iter()
            .map(|p| (p.label.as_str(), p.id.as_str()))
            .collect::<Vec<_>>(),
        vec![("A", "p-c"), ("B", "p-a")]
    );
    assert_eq!(c.meta.problems[1].title.as_deref(), Some("Alpha Override"));

    // Duplicate IDs rejected.
    let err = ws
        .compose_contest("round-1", &["p-a".into(), "p-a".into()], None)
        .unwrap_err();
    assert_eq!(err.code(), "invalid_file");

    // Non-existent problem rejected.
    let err = ws
        .compose_contest("round-1", &["ghost".into()], None)
        .unwrap_err();
    assert_eq!(err.code(), "not_found");

    // > 26 problems rejected with `too_many_problems`.
    let mut many = Vec::new();
    for i in 0..27 {
        let pid = format!("prob-{i:02}");
        ws.create_problem(&pid, &format!("P{i}")).unwrap();
        many.push(pid);
    }
    let err = ws.compose_contest("huge", &many, None).unwrap_err();
    assert_eq!(err.code(), "too_many_problems");
    assert!(matches!(err, Error::TooManyProblems(27)));
    // Exactly 26 problems succeeds with labels A..Z.
    let c26 = ws.compose_contest("exact-26", &many[..26], None).unwrap();
    assert_eq!(c26.meta.problems[0].label, "A");
    assert_eq!(c26.meta.problems[25].label, "Z");
}

#[test]
fn set_override_applies_and_auto_cleans_matching_defaults() {
    let (_dir, ws) = workspace();
    ws.compose_contest("c1", &["two-sum".into()], None).unwrap();

    // Set title, limits.time, limits.input, show-metadata overrides.
    let c = ws
        .set_override(
            "c1",
            "A",
            &OverridePatch {
                title: Patch::Set("Two Sum (Hard)".into()),
                limits: LimitsPatch {
                    time: Patch::Set("3s".into()),
                    input: Patch::Set("sum.in".into()),
                    ..Default::default()
                },
                show_metadata: Patch::Set(false),
                ..Default::default()
            },
        )
        .unwrap();
    let item = &c.meta.problems[0];
    assert_eq!(item.title.as_deref(), Some("Two Sum (Hard)"));
    assert_eq!(item.show_metadata, Some(false));
    assert_eq!(item.limits.as_ref().unwrap().time.as_deref(), Some("3s"));
    assert_eq!(
        item.limits.as_ref().unwrap().input.as_deref(),
        Some("sum.in")
    );

    // Reverting to library original values and contest default show-metadata auto-deletes keys
    // and deletes the now-empty `[problem.limits]` table!
    let c = ws
        .set_override(
            "c1",
            "A",
            &OverridePatch {
                title: Patch::Set("Two Sum".into()),
                limits: LimitsPatch {
                    time: Patch::Set("1s".into()),
                    input: Patch::Set(String::new()),
                    ..Default::default()
                },
                show_metadata: Patch::Set(true),
                ..Default::default()
            },
        )
        .unwrap();
    let item = &c.meta.problems[0];
    assert_eq!(item.title, None);
    assert_eq!(item.show_metadata, None);
    assert_eq!(item.limits, None);

    let raw = ws.read_text("contests/c1/contest.toml").unwrap();
    assert!(!raw.contains("title"), "{raw}");
    assert!(!raw.contains("show-metadata"), "{raw}");
    assert!(!raw.contains("limits"), "{raw}");
}

#[test]
fn delete_problem_checks_unfrozen_references_only() {
    let (_dir, ws) = workspace();
    freeze(&ws); // `old` is frozen and references `two-sum`; `live` is unfrozen and references `two-sum`.

    let err = ws.delete_problem("two-sum").unwrap_err();
    assert_eq!(err.code(), "problem_referenced");
    assert!(matches!(
        err,
        Error::Referenced { ref id, ref contests }
            if id == "two-sum" && contests == &vec!["live".to_owned()]
    ));

    // Once removed from `live`, only frozen `old` references it -> deletion allowed!
    ws.remove_problem_from_contest("live", "two-sum").unwrap();
    ws.delete_problem("two-sum").unwrap();
    assert!(ws.list_problems().unwrap().is_empty());

    // Frozen contest still loads from its snapshot cleanly.
    let old = ws.load_contest("old").unwrap();
    let probs = ws.contest_problems(&old).unwrap();
    assert_eq!(probs[0].1.meta.title, "Frozen Sum");
}

#[test]
fn freeze_commit_excludes_package_files_and_unfreeze_diff_is_byte_accurate() {
    let (_dir, ws) = workspace();

    // Populate editorial, binary asset, AND problem-package files that must NOT enter snapshot.
    ws.write("problems/two-sum/editorial.md", b"Sort or hash.\n")
        .unwrap();
    ws.write("problems/two-sum/assets/fig.png", b"\x89PNG\r\n\x1a\nfake")
        .unwrap();
    ws.write("problems/two-sum/solutions/std.cpp", b"int main(){}\n")
        .unwrap();
    ws.write("problems/two-sum/generators/gen.cpp", b"int main(){}\n")
        .unwrap();
    ws.write("problems/two-sum/tests/01.in", b"secret\n")
        .unwrap();
    ws.write("problems/two-sum/checker.cpp", b"int main(){}\n")
        .unwrap();
    ws.write("problems/two-sum/validator.cpp", b"int main(){}\n")
        .unwrap();
    ws.write("problems/two-sum/interactor.cpp", b"int main(){}\n")
        .unwrap();

    // Add comment & override in `live` contest.
    let contest_toml = r#"# Live contest comment
name = "Live"
custom_flag = true

[[problem]]
id = "two-sum"
label = "A"
title = "Renamed"
[problem.limits]
time = "2s"
"#;
    ws.write("contests/live/contest.toml", contest_toml.as_bytes())
        .unwrap();

    // Cannot unfreeze or diff an unfrozen contest.
    assert_eq!(ws.unfreeze_diff("live").unwrap_err().code(), "not_frozen");
    assert_eq!(
        ws.unfreeze_contest("live").unwrap_err().code(),
        "not_frozen"
    );

    // Freeze commit.
    let frozen = ws
        .freeze_commit("live", b"%PDF-statements", Some(b"%PDF-editorial"))
        .unwrap();
    assert!(frozen.meta.frozen);

    // Double freeze fails with `already_frozen`.
    assert_eq!(
        ws.freeze_commit("live", b"%PDF", None).unwrap_err().code(),
        "already_frozen"
    );

    // Snapshot has PDFs, merged problem.toml, statements, editorial, samples, assets...
    let snap = ws.root().join("contests/live/snapshot");
    assert_eq!(fs::read(snap.join("live.pdf")).unwrap(), b"%PDF-statements");
    assert_eq!(
        fs::read(snap.join("live-editorial.pdf")).unwrap(),
        b"%PDF-editorial"
    );
    let snap_a = snap.join("A");
    assert!(snap_a.join("problem.toml").is_file());
    assert!(snap_a.join("statement.md").is_file());
    assert!(snap_a.join("statement.en.md").is_file());
    assert!(snap_a.join("editorial.md").is_file());
    assert!(snap_a.join("samples/01.in").is_file());
    assert!(snap_a.join("assets/fig.png").is_file());

    // ...and strictly EXCLUDES problem-package code and tests!
    for forbidden in [
        "solutions",
        "generators",
        "tests",
        "checker.cpp",
        "validator.cpp",
        "interactor.cpp",
    ] {
        assert!(
            !snap_a.join(forbidden).exists(),
            "must not copy {forbidden}"
        );
    }

    // Snapshot problem.toml has merged overrides (`Renamed`, `2s`).
    let probs = ws.contest_problems(&frozen).unwrap();
    assert_eq!(probs[0].1.meta.title, "Renamed");
    assert_eq!(probs[0].1.meta.limits.time.as_deref(), Some("2s"));

    // Frozen contest refuses all normal mutations.
    assert_eq!(
        ws.update_contest_meta(
            "live",
            &ContestMetaPatch {
                name: Some("Hacked".into()),
                ..Default::default()
            }
        )
        .unwrap_err()
        .code(),
        "contest_frozen"
    );
    assert_eq!(
        ws.set_override("live", "A", &OverridePatch::default())
            .unwrap_err()
            .code(),
        "contest_frozen"
    );

    // Immediately after freeze (even with overrides!), unfreeze_diff reports NO differences.
    assert!(ws.unfreeze_diff("live").unwrap().is_empty());

    // Modifying package files (`solutions/`) must NOT trigger false-positive diff.
    ws.write("problems/two-sum/solutions/brute.cpp", b"int main(){}\n")
        .unwrap();
    assert!(ws.unfreeze_diff("live").unwrap().is_empty());

    // Modify statement, sample, editorial, binary asset, and non-overridden meta field.
    ws.write("problems/two-sum/statement.md", "改动题面\n".as_bytes())
        .unwrap();
    ws.write("problems/two-sum/samples/01.ans", b"999\n")
        .unwrap();
    ws.write("problems/two-sum/editorial.md", b"New editorial.\n")
        .unwrap();
    ws.write("problems/two-sum/assets/fig.png", b"\x89PNG-changed")
        .unwrap();
    ws.update_problem_meta(
        "two-sum",
        &ProblemMetaPatch {
            rating: Patch::Set(1500),
            ..Default::default()
        },
    )
    .unwrap();

    let diffs = ws.unfreeze_diff("live").unwrap();
    assert_eq!(diffs.len(), 1);
    assert_eq!(diffs[0].label, "A");
    assert_eq!(diffs[0].problem_id, "two-sum");
    assert_eq!(
        diffs[0].kinds,
        vec![
            DiffKind::StatementChanged,
            DiffKind::SamplesChanged,
            DiffKind::EditorialChanged,
            DiffKind::AssetsChanged,
            DiffKind::MetaChanged,
        ]
    );
    assert!(diffs[0].changed_files.contains(&"statement.md".to_owned()));
    assert!(
        diffs[0]
            .changed_files
            .contains(&"samples/01.ans".to_owned())
    );
    assert!(diffs[0].changed_files.contains(&"editorial.md".to_owned()));
    assert!(
        diffs[0]
            .changed_files
            .contains(&"assets/fig.png".to_owned())
    );
    assert!(diffs[0].changed_files.contains(&"problem.toml".to_owned()));

    // Deleting the library problem (allowed because only frozen contest references it) -> ProblemDeleted.
    ws.delete_problem("two-sum").unwrap();
    let del_diffs = ws.unfreeze_diff("live").unwrap();
    assert_eq!(del_diffs.len(), 1);
    assert_eq!(del_diffs[0].kinds, vec![DiffKind::ProblemDeleted]);

    // Read-only workspace refuses unfreeze.
    let ro = Workspace::open(ws.root(), true).unwrap();
    assert_eq!(ro.unfreeze_contest("live").unwrap_err().code(), "read_only");

    // Unfreezing sets frozen = false (preserving comments/unknown keys) and deletes snapshot/.
    let unfrozen = ws.unfreeze_contest("live").unwrap();
    assert!(!unfrozen.meta.frozen);
    assert!(!snap.exists());
    let raw = ws.read_text("contests/live/contest.toml").unwrap();
    assert!(raw.contains("# Live contest comment"), "{raw}");
    assert!(raw.contains("custom_flag = true"), "{raw}");
}

#[test]
fn workspace_index_search_stats_refresh_and_fuzzy_frecency() {
    let (_dir, ws) = workspace();
    ws.update_problem_meta(
        "two-sum",
        &ProblemMetaPatch {
            rating: Patch::Set(800),
            tags: Some(vec!["greedy".into(), "math".into()]),
            ..Default::default()
        },
    )
    .unwrap();
    ws.write("problems/two-sum/editorial.md", b"Use a hash map.\n")
        .unwrap();
    ws.write("problems/two-sum/solutions/std.cpp", b"int main(){}\n")
        .unwrap();

    ws.create_problem("seg-tree", "线段树合并").unwrap();
    ws.update_problem_meta(
        "seg-tree",
        &ProblemMetaPatch {
            rating: Patch::Set(2200),
            tags: Some(vec!["data structures".into(), "trees".into()]),
            sources: Some(vec![Source {
                platform: "atcoder".into(),
                id: "ABC300G".into(),
                url: None,
            }]),
            ..Default::default()
        },
    )
    .unwrap();
    ws.write(
        "problems/seg-tree/statement.md",
        "维护一棵带权有根树。\n\n## 输入格式\n\n## 输出格式\n".as_bytes(),
    )
    .unwrap();

    ws.create_problem("unrated-dp", "Mystery DP").unwrap();
    ws.update_problem_meta(
        "unrated-dp",
        &ProblemMetaPatch {
            tags: Some(vec!["dp".into(), "math".into()]),
            ..Default::default()
        },
    )
    .unwrap();

    ws.compose_contest(
        "icpc-sh",
        &["two-sum".into(), "seg-tree".into(), "unrated-dp".into()],
        Some(&ContestMetaPatch {
            name: Some("ICPC Shanghai".into()),
            ..Default::default()
        }),
    )
    .unwrap();

    let mut idx = WorkspaceIndex::scan(&ws).unwrap();
    assert_eq!(idx.problems().len(), 3);
    assert!(idx.problem("two-sum").unwrap().has_editorial);
    assert!(idx.problem("two-sum").unwrap().has_solutions);
    assert!(!idx.problem("seg-tree").unwrap().has_editorial);
    assert!(!idx.problem("seg-tree").unwrap().has_solutions);

    // Search by normalized text (matches statement body & editorial).
    let hits = idx.search_problems(&SearchQuery {
        text: Some("hash   MAP".into()),
        ..Default::default()
    });
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, "two-sum");

    let zh_hits = idx.search_problems(&SearchQuery {
        text: Some("有根树".into()),
        ..Default::default()
    });
    assert_eq!(zh_hits.len(), 1);
    assert_eq!(zh_hits[0].id, "seg-tree");

    // Structured filters: rating range excludes unrated.
    let rated = idx.search_problems(&SearchQuery {
        rating_min: Some(1000),
        rating_max: Some(2300),
        ..Default::default()
    });
    assert_eq!(rated.len(), 1);
    assert_eq!(rated[0].id, "seg-tree");

    // Platform + tag filters (case-insensitive).
    let plat = idx.search_problems(&SearchQuery {
        platform: Some("Codeforces".into()),
        tags: vec!["MATH".into(), "greedy".into()],
        ..Default::default()
    });
    assert_eq!(plat.len(), 1);
    assert_eq!(plat[0].id, "two-sum");

    // Contest stats: rating tiers + unrated + sorted tag counts.
    let stats = idx.contest_stats_by_id("icpc-sh").unwrap();
    assert_eq!(stats.problem_count, 3);
    assert_eq!(stats.rating_distribution[&RatingTier::Newbie], 1);
    assert_eq!(stats.rating_distribution[&RatingTier::Master], 1);
    assert_eq!(stats.rating_distribution[&RatingTier::Unrated], 1);
    assert_eq!(stats.rating_distribution[&RatingTier::Expert], 0);
    assert_eq!(stats.tag_counts[0], ("math".to_owned(), 2));

    // Incremental refresh on edit and deletion.
    ws.create_problem("fresh-one", "Fresh Graph").unwrap();
    idx.refresh_path(&ws, "problems/fresh-one/problem.toml")
        .unwrap();
    assert!(idx.problem("fresh-one").is_some());
    ws.delete_problem("fresh-one").unwrap();
    idx.refresh_path(&ws, "problems/fresh-one/problem.toml")
        .unwrap();
    assert!(idx.problem("fresh-one").is_none());

    // Fuzzy search + Frecency boost.
    ws.create_problem("two-sat", "Two Sat").unwrap();
    idx.refresh_problem(&ws, "two-sat").unwrap();
    let t0 = 1_700_000_000;
    let before = idx.fuzzy_search_at("two", 5, t0);
    assert!(before.iter().any(|h| h.id == "two-sat"));
    assert!(before.iter().any(|h| h.id == "two-sum"));
    // Visit `two-sat` multiple times -> ranks first for `"two"` and for empty Cmd+K query.
    idx.record_visit_at(FuzzyMatchKind::Problem, "two-sat", t0);
    idx.record_visit_at(FuzzyMatchKind::Problem, "two-sat", t0 + 10);
    let after = idx.fuzzy_search_at("two", 5, t0 + 20);
    assert_eq!(after[0].id, "two-sat");
    assert_eq!(after[0].kind, FuzzyMatchKind::Problem);

    let empty_q = idx.fuzzy_search_at("", 2, t0 + 20);
    assert_eq!(empty_q[0].id, "two-sat");

    // Contest fuzzy matching works too.
    let contest_hits = idx.fuzzy_search_at("shanghai", 5, t0 + 20);
    assert_eq!(contest_hits[0].kind, FuzzyMatchKind::Contest);
    assert_eq!(contest_hits[0].id, "icpc-sh");
}

#[test]
fn watcher_ignores_internal_paths_and_filters_self_writes() {
    use watcher::{SelfWriteTracker, WorkspaceWatcher, should_ignore};

    // Unit-level ignore checks.
    for ignored in [
        "",
        "build/out.pdf",
        ".xcpc/oj_sync.db",
        ".git/HEAD",
        ".jj/working_copy/checkout",
        "problems/two-sum/.statement.md.tmp-123-0",
        "problems/two-sum/.tmp-foo",
        "contests/c/.snapshot.tmp-1-0/A/problem.toml",
    ] {
        assert!(should_ignore(ignored), "should ignore {ignored:?}");
    }
    for kept in [
        "workspace.toml",
        "problems/two-sum/statement.md",
        "contests/live/contest.toml",
    ] {
        assert!(!should_ignore(kept), "should not ignore {kept:?}");
    }

    // Unit-level SelfWriteTracker checks.
    let tracker = SelfWriteTracker::new();
    let h1 = content_hash(b"v1");
    let h2 = content_hash(b"v2");
    tracker.record("problems/a/statement.md", &h1);
    assert!(tracker.is_self_write("problems/a/statement.md", &h1));
    // A different hash means external write and evicts the entry.
    assert!(!tracker.is_self_write("problems/a/statement.md", &h2));
    assert!(!tracker.is_self_write("problems/a/statement.md", &h1));

    // End-to-end notify watcher test with short debounce.
    let (_dir, ws) = workspace();
    let live_tracker = SelfWriteTracker::new();
    let (_watcher, rx) =
        WorkspaceWatcher::start_with(ws.root(), Duration::from_millis(30), live_tracker.clone())
            .unwrap();
    let tracked_ws = ws.with_write_tracker(live_tracker);

    // 1. Self-write via tracked Workspace must NOT emit any WatchEvent.
    tracked_ws
        .write("problems/two-sum/statement.md", b"self save\n")
        .unwrap();
    // 2. Ignored paths written directly must NOT emit any WatchEvent.
    fs::create_dir_all(ws.root().join("build")).unwrap();
    fs::write(ws.root().join("build/out.pdf"), b"%PDF").unwrap();
    fs::create_dir_all(ws.root().join(".xcpc")).unwrap();
    fs::write(ws.root().join(".xcpc/cache.db"), b"x").unwrap();
    fs::write(
        ws.root().join("problems/two-sum/.statement.md.tmp-99-0"),
        b"tmp",
    )
    .unwrap();

    assert!(
        rx.recv_timeout(Duration::from_millis(200)).is_err(),
        "self-writes and ignored paths must not trigger external watch events"
    );

    // 3. External edit bypassing Workspace MUST emit WatchEvent::Changed.
    fs::write(
        ws.root().join("problems/two-sum/statement.md"),
        b"external agent edit\n",
    )
    .unwrap();
    let ev = rx
        .recv_timeout(Duration::from_secs(2))
        .expect("expected WatchEvent for external edit");
    match ev {
        WatchEvent::Changed(paths) => {
            assert!(
                paths.contains(&"problems/two-sum/statement.md".to_owned()),
                "unexpected paths: {paths:?}"
            );
        }
        WatchEvent::Rescan => panic!("unexpected Rescan"),
    }

    // 4. External deletion MUST also emit WatchEvent::Changed.
    fs::remove_file(ws.root().join("problems/two-sum/statement.en.md")).unwrap();
    let ev_del = rx
        .recv_timeout(Duration::from_secs(2))
        .expect("expected WatchEvent for external delete");
    match ev_del {
        WatchEvent::Changed(paths) => {
            assert!(
                paths.contains(&"problems/two-sum/statement.en.md".to_owned()),
                "unexpected paths: {paths:?}"
            );
        }
        WatchEvent::Rescan => panic!("unexpected Rescan"),
    }
}

#[test]
fn error_human_messages_are_actionable_chinese_while_codes_stay_stable() {
    let cases = [
        (Error::NotFound("problems/x".into()), "not_found"),
        (Error::PathEscape("../etc/passwd".into()), "path_escape"),
        (Error::Frozen("spring".into()), "contest_frozen"),
        (Error::ReadOnly, "read_only"),
        (
            Error::Invalid {
                path: "problem.toml".into(),
                message: "bad".into(),
            },
            "invalid_file",
        ),
        (
            Error::Conflict {
                path: "statement.md".into(),
                expected: "a".into(),
                actual: "b".into(),
            },
            "conflict",
        ),
        (Error::TooManyProblems(27), "too_many_problems"),
        (
            Error::Referenced {
                id: "p1".into(),
                contests: vec!["c1".into()],
            },
            "problem_referenced",
        ),
        (Error::AlreadyFrozen("c1".into()), "already_frozen"),
        (Error::NotFrozen("c1".into()), "not_frozen"),
        (Error::DisallowedPath("foo".into()), "disallowed_path"),
    ];
    for (err, expected_code) in cases {
        assert_eq!(err.code(), expected_code);
        let msg = err.human_message();
        assert!(msg.contains("请"), "missing next-step guidance in: {msg}");
    }
}

#[test]
fn init_example_workspace_seeds_files_and_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let ws = init_example_workspace(dir.path()).unwrap();
    assert_eq!(
        ws.list_problems().unwrap(),
        vec![
            "bipartite-coloring",
            "bracket-balance",
            "crc-check",
            "flow-network-cut",
            "hidden-peak",
            "interval-sum",
            "secret-relay",
            "sliding-window-median",
            "suffix-automaton-lcp",
            "tree-beacon",
            "typst-showcase",
            "weighted-walk",
        ]
    );
    assert_eq!(
        ws.workspace_groups(),
        vec!["出版精选", "本地出题包", "省赛候选", "待整理"]
    );
    assert_eq!(
        ws.list_contests().unwrap(),
        vec!["oi-mock", "welcome-round"]
    );

    // User edits must not be overwritten on a second init_example_workspace call.
    let custom = "自定义题面\n";
    ws.write("problems/crc-check/statement.md", custom.as_bytes())
        .unwrap();
    let ws2 = init_example_workspace(dir.path()).unwrap();
    assert_eq!(
        ws2.read_text("problems/crc-check/statement.md").unwrap(),
        custom
    );
}

#[test]
fn asset_name_validation_rejects_windows_illegal_and_reserved_names() {
    assert!(is_safe_asset_name("fig-1_ok.png"));
    assert!(is_safe_asset_name("中文图.svg"));
    assert!(!is_safe_asset_name(""));
    assert!(!is_safe_asset_name(".hidden.png"));
    assert!(!is_safe_asset_name("trailing."));
    assert!(!is_safe_asset_name("trailing "));
    assert!(!is_safe_asset_name("a/b.png"));
    assert!(!is_safe_asset_name("a\\b.png"));
    assert!(!is_safe_asset_name("a:b.png"));
    assert!(!is_safe_asset_name("a*b.png"));
    assert!(!is_safe_asset_name("a?b.png"));
    assert!(!is_safe_asset_name("a\"b.png"));
    assert!(!is_safe_asset_name("a<b.png"));
    assert!(!is_safe_asset_name("a>b.png"));
    assert!(!is_safe_asset_name("a|b.png"));
    assert!(!is_safe_asset_name("CON"));
    assert!(!is_safe_asset_name("con.png"));
    assert!(!is_safe_asset_name("NUL.svg"));
    assert!(!is_safe_asset_name("com1.jpg"));
    assert!(!is_safe_asset_name("LPT9.webp"));
    assert!(!is_safe_asset_name(&"a".repeat(201)));
}

#[test]
fn read_image_dimensions_parses_png_svg_and_webp() {
    let dir = tempfile::tempdir().unwrap();

    // Minimal valid PNG header with 320x240 IHDR.
    let mut png = Vec::new();
    png.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    png.extend_from_slice(&13u32.to_be_bytes());
    png.extend_from_slice(b"IHDR");
    png.extend_from_slice(&320u32.to_be_bytes());
    png.extend_from_slice(&240u32.to_be_bytes());
    let png_path = dir.path().join("a.png");
    std::fs::write(&png_path, &png).unwrap();
    assert_eq!(read_image_dimensions(&png_path), Some((320, 240)));

    // SVG with viewBox.
    let svg_path = dir.path().join("b.svg");
    std::fs::write(
        &svg_path,
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 480"></svg>"#,
    )
    .unwrap();
    assert_eq!(read_image_dimensions(&svg_path), Some((640, 480)));

    // Minimal VP8X WebP header with 800x600.
    let mut webp = vec![0u8; 30];
    webp[0..4].copy_from_slice(b"RIFF");
    webp[8..12].copy_from_slice(b"WEBP");
    webp[12..16].copy_from_slice(b"VP8X");
    let w_minus_1 = 799u32.to_le_bytes();
    let h_minus_1 = 599u32.to_le_bytes();
    webp[24..27].copy_from_slice(&w_minus_1[0..3]);
    webp[27..30].copy_from_slice(&h_minus_1[0..3]);
    let webp_path = dir.path().join("c.webp");
    std::fs::write(&webp_path, &webp).unwrap();
    assert_eq!(read_image_dimensions(&webp_path), Some((800, 600)));
}

#[test]
fn list_assets_generates_cached_thumbnails_for_png_and_svg_and_falls_back_safely() {
    let dir = tempfile::tempdir().unwrap();
    let ws = init_workspace(dir.path()).unwrap();
    ws.write("problems/p1/problem.toml", b"title = \"Thumbs\"\n")
        .unwrap();
    let assets_dir = dir.path().join("problems/p1/assets");
    std::fs::create_dir_all(&assets_dir).unwrap();

    // Create a valid 600x400 PNG.
    let mut png_bytes = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut png_bytes, 600, 400);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().unwrap();
        w.write_image_data(&vec![128u8; 600 * 400 * 4]).unwrap();
    }
    std::fs::write(assets_dir.join("big.png"), &png_bytes).unwrap();

    // Create a valid SVG.
    std::fs::write(
        assets_dir.join("vec.svg"),
        br#"<svg xmlns="http://www.w3.org/2000/svg" width="800" height="400"><rect width="800" height="400" fill="red"/></svg>"#,
    )
    .unwrap();

    // Create a corrupt PNG and an unsupported PDF.
    std::fs::write(assets_dir.join("broken.png"), b"not a png").unwrap();
    std::fs::write(assets_dir.join("doc.pdf"), b"%PDF-1.4").unwrap();

    let entries = ws.list_assets_with("problems/p1", |_| Vec::new()).unwrap();
    let by_name = |n: &str| entries.iter().find(|e| e.name == n).unwrap();

    let big = by_name("big.png");
    assert_eq!((big.width, big.height), (Some(600), Some(400)));
    let big_thumb = big.thumb_path.as_ref().expect("expected PNG thumbnail");
    assert!(big_thumb.is_file());
    assert!(big_thumb.starts_with(dir.path().join(".xcpc/thumbs")));
    let (tw, th) = read_image_dimensions(big_thumb).unwrap();
    assert!(tw <= 336 && th <= 336, "thumb too big: {tw}x{th}");

    let svg = by_name("vec.svg");
    let svg_thumb = svg.thumb_path.as_ref().expect("expected SVG thumbnail");
    assert!(svg_thumb.is_file());
    let (stw, sth) = read_image_dimensions(svg_thumb).unwrap();
    assert_eq!((stw, sth), (336, 168));

    assert!(by_name("broken.png").thumb_path.is_none());
    assert!(by_name("doc.pdf").thumb_path.is_none());

    // Read-only workspace reuses existing cached thumbs and never writes new ones.
    std::fs::write(assets_dir.join("fresh.png"), &png_bytes).unwrap();
    let ro = Workspace::open(dir.path(), true).unwrap();
    let ro_entries = ro.list_assets_with("problems/p1", |_| Vec::new()).unwrap();
    assert!(
        ro_entries
            .iter()
            .find(|e| e.name == "big.png")
            .unwrap()
            .thumb_path
            .is_some()
    );
    assert!(
        ro_entries
            .iter()
            .find(|e| e.name == "fresh.png")
            .unwrap()
            .thumb_path
            .is_none()
    );
}

#[test]
fn human_messages_use_reedit_terminology_and_typed_not_found() {
    assert!(
        Error::NotFound("problem \"nope\"".into())
            .human_message()
            .contains("找不到题目「nope」")
    );
    assert!(
        Error::NotFound("contest \"spring\"".into())
            .human_message()
            .contains("找不到比赛「spring」")
    );
    assert!(
        Error::NotFound("label \"A\" in contest \"spring\"".into())
            .human_message()
            .contains("比赛「spring」中找不到题号「A」")
    );
    for err in [
        Error::Frozen("c1".into()),
        Error::AlreadyFrozen("c1".into()),
        Error::NotFrozen("c1".into()),
    ] {
        let msg = err.human_message();
        assert!(!msg.contains("解稿"), "unexpected legacy term in: {msg}");
        assert!(msg.contains("重新编辑"), "expected 重新编辑 in: {msg}");
    }
    let id_msg = Error::InvalidId(xcpc_domain::InvalidId("Bad".into())).human_message();
    assert!(id_msg.contains("64"));
}

#[test]
fn template_seeds_disk_only_listing_detach_remove_and_image_import() {
    let dir = tempfile::tempdir().unwrap();
    let ws = init_workspace_with_template(dir.path(), Some("cnoi-classic")).unwrap();

    // Only the chosen preset is materialized into templates/ (no implicit built-in list).
    assert_eq!(ws.list_templates(), vec!["cnoi-classic".to_owned()]);
    assert!(dir.path().join("templates/cnoi-classic.typ").is_file());
    assert!(!dir.path().join("templates/olymp-classic.typ").exists());

    // install_preset_template installs to default or custom name and refuses overwrite.
    let installed = ws
        .install_preset_template("minimal", Some("my-min"))
        .unwrap();
    assert_eq!(installed, "templates/my-min.typ");
    assert_eq!(
        ws.list_templates(),
        vec!["cnoi-classic".to_owned(), "my-min".to_owned()]
    );
    let conflict = ws
        .install_preset_template("minimal", Some("my-min"))
        .unwrap_err();
    assert_eq!(conflict.code(), "conflict");
    let unknown = ws
        .install_preset_template("no-such-preset", None)
        .unwrap_err();
    assert_eq!(unknown.code(), "not_found");

    // Deleting files in templates/ updates list immediately (no built-in fallback).
    fs::remove_file(dir.path().join("templates/my-min.typ")).unwrap();
    assert_eq!(ws.list_templates(), vec!["cnoi-classic".to_owned()]);

    // Contest-local template detach & remove.
    ws.create_problem("p1", "P1").unwrap();
    ws.compose_contest("r1", &["p1".into()], None).unwrap();
    assert!(!ws.has_contest_template("r1"));
    let detached = ws.detach_contest_template("r1").unwrap();
    assert_eq!(detached, "contests/r1/template.typ");
    assert!(ws.has_contest_template("r1"));
    assert!(ws.contest_template_path("r1").unwrap().is_some());
    assert_eq!(
        ws.detach_contest_template("r1").unwrap_err().code(),
        "conflict"
    );
    ws.remove_contest_template("r1").unwrap();
    assert!(!ws.has_contest_template("r1"));
    assert_eq!(
        ws.remove_contest_template("r1").unwrap_err().code(),
        "not_found"
    );

    // Template image import with collision avoidance.
    let src_dir = tempfile::tempdir().unwrap();
    let logo_src = src_dir.path().join("school logo.png");
    fs::write(&logo_src, b"fake-png-1").unwrap();
    let rel1 = ws.import_template_image(Some("r1"), &logo_src).unwrap();
    assert_eq!(rel1, "contests/r1/assets/school-logo.png");
    let rel2 = ws.import_template_image(Some("r1"), &logo_src).unwrap();
    assert_eq!(rel2, "contests/r1/assets/school-logo-2.png");
    let ws_rel = ws.import_template_image(None, &logo_src).unwrap();
    assert_eq!(ws_rel, "assets/school-logo.png");

    // Read-only workspace blocks all template mutations.
    let ro = Workspace::open(dir.path(), true).unwrap();
    assert_eq!(
        ro.install_preset_template("olymp-classic", None)
            .unwrap_err()
            .code(),
        "read_only"
    );
    assert_eq!(
        ro.detach_contest_template("r1").unwrap_err().code(),
        "read_only"
    );
    assert_eq!(
        ro.import_template_image(None, &logo_src)
            .unwrap_err()
            .code(),
        "read_only"
    );
}

#[test]
fn duplicate_problem_copies_tree_preserves_toml_and_enforces_guards() {
    let dir = tempfile::tempdir().unwrap();
    let ws = init_workspace(dir.path()).unwrap();

    // Create a rich source problem with comments, unknown fields, [[source]], [template-options],
    // samples/, assets/, solutions/.
    let src_toml = "\
# leading comment
title = \"原题标题\"
rating = 1800
tags = [\"dp\", \"math\"]
custom_extra = \"keep-me\"

[limits]
time = \"2s\"
memory = \"512MB\"

[[source]]
platform = \"codeforces\"
id = \"1999A\"

[template-options.olymp-classic]
sample-layout = \"wide\"
";
    ws.write("problems/src-prob/problem.toml", src_toml.as_bytes())
        .unwrap();
    ws.write("problems/src-prob/statement.md", b"Original statement.\n")
        .unwrap();
    ws.write("problems/src-prob/editorial.md", b"Original editorial.\n")
        .unwrap();
    ws.write("problems/src-prob/samples/01.in", b"1 2\n")
        .unwrap();
    ws.write("problems/src-prob/samples/01.ans", b"3\n")
        .unwrap();
    ws.write("problems/src-prob/assets/fig.svg", b"<svg/>")
        .unwrap();
    ws.write("problems/src-prob/solutions/ac.cpp", b"int main(){}\n")
        .unwrap();

    // 1. Full copy with new_title preserves comments, unknown fields, sources, options, nested files.
    let dup = ws
        .duplicate_problem("src-prob", "src-prob-copy", Some("新题标题"))
        .unwrap();
    assert_eq!(dup.id, "src-prob-copy");
    assert_eq!(dup.meta.title, "新题标题");
    assert_eq!(dup.meta.rating, Some(1800));
    assert_eq!(dup.meta.sources.len(), 1);
    assert_eq!(dup.samples.len(), 1);
    assert!(dup.editorial.is_some());
    assert_eq!(
        ws.read_text("problems/src-prob-copy/assets/fig.svg")
            .unwrap(),
        "<svg/>"
    );
    assert_eq!(
        ws.read_text("problems/src-prob-copy/solutions/ac.cpp")
            .unwrap(),
        "int main(){}\n"
    );
    let copied_toml = ws.read_text("problems/src-prob-copy/problem.toml").unwrap();
    assert!(copied_toml.contains("# leading comment"));
    assert!(copied_toml.contains("custom_extra = \"keep-me\""));
    assert!(copied_toml.contains("sample-layout = \"wide\""));

    // 2. Omitting or blank new_title keeps source title.
    let dup2 = ws
        .duplicate_problem("src-prob", "src-prob-copy-2", Some("   "))
        .unwrap();
    assert_eq!(dup2.meta.title, "原题标题");

    // 3. Conflict, invalid ID, not found, read-only guards.
    assert_eq!(
        ws.duplicate_problem("src-prob", "src-prob-copy", None)
            .unwrap_err()
            .code(),
        "conflict"
    );
    assert_eq!(
        ws.duplicate_problem("src-prob", "Bad ID", None)
            .unwrap_err()
            .code(),
        "invalid_id"
    );
    assert_eq!(
        ws.duplicate_problem("missing-src", "ok-id", None)
            .unwrap_err()
            .code(),
        "not_found"
    );
    let ro = Workspace::open(dir.path(), true).unwrap();
    assert_eq!(
        ro.duplicate_problem("src-prob", "ro-copy", None)
            .unwrap_err()
            .code(),
        "read_only"
    );

    // 4. Symlink inside source problem is rejected with PathEscape and leaves no temp/target dir.
    #[cfg(unix)]
    {
        let outside = tempfile::tempdir().unwrap();
        let secret = outside.path().join("secret.txt");
        fs::write(&secret, b"top-secret").unwrap();
        std::os::unix::fs::symlink(
            &secret,
            dir.path().join("problems/src-prob/assets/leak.txt"),
        )
        .unwrap();
        let err = ws
            .duplicate_problem("src-prob", "symlink-copy", None)
            .unwrap_err();
        assert_eq!(err.code(), "path_escape");
        assert!(!dir.path().join("problems/symlink-copy").exists());
        let leftover_tmp: Vec<_> = fs::read_dir(dir.path().join("problems"))
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with(".dup.tmp-"))
            .collect();
        assert!(
            leftover_tmp.is_empty(),
            "leftover temp dir: {leftover_tmp:?}"
        );
    }
}

#[test]
fn set_contest_archived_is_idempotent_orthogonal_to_frozen_and_blocks_writes() {
    let dir = tempfile::tempdir().unwrap();
    let ws = init_workspace(dir.path()).unwrap();
    ws.create_problem("p1", "P1").unwrap();
    ws.compose_contest("c1", &["p1".into()], None).unwrap();

    // Add comment & unknown key to contest.toml to verify toml_edit preserves them.
    let raw = ws.read_text("contests/c1/contest.toml").unwrap();
    ws.write(
        "contests/c1/contest.toml",
        format!("# contest comment\ncustom_flag = 42\n{raw}").as_bytes(),
    )
    .unwrap();

    // 1. Archive & idempotent re-archive.
    let meta = ws.set_contest_archived("c1", true).unwrap();
    assert!(meta.archived);
    assert!(!meta.frozen);
    let meta_again = ws.set_contest_archived("c1", true).unwrap();
    assert!(meta_again.archived);
    let archived_msg = Error::Archived("c1".into()).human_message();
    assert_eq!(
        archived_msg,
        "比赛「c1」已归档，当前只读。如需修改，请先「取消归档」。"
    );

    // 2. Archived contest blocks all write operations with `contest_archived`.
    assert_eq!(
        ws.compose_contest("c1", &["p1".into()], None)
            .unwrap_err()
            .code(),
        "contest_archived"
    );
    assert_eq!(
        ws.update_contest_meta(
            "c1",
            &ContestMetaPatch {
                name: Some("Hack".into()),
                ..Default::default()
            }
        )
        .unwrap_err()
        .code(),
        "contest_archived"
    );
    assert_eq!(
        ws.set_override(
            "c1",
            "A",
            &OverridePatch {
                title: Patch::Set("Hack".into()),
                ..Default::default()
            }
        )
        .unwrap_err()
        .code(),
        "contest_archived"
    );
    assert_eq!(
        ws.freeze_commit("c1", b"%PDF", None).unwrap_err().code(),
        "contest_archived"
    );
    assert_eq!(
        ws.detach_contest_template("c1").unwrap_err().code(),
        "contest_archived"
    );
    assert_eq!(
        ws.write("contests/c1/extra.txt", b"x").unwrap_err().code(),
        "contest_archived"
    );

    // 3. Unfrozen archived contest still blocks delete_problem.
    assert_eq!(
        ws.delete_problem("p1").unwrap_err().code(),
        "problem_referenced"
    );

    // 4. Unarchive removes `archived` key cleanly, preserves comments & unknown keys, idempotent.
    let unarchived = ws.set_contest_archived("c1", false).unwrap();
    assert!(!unarchived.archived);
    let text = ws.read_text("contests/c1/contest.toml").unwrap();
    assert!(!text.contains("archived"));
    assert!(text.contains("# contest comment"));
    assert!(text.contains("custom_flag = 42"));
    assert!(!ws.set_contest_archived("c1", false).unwrap().archived);

    // 5. Orthogonal to `frozen`: freeze -> archive -> unfreeze blocked -> unarchive -> still frozen.
    ws.freeze_commit("c1", b"%PDF", None).unwrap();
    let frozen_archived = ws.set_contest_archived("c1", true).unwrap();
    assert!(frozen_archived.frozen && frozen_archived.archived);
    assert_eq!(
        ws.unfreeze_contest("c1").unwrap_err().code(),
        "contest_archived"
    );
    let restored = ws.set_contest_archived("c1", false).unwrap();
    assert!(restored.frozen && !restored.archived);

    // 6. Read-only & not_found guards.
    let ro = Workspace::open(dir.path(), true).unwrap();
    assert_eq!(
        ro.set_contest_archived("c1", true).unwrap_err().code(),
        "read_only"
    );
    assert_eq!(
        ws.set_contest_archived("no-such", true).unwrap_err().code(),
        "not_found"
    );
}

#[test]
fn detach_contest_template_copies_options_to_local_and_not_found_messages_guide_presets() {
    let dir = tempfile::tempdir().unwrap();
    let ws = init_workspace(dir.path()).unwrap();
    ws.create_problem("p1", "P1").unwrap();
    ws.compose_contest("r1", &["p1".into()], None).unwrap();

    // Workspace default option + contest override + per-problem option on `olymp-classic`.
    let mut ws_patch = BTreeMap::new();
    ws_patch.insert(
        "margin-x".to_owned(),
        Patch::Set(TemplateOptionValue::String("20mm".into())),
    );
    ws.update_workspace_template(
        &Patch::Keep,
        &BTreeMap::from([("olymp-classic".to_owned(), ws_patch)]),
    )
    .unwrap();

    let mut book_patch = BTreeMap::new();
    book_patch.insert(
        "logo".to_owned(),
        Patch::Set(TemplateOptionValue::String("assets/logo.png".into())),
    );
    ws.update_contest_meta(
        "r1",
        &ContestMetaPatch {
            template_options: BTreeMap::from([("olymp-classic".to_owned(), book_patch)]),
            ..Default::default()
        },
    )
    .unwrap();

    let mut prob_patch = BTreeMap::new();
    prob_patch.insert(
        "sample-layout".to_owned(),
        Patch::Set(TemplateOptionValue::String("wide".into())),
    );
    ws.set_override(
        "r1",
        "A",
        &OverridePatch {
            options: BTreeMap::from([("olymp-classic".to_owned(), prob_patch)]),
            ..Default::default()
        },
    )
    .unwrap();

    // Detaching copies both merged book options and per-problem options into `local`.
    ws.detach_contest_template("r1").unwrap();
    let c = ws.load_contest("r1").unwrap();
    let local_book = c.meta.template_options.get("local").unwrap();
    assert_eq!(
        local_book.get("margin-x"),
        Some(&TemplateOptionValue::String("20mm".into()))
    );
    assert_eq!(
        local_book.get("logo"),
        Some(&TemplateOptionValue::String("assets/logo.png".into()))
    );
    let local_prob = c.meta.problems[0].options.get("local").unwrap();
    assert_eq!(
        local_prob.get("sample-layout"),
        Some(&TemplateOptionValue::String("wide".into()))
    );

    // Typed Chinese messages for missing preset and missing template.
    let preset_msg = Error::NotFound("preset \"foo\"".into()).human_message();
    assert!(preset_msg.contains("找不到预设模板「foo」"));
    assert!(preset_msg.contains("olymp-classic"));
    assert!(preset_msg.contains("minimal"));
    let tpl_msg = Error::NotFound("template \"bar\"".into()).human_message();
    assert!(tpl_msg.contains("找不到模板「bar」"));
    assert!(tpl_msg.contains("xcpc template list"));
}

#[test]
fn preamble_lives_in_templates_and_is_reserved() {
    let dir = tempfile::tempdir().unwrap();
    let ws = init_workspace(dir.path()).unwrap();

    // Initially absent.
    assert!(ws.load_preamble().is_none());

    // ensure_preamble creates `templates/preamble.typ`.
    let preamble_path = ws.ensure_preamble().unwrap();
    assert!(preamble_path.ends_with("templates/preamble.typ"));
    assert!(preamble_path.is_file());
    assert!(ws.load_preamble().is_some());

    // Legacy `.xcpc/preamble.typ` is never created or read.
    assert!(!dir.path().join(".xcpc/preamble.typ").exists());

    // `preamble` is excluded from `list_templates`, `has_custom_template`, and `template_file_path`.
    assert!(!ws.list_templates().iter().any(|name| name == "preamble"));
    assert!(!ws.has_custom_template("preamble"));
    assert!(ws.template_file_path("preamble").unwrap().is_none());

    // `fork_template` and `install_preset_template` reject `"preamble"`.
    let err_fork = ws.fork_template("preamble", b"// nope").unwrap_err();
    assert_eq!(err_fork.code(), "invalid_file");
    let err_install = ws
        .install_preset_template("minimal", Some("preamble"))
        .unwrap_err();
    assert_eq!(err_install.code(), "invalid_file");

    // Watcher ignores `.xcpc/*` completely (including legacy paths) and watches `templates/preamble.typ`.
    assert!(watcher::should_ignore(".xcpc/preamble.typ"));
    assert!(watcher::should_ignore(".xcpc/theme.toml"));
    assert!(!watcher::should_ignore("templates/preamble.typ"));
}

#[test]
fn install_workspace_skills_and_upgraded_agents_md() {
    let dir = tempfile::tempdir().unwrap();
    let ws = init_workspace(dir.path()).unwrap();

    // Upgraded AGENTS.md documents Phase 3A workflow, anti-cheating discipline, and 5 MCP tools.
    let agents = fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
    assert!(agents.contains("xcpc-problem-creator"));
    assert!(agents.contains("inspect_package"));
    assert!(agents.contains("inspect_test_case"));
    assert!(agents.contains("init_package"));
    assert!(agents.contains("build_package"));
    assert!(agents.contains("stress_package"));
    assert!(agents.contains("Never relax `[limits]`"));
    assert!(agents.contains("xcpc package build"));

    // install_workspace_skills writes all 5 embedded files atomically.
    let written = install_workspace_skills(ws.root()).unwrap();
    assert_eq!(written.len(), 5);
    for rel in [
        ".agents/skills/xcpc-problem-creator/SKILL.md",
        ".agents/skills/xcpc-problem-creator/mistake-taxonomy.md",
        ".agents/skills/xcpc-problem-creator/judge-validator/SKILL.md",
        ".agents/skills/xcpc-problem-creator/solutions-tutorial/SKILL.md",
        ".agents/skills/xcpc-problem-creator/wrong-gen/SKILL.md",
    ] {
        assert!(
            written.iter().any(|p| p == rel),
            "missing {rel} in {written:?}"
        );
        let content = fs::read_to_string(dir.path().join(rel)).unwrap();
        assert!(!content.trim().is_empty(), "empty skill file {rel}");
    }
}

#[test]
fn watcher_and_problem_whitelist_cover_phase3a_package_paths() {
    let dir = tempfile::tempdir().unwrap();
    let ws = init_workspace(dir.path()).unwrap();
    ws.create_problem("pkg", "Package Problem").unwrap();

    // Package paths must not be ignored by the watcher.
    for rel in [
        "problems/pkg/validator.cpp",
        "problems/pkg/checker.cpp",
        "problems/pkg/generators/gen_random.cpp",
        "problems/pkg/solutions/std.cpp",
        "problems/pkg/tests/hand-01.in",
        "problems/pkg/tests/validator/valid/01.in",
        "problems/pkg/tests/validator/invalid/01.in",
        "problems/pkg/tests/checker/ok-01/in",
    ] {
        assert!(!watcher::should_ignore(rel), "should watch {rel}");
    }

    // write_problem_files accepts nested tests/ self-tests and versioned validator/checker files.
    let mut files = BTreeMap::new();
    files.insert("validator.20.cpp".into(), "int main(){}\n".into());
    files.insert("tests/validator/valid/01.in".into(), "1 2\n".into());
    files.insert("tests/checker/ok-01/in".into(), "1 2\n".into());
    let hashes = ws.write_problem_files("pkg", &files, None).unwrap();
    assert_eq!(hashes.len(), 3);

    // Still rejects path traversal and hidden files inside tests/.
    let mut bad = BTreeMap::new();
    bad.insert("tests/../escape.in".into(), "x".into());
    assert_eq!(
        ws.write_problem_files("pkg", &bad, None)
            .unwrap_err()
            .code(),
        "disallowed_path"
    );

    // Live watcher emits Changed events for external edits inside package directories.
    fs::create_dir_all(dir.path().join("problems/pkg/solutions")).unwrap();
    let (_tracked, _watcher, rx) = ws.watch().unwrap();
    fs::write(
        dir.path().join("problems/pkg/solutions/std.cpp"),
        "int main() { return 0; }\n",
    )
    .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    let mut saw_solution = false;
    while std::time::Instant::now() < deadline {
        if let Ok(WatchEvent::Changed(paths)) =
            rx.recv_timeout(std::time::Duration::from_millis(200))
            && paths.iter().any(|p| p == "problems/pkg/solutions/std.cpp")
        {
            saw_solution = true;
            break;
        }
    }
    assert!(
        saw_solution,
        "expected watcher event for problems/pkg/solutions/std.cpp"
    );
}

#[test]
fn groups_crud_preserves_toml_comments_and_unknown_fields() {
    let dir = tempfile::tempdir().unwrap();
    let ws = init_workspace(dir.path()).unwrap();

    // Seed workspace.toml with comments, unknown field, and template-options.
    let initial_ws_toml = "\
# workspace header comment
template = \"olymp-classic\"
custom_note = \"keep-me\"

[template-options.olymp-classic]
margin-x = \"18mm\"
";
    ws.write(WORKSPACE_FILE, initial_ws_toml.as_bytes())
        .unwrap();

    ws.create_problem("p1", "Problem 1").unwrap();
    ws.create_problem("p2", "Problem 2").unwrap();
    ws.create_problem("p3", "Problem 3").unwrap();

    // Add comment & unknown key to p1/problem.toml.
    let p1_raw = "\
# keep problem comment
title = \"Problem 1\"
extra_key = 42

[limits]
time = \"1s\"
memory = \"256MB\"
";
    ws.write("problems/p1/problem.toml", p1_raw.as_bytes())
        .unwrap();

    // Create declared groups (trimming whitespace).
    assert_eq!(ws.create_group("  省赛候选  ").unwrap(), "省赛候选");
    assert_eq!(ws.create_group("校赛备选").unwrap(), "校赛备选");
    // Duplicate rejected with GroupAlreadyExists.
    let dup = ws.create_group("省赛候选").unwrap_err();
    assert_eq!(dup.code(), "group_already_exists");
    assert!(dup.human_message().contains("省赛候选"));

    // Invalid group name rejected.
    let bad = ws.create_group("bad\nname").unwrap_err();
    assert_eq!(bad.code(), "invalid_group_name");
    assert!(bad.human_message().contains("无效"));

    // ensure_group_registered is idempotent.
    assert_eq!(ws.ensure_group_registered("省赛候选").unwrap(), "省赛候选");
    assert_eq!(ws.workspace_groups(), vec!["省赛候选", "校赛备选"]);

    // Assign p1 via set_problem_group, and give p2 an undeclared group directly in problem.toml.
    ws.set_problem_group("p1", Some("省赛候选")).unwrap();
    ws.update_problem_meta(
        "p2",
        &ProblemMetaPatch {
            group: Patch::Set("未声明组".into()),
            ..Default::default()
        },
    )
    .unwrap();

    let (groups, ungrouped) = ws.list_groups().unwrap();
    assert_eq!(ungrouped, 1); // p3
    assert_eq!(
        groups,
        vec![
            xcpc_domain::GroupSummary {
                name: "省赛候选".into(),
                count: 1,
                declared: true,
            },
            xcpc_domain::GroupSummary {
                name: "校赛备选".into(),
                count: 0,
                declared: true,
            },
            xcpc_domain::GroupSummary {
                name: "未声明组".into(),
                count: 1,
                declared: false,
            },
        ]
    );

    // Rename group preserves workspace.toml order and updates member problem.toml.
    let rename_rep = ws.rename_group("省赛候选", "区域赛候选").unwrap();
    assert_eq!(rename_rep.updated_problems, 1);
    assert_eq!(ws.workspace_groups(), vec!["区域赛候选", "校赛备选"]);
    assert_eq!(
        ws.load_problem("p1").unwrap().meta.group.as_deref(),
        Some("区域赛候选")
    );
    // Rename missing group -> NotFound with Chinese message.
    let nf = ws.rename_group("不存在", "新名字").unwrap_err();
    assert_eq!(nf.code(), "not_found");
    assert!(nf.human_message().contains("找不到分组「不存在」"));
    // Rename to existing undeclared group -> GroupAlreadyExists.
    let conflict = ws.rename_group("校赛备选", "未声明组").unwrap_err();
    assert_eq!(conflict.code(), "group_already_exists");

    // Reorder groups: strict permutation check.
    assert!(
        ws.reorder_groups(&["区域赛候选".into()])
            .unwrap_err()
            .code()
            == "invalid_file"
    );
    let reordered = ws
        .reorder_groups(&["校赛备选".into(), "区域赛候选".into()])
        .unwrap();
    assert_eq!(reordered, vec!["校赛备选", "区域赛候选"]);
    assert_eq!(ws.workspace_groups(), vec!["校赛备选", "区域赛候选"]);

    // Delete group clears member problems' group without deleting the problems.
    let del_rep = ws.delete_group("区域赛候选").unwrap();
    assert_eq!(del_rep.ungrouped_problems, 1);
    assert_eq!(ws.workspace_groups(), vec!["校赛备选"]);
    assert_eq!(ws.load_problem("p1").unwrap().meta.group, None);
    assert!(ws.load_problem("p1").is_ok());

    // Comments and unknown fields survived in both workspace.toml and p1/problem.toml.
    let ws_after = ws.read_text(WORKSPACE_FILE).unwrap();
    assert!(ws_after.contains("# workspace header comment"));
    assert!(ws_after.contains("custom_note = \"keep-me\""));
    assert!(ws_after.contains("margin-x = \"18mm\""));
    let p1_after = ws.read_text("problems/p1/problem.toml").unwrap();
    assert!(p1_after.contains("# keep problem comment"));
    assert!(p1_after.contains("extra_key = 42"));

    // write_problem_files validates meta.group.
    let mut bad_files = BTreeMap::new();
    bad_files.insert(
        "problem.toml".into(),
        "title = \"Bad\"\ngroup = \"   \"\n".into(),
    );
    assert_eq!(
        ws.write_problem_files("p1", &bad_files, None)
            .unwrap_err()
            .code(),
        "invalid_group_name"
    );
}

#[test]
fn index_groups_search_and_workspace_toml_refresh() {
    let dir = tempfile::tempdir().unwrap();
    let ws = init_workspace(dir.path()).unwrap();

    ws.create_group("ungrouped").unwrap(); // Literal group named "ungrouped" must not collide.
    ws.create_group("空分组").unwrap();
    ws.create_problem("p1", "Alpha").unwrap();
    ws.create_problem("p2", "Beta").unwrap();
    ws.set_problem_group("p1", Some("ungrouped")).unwrap();

    ws.compose_contest("c-active", &["p1".into()], None)
        .unwrap();
    ws.compose_contest("c-archived", &["p1".into()], None)
        .unwrap();
    ws.set_contest_archived("c-archived", true).unwrap();

    let mut idx = WorkspaceIndex::scan(&ws).unwrap();
    let (counts, ungrouped) = idx.group_counts();
    assert_eq!(ungrouped, 1);
    assert_eq!(counts.len(), 2);
    assert_eq!(counts[0].name, "ungrouped");
    assert_eq!(counts[0].count, 1);
    assert!(counts[0].declared);
    assert_eq!(counts[1].name, "空分组");
    assert_eq!(counts[1].count, 0);

    // GroupFilter::Named("ungrouped") vs GroupFilter::Ungrouped.
    let named_hits = idx.search_problems(&SearchQuery {
        group: Some(xcpc_domain::GroupFilter::Named("ungrouped".into())),
        ..Default::default()
    });
    assert_eq!(named_hits.len(), 1);
    assert_eq!(named_hits[0].id, "p1");

    let ungrouped_hits = idx.search_problems(&SearchQuery {
        group: Some(xcpc_domain::GroupFilter::Ungrouped),
        ..Default::default()
    });
    assert_eq!(ungrouped_hits.len(), 1);
    assert_eq!(ungrouped_hits[0].id, "p2");

    // Non-archived contest mapping helper.
    assert_eq!(idx.problem_contest_ids("p1"), vec!["c-active"]);
    assert!(idx.problem_contest_ids("p2").is_empty());
    let map = idx.problem_contest_map();
    assert_eq!(map.get("p1").unwrap(), &vec!["c-active".to_owned()]);
    assert!(!map.contains_key("p2"));

    // External edit to workspace.toml refreshed via refresh_path("workspace.toml").
    fs::write(
        dir.path().join("workspace.toml"),
        "groups = [\"空分组\", \"外部新增\"]\n",
    )
    .unwrap();
    idx.refresh_path(&ws, "workspace.toml").unwrap();
    let (counts2, _) = idx.group_counts();
    assert_eq!(counts2[0].name, "空分组");
    assert!(counts2[0].declared);
    assert_eq!(counts2[1].name, "外部新增");
    assert!(counts2[1].declared);
    assert_eq!(counts2[2].name, "ungrouped");
    assert!(!counts2[2].declared);
}

#[test]
fn batch_metadata_operations_and_read_only_guard() {
    let dir = tempfile::tempdir().unwrap();
    let ws = init_workspace(dir.path()).unwrap();
    ws.create_problem("p1", "P1").unwrap();
    ws.create_problem("p2", "P2").unwrap();

    // batch_set_group: dedups IDs, skips unchanged, fails missing/invalid, registers group once.
    let rep = ws
        .batch_set_group(
            &[
                "p1".into(),
                "p1".into(),
                "missing".into(),
                "BAD_ID".into(),
                "p2".into(),
            ],
            Some("省赛"),
        )
        .unwrap();
    assert!(!rep.ok);
    assert_eq!(rep.total, 4);
    assert_eq!(rep.succeeded, 2);
    assert_eq!(rep.skipped, 0);
    assert_eq!(rep.failed, 2);
    assert_eq!(ws.workspace_groups(), vec!["省赛"]);

    // Second call skips unchanged without writing.
    let rep2 = ws
        .batch_set_group(&["p1".into(), "p2".into()], Some("省赛"))
        .unwrap();
    assert!(rep2.ok);
    assert_eq!(rep2.succeeded, 0);
    assert_eq!(rep2.skipped, 2);

    // batch_edit_tags: add + case-insensitive remove.
    let tag_rep = ws
        .batch_edit_tags(
            &["p1".into(), "p2".into()],
            &["DP".into(), "dp".into(), " graph ".into()],
            &[],
        )
        .unwrap();
    assert!(tag_rep.ok);
    assert_eq!(tag_rep.succeeded, 2);
    assert_eq!(
        ws.load_problem("p1").unwrap().meta.tags,
        vec!["DP", "graph"]
    );

    let tag_rep2 = ws
        .batch_edit_tags(
            &["p1".into()],
            &["greedy".into()],
            &["dp".into(), "nonexistent".into()],
        )
        .unwrap();
    assert!(tag_rep2.ok);
    assert_eq!(tag_rep2.succeeded, 1);
    assert_eq!(
        ws.load_problem("p1").unwrap().meta.tags,
        vec!["graph", "greedy"]
    );

    // batch_set_rating + batch_update_problems.
    let rat_rep = ws
        .batch_set_rating(&["p1".into(), "p2".into()], Some(1600))
        .unwrap();
    assert!(rat_rep.ok);
    assert_eq!(rat_rep.succeeded, 2);
    let rat_skip = ws.batch_set_rating(&["p1".into()], Some(1600)).unwrap();
    assert_eq!(rat_skip.skipped, 1);

    let upd_rep = ws
        .batch_update_problems(
            &["p1".into(), "p2".into()],
            &Patch::Clear,
            &["math".into()],
            &["graph".into()],
            &Patch::Set(1900),
        )
        .unwrap();
    assert!(upd_rep.ok);
    assert_eq!(upd_rep.succeeded, 2);
    let p1 = ws.load_problem("p1").unwrap().meta;
    assert_eq!(p1.group, None);
    assert_eq!(p1.rating, Some(1900));
    assert_eq!(p1.tags, vec!["greedy", "math"]);

    // Read-only guard rejects all group & batch mutations.
    let ro = Workspace::open(dir.path(), true).unwrap();
    assert_eq!(ro.create_group("x").unwrap_err().code(), "read_only");
    assert_eq!(
        ro.batch_set_group(&["p1".into()], Some("x"))
            .unwrap_err()
            .code(),
        "read_only"
    );
    assert_eq!(
        ro.batch_edit_tags(&["p1".into()], &["a".into()], &[])
            .unwrap_err()
            .code(),
        "read_only"
    );
    assert_eq!(
        ro.batch_set_rating(&["p1".into()], Some(1200))
            .unwrap_err()
            .code(),
        "read_only"
    );
    assert_eq!(
        ro.batch_delete_problems(&["p1".into()]).unwrap_err().code(),
        "read_only"
    );
}

#[test]
fn batch_add_to_contest_guards_and_26_cap() {
    let dir = tempfile::tempdir().unwrap();
    let ws = init_workspace(dir.path()).unwrap();

    let mut all_ids = Vec::new();
    for i in 1..=27 {
        let id = format!("p{i:02}");
        ws.create_problem(&id, &format!("Problem {i}")).unwrap();
        all_ids.push(id);
    }

    ws.compose_contest("c1", &all_ids[0..2], None).unwrap();

    // Add p02 (duplicate -> Skipped), p03 (Ok), missing (Failed).
    let res = ws
        .batch_add_to_contest("c1", &["p02".into(), "p03".into(), "missing".into()])
        .unwrap();
    assert_eq!(res.added_ids, vec!["p03"]);
    assert_eq!(res.problem_count, 3);
    assert_eq!(res.report.succeeded, 1);
    assert_eq!(res.report.skipped, 1);
    assert_eq!(res.report.failed, 1);
    assert_eq!(
        res.report.items[0].code.as_deref(),
        Some("already_in_contest")
    );

    // Atomic 26-problem cap: adding p04..=p27 (24 problems) to 3 existing = 27 > 26 -> refused!
    let cap_err = ws.batch_add_to_contest("c1", &all_ids[3..27]).unwrap_err();
    assert_eq!(cap_err.code(), "too_many_problems");
    // Contest remains untouched at 3 problems.
    assert_eq!(ws.load_contest("c1").unwrap().meta.problems.len(), 3);

    // Archived contest refuses batch_add_to_contest.
    ws.set_contest_archived("c1", true).unwrap();
    assert_eq!(
        ws.batch_add_to_contest("c1", &["p04".into()])
            .unwrap_err()
            .code(),
        "contest_archived"
    );
    ws.set_contest_archived("c1", false).unwrap();

    // Frozen contest refuses batch_add_to_contest.
    ws.freeze_commit("c1", b"%PDF-1.4\n", None).unwrap();
    assert_eq!(
        ws.batch_add_to_contest("c1", &["p04".into()])
            .unwrap_err()
            .code(),
        "contest_frozen"
    );
}

#[test]
fn batch_delete_problems_reference_and_symlink_guards() {
    let dir = tempfile::tempdir().unwrap();
    let ws = init_workspace(dir.path()).unwrap();
    ws.create_problem("p-free", "Free").unwrap();
    ws.create_problem("p-active", "Active").unwrap();
    ws.create_problem("p-archived-unfrozen", "Archived Unfrozen")
        .unwrap();
    ws.create_problem("p-frozen", "Frozen").unwrap();

    ws.compose_contest("c-active", &["p-active".into()], None)
        .unwrap();
    ws.compose_contest("c-arch", &["p-archived-unfrozen".into()], None)
        .unwrap();
    ws.set_contest_archived("c-arch", true).unwrap();
    ws.compose_contest("c-frozen", &["p-frozen".into()], None)
        .unwrap();
    ws.freeze_commit("c-frozen", b"%PDF-1.4\n", None).unwrap();

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(
            dir.path().join("problems/p-active"),
            dir.path().join("problems/p-link"),
        )
        .unwrap();
    }

    let mut targets = vec![
        "p-free".into(),
        "p-active".into(),
        "p-archived-unfrozen".into(),
        "p-frozen".into(),
        "missing".into(),
    ];
    #[cfg(unix)]
    targets.push("p-link".into());

    let rep = ws.batch_delete_problems(&targets).unwrap();
    // p-free and p-frozen deleted; p-active and p-archived-unfrozen skipped; missing (+ p-link) failed.
    assert_eq!(rep.succeeded, 2);
    assert_eq!(rep.skipped, 2);
    #[cfg(unix)]
    assert_eq!(rep.failed, 2);
    #[cfg(not(unix))]
    assert_eq!(rep.failed, 1);

    assert!(ws.load_problem("p-free").is_err());
    assert!(ws.load_problem("p-frozen").is_err());
    assert!(ws.load_problem("p-active").is_ok());
    assert!(ws.load_problem("p-archived-unfrozen").is_ok());

    let active_item = rep.items.iter().find(|i| i.id == "p-active").unwrap();
    assert_eq!(active_item.status, xcpc_domain::BatchItemStatus::Skipped);
    assert_eq!(active_item.code.as_deref(), Some("problem_referenced"));
    assert_eq!(active_item.referencing_contests, vec!["c-active"]);

    let arch_item = rep
        .items
        .iter()
        .find(|i| i.id == "p-archived-unfrozen")
        .unwrap();
    assert_eq!(arch_item.status, xcpc_domain::BatchItemStatus::Skipped);
    assert_eq!(arch_item.referencing_contests, vec!["c-arch"]);

    #[cfg(unix)]
    {
        let link_item = rep.items.iter().find(|i| i.id == "p-link").unwrap();
        assert_eq!(link_item.status, xcpc_domain::BatchItemStatus::Failed);
        assert_eq!(link_item.code.as_deref(), Some("path_escape"));
    }
}
