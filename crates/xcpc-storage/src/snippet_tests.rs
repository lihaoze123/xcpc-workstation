//! Phase 5 unit tests for snippets, bundler, reverse index, TRD preflight/freeze/unfreeze,
//! workspace `snippets` setting, watcher coverage, and example workspace seeds.

use std::collections::BTreeMap;
use std::fs;
use std::time::Duration;

use xcpc_domain::{
    BundleOptions, Patch, SnippetMeta, SnippetMetaPatch, SnippetVerifySpec, TrdChapter,
    TrdMetaPatch, TrdPreflightKind, TrdPreflightSeverity, TrdSnippetDiffKind,
};

use crate::{
    Error, SnapshotTemplateArchive, SnapshotTemplateMeta, WatchEvent, Workspace,
    init_example_workspace, init_example_workspace_with_options, init_workspace,
};

#[test]
fn workspace_snippets_enabled_default_and_preserves_unknown_fields() {
    let dir = tempfile::tempdir().unwrap();
    let ws = init_workspace(dir.path()).unwrap();
    assert!(ws.snippets_enabled());
    assert!(ws.load_workspace_meta().unwrap().snippets_enabled());

    ws.write(
        "workspace.toml",
        b"# keep this comment\ntemplate = \"olymp-classic\"\ncustom_key = 123\n",
    )
    .unwrap();
    assert!(ws.snippets_enabled());

    ws.set_snippets_enabled(false).unwrap();
    assert!(!ws.snippets_enabled());
    let raw = ws.read_text("workspace.toml").unwrap();
    assert!(raw.contains("# keep this comment"));
    assert!(raw.contains("custom_key = 123"));
    assert!(raw.contains("snippets = false"));

    ws.set_snippets_enabled(true).unwrap();
    assert!(ws.snippets_enabled());

    let ro = Workspace::open(dir.path(), true).unwrap();
    assert!(matches!(
        ro.set_snippets_enabled(false),
        Err(Error::ReadOnly)
    ));
}

#[test]
fn snippet_crud_preserves_toml_comments_and_reverse_index() {
    let dir = tempfile::tempdir().unwrap();
    let ws = init_workspace(dir.path()).unwrap();

    // Seed a workspace problem referenced by snippet.
    ws.write("problems/poly-mul/problem.toml", b"title = \"Poly Mul\"\n")
        .unwrap();

    let mod_meta = SnippetMeta {
        title: "ModInt".into(),
        tags: vec!["math".into()],
        time_complexity: Some("O(1)".into()),
        ..Default::default()
    };
    ws.create_snippet(
        "modint",
        &mod_meta,
        Some("#pragma once\nstruct Mint { int v; };\n"),
        Some("Notes for modint."),
    )
    .unwrap();

    // Add comment and unknown field to snippet.toml, then patch via toml_edit.
    ws.write(
        "snippets/modint/snippet.toml",
        b"# top comment\ntitle = \"ModInt\"\ncustom_extra = \"keep\"\n",
    )
    .unwrap();
    let updated = ws
        .update_snippet_meta(
            "modint",
            &SnippetMetaPatch {
                title: Some("ModInt v2".into()),
                tags: Some(vec!["math".into(), "ntt".into()]),
                description: Patch::Set("fast modular arithmetic".into()),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(updated.title, "ModInt v2");
    let raw = ws.read_text("snippets/modint/snippet.toml").unwrap();
    assert!(raw.contains("# top comment"));
    assert!(raw.contains("custom_extra = \"keep\""));

    // Create downstream snippet `ntt` referencing `modint` and workspace problem `poly-mul`.
    let ntt_meta = SnippetMeta {
        title: "NTT".into(),
        source_problem: Some("poly-mul".into()),
        verify: vec![SnippetVerifySpec::Problem {
            problem: "poly-mul".into(),
            adapter: "verify_poly.cpp".into(),
        }],
        ..Default::default()
    };
    ws.create_snippet(
        "ntt",
        &ntt_meta,
        Some("#pragma once\n#include \"modint/code.hpp\"\nvoid ntt() {}\n"),
        Some("See problems/poly-mul for details."),
    )
    .unwrap();

    let mod_detail = ws.load_snippet("modint").unwrap();
    assert_eq!(mod_detail.reverse_refs.reverse_deps, vec!["ntt"]);
    assert!(mod_detail.reverse_refs.deps.is_empty());

    let ntt_detail = ws.load_snippet("ntt").unwrap();
    assert_eq!(ntt_detail.reverse_refs.deps, vec!["modint"]);
    assert_eq!(ntt_detail.reverse_refs.transitive_deps, vec!["modint"]);
    assert_eq!(ntt_detail.reverse_refs.related_problems, vec!["poly-mul"]);

    // Cascade topo order puts upstream `modint` before downstream `ntt`.
    assert_eq!(
        ws.snippet_dependents_topo("modint").unwrap(),
        vec!["modint", "ntt"]
    );

    // Delete works and enforces read-only.
    ws.delete_snippet("ntt").unwrap();
    assert_eq!(ws.list_snippets().unwrap(), vec!["modint"]);
}

#[test]
fn bundler_handles_diamond_deps_include_guards_cycles_and_sandbox() {
    let dir = tempfile::tempdir().unwrap();
    let ws = init_workspace(dir.path()).unwrap();

    // Diamond: a -> b, c -> modint (modint uses classic #ifndef guard).
    ws.write("snippets/modint/snippet.toml", b"title = \"ModInt\"\n")
        .unwrap();
    ws.write(
        "snippets/modint/code.hpp",
        b"#ifndef MODINT_HPP\n#define MODINT_HPP\n#include <cstdint>\n// modint comment\nstruct Mint { uint32_t v; };\n#endif // MODINT_HPP\n",
    )
    .unwrap();
    ws.write("snippets/b/snippet.toml", b"title = \"B\"\n")
        .unwrap();
    ws.write(
        "snippets/b/code.hpp",
        b"#pragma once\n#include <vector>\n#include \"modint/code.hpp\"\ninline Mint fb(Mint x) { return x; }\n",
    )
    .unwrap();
    ws.write("snippets/c/snippet.toml", b"title = \"C\"\n")
        .unwrap();
    ws.write(
        "snippets/c/code.hpp",
        b"#pragma once\n#include <vector>\n#include \"modint/code.hpp\"\ninline Mint fc(Mint x) { return x; }\n",
    )
    .unwrap();
    ws.write("snippets/a/snippet.toml", b"title = \"A\"\n")
        .unwrap();
    ws.write(
        "snippets/a/code.hpp",
        b"#pragma once\n#include \"b/code.hpp\"\n#include \"c/code.hpp\"\ninline Mint fa(Mint x) { return fb(fc(x)); }\n",
    )
    .unwrap();

    let bundled = ws.bundle_snippet("a", &BundleOptions::default()).unwrap();
    // `struct Mint` appears exactly once (diamond deduped, include guard stripped).
    assert_eq!(bundled.code.matches("struct Mint").count(), 1);
    assert!(!bundled.code.contains("#ifndef MODINT_HPP"));
    assert!(!bundled.code.contains("#pragma once"));
    // System includes deduplicated in first-seen order.
    assert_eq!(bundled.system_includes, vec!["vector", "cstdint"]);
    assert!(bundled.code.contains("// --- modint/code.hpp ---"));

    // Compact mode strips comments and boundary markers.
    let compact = ws
        .bundle_snippet(
            "a",
            &BundleOptions {
                compact: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(!compact.code.contains("// modint comment"));
    assert!(!compact.code.contains("// ---"));
    assert_eq!(compact.code.matches("struct Mint").count(), 1);

    // Local unrestricted file + external `-I` directory works.
    let ext_dir = tempfile::tempdir().unwrap();
    let ext_inc = ext_dir.path().join("my_inc");
    fs::create_dir_all(&ext_inc).unwrap();
    fs::write(
        ext_inc.join("helper.hpp"),
        "#pragma once\nint helper() { return 42; }\n",
    )
    .unwrap();
    let sol_file = ext_dir.path().join("sol.cpp");
    fs::write(
        &sol_file,
        "#include <iostream>\n#include \"helper.hpp\"\n#include \"a/code.hpp\"\nint main() { std::cout << helper(); }\n",
    )
    .unwrap();

    let local_res = ws
        .bundle_local_file(
            &sol_file,
            &BundleOptions {
                include_dirs: vec![ext_inc.to_string_lossy().into_owned()],
                ..Default::default()
            },
        )
        .unwrap();
    assert!(local_res.code.contains("int helper()"));
    assert_eq!(local_res.code.matches("struct Mint").count(), 1);

    // MCP sandbox rejects external `-I` path.
    let mcp_err = ws
        .bundle_workspace_code(
            None,
            Some("#include \"helper.hpp\"\n"),
            &BundleOptions {
                include_dirs: vec![ext_inc.to_string_lossy().into_owned()],
                ..Default::default()
            },
        )
        .unwrap_err();
    assert!(matches!(mcp_err, Error::PathEscape(_)));

    // Missing include reports file + line.
    let miss = ws
        .bundle_workspace_code(
            None,
            Some("int x;\n#include \"no_such.hpp\"\n"),
            &BundleOptions::default(),
        )
        .unwrap_err();
    match miss {
        Error::MissingInclude { line, include, .. } => {
            assert_eq!(line, 2);
            assert_eq!(include, "no_such.hpp");
        }
        other => panic!("expected MissingInclude, got {other:?}"),
    }

    // Cycle detection reports structured error with file + line.
    ws.write("snippets/cyc1/snippet.toml", b"title = \"C1\"\n")
        .unwrap();
    ws.write("snippets/cyc1/code.hpp", b"#include \"cyc2/code.hpp\"\n")
        .unwrap();
    ws.write("snippets/cyc2/snippet.toml", b"title = \"C2\"\n")
        .unwrap();
    ws.write("snippets/cyc2/code.hpp", b"\n#include \"cyc1/code.hpp\"\n")
        .unwrap();
    let cyc_err = ws
        .bundle_snippet("cyc1", &BundleOptions::default())
        .unwrap_err();
    match cyc_err {
        Error::DependencyCycle { line, cycle, .. } => {
            assert_eq!(line, 2);
            assert!(cycle.len() >= 2);
        }
        other => panic!("expected DependencyCycle, got {other:?}"),
    }
}

#[test]
fn trd_preflight_freeze_strict_soft_and_unfreeze_diff() {
    let dir = tempfile::tempdir().unwrap();
    let ws = init_example_workspace(dir.path()).unwrap();

    // Missing transitive dependency: include `ntt` without `modint`.
    ws.compose_trd(
        "broken-trd",
        &[TrdChapter {
            title: "Poly".into(),
            items: vec!["ntt".into()],
        }],
        Some(&TrdMetaPatch {
            name: Some("Broken TRD".into()),
            ..Default::default()
        }),
    )
    .unwrap();

    let mut statuses = BTreeMap::new();
    let issues = ws.preflight_trd("broken-trd", &statuses).unwrap();
    assert!(issues.iter().any(|i| {
        i.severity == TrdPreflightSeverity::Error
            && i.kind == TrdPreflightKind::MissingDependency
            && i.snippet == "ntt"
            && i.missing_deps == vec!["modint"]
    }));
    // Hard error blocks freeze even when strict = false.
    assert!(matches!(
        ws.freeze_trd("broken-trd", false, &statuses, None, None),
        Err(Error::TrdFreezeBlocked { .. })
    ));

    // Valid example TRD `icpc-regional` has all deps; unverified code snippets warn.
    let reg_issues = ws.preflight_trd("icpc-regional", &statuses).unwrap();
    assert!(
        reg_issues
            .iter()
            .all(|i| i.severity == TrdPreflightSeverity::Warning)
    );
    // Pure formula card `number-theory-facts` has no code.hpp, so no unverified warning!
    assert!(
        !reg_issues
            .iter()
            .any(|i| i.snippet == "number-theory-facts")
    );

    // Strict freeze blocks on unverified warnings.
    assert!(matches!(
        ws.freeze_trd("icpc-regional", true, &statuses, None, None),
        Err(Error::TrdFreezeBlocked { .. })
    ));

    // Soft freeze succeeds with Stale warning, archives snapshot + template + PDF, sets frozen = true.
    statuses.insert("modint".into(), xcpc_domain::SnippetVerifyStatus::Stale);
    let tpl_archive = SnapshotTemplateArchive {
        meta: SnapshotTemplateMeta {
            template: "trd-compact".into(),
            ..Default::default()
        },
        ..Default::default()
    };
    let freeze_res = ws
        .freeze_trd(
            "icpc-regional",
            false,
            &statuses,
            Some(&tpl_archive),
            Some(b"%PDF-1.7 mock"),
        )
        .unwrap();
    assert!(freeze_res.pdf_archived);
    assert_eq!(freeze_res.snippet_count, 4);
    assert_eq!(freeze_res.warnings.len(), 1);
    assert_eq!(freeze_res.warnings[0].kind, TrdPreflightKind::StaleSnippet);

    // Frozen TRD rejects writes.
    assert!(matches!(
        ws.update_trd_meta(
            "icpc-regional",
            &TrdMetaPatch {
                name: Some("Mutated".into()),
                ..Default::default()
            }
        ),
        Err(Error::TrdFrozen(_))
    ));

    // Modify live `modint/code.hpp` -> frozen TRD still reads old snapshot, and diff reports CodeChanged.
    let mut live_code = ws.read_text("snippets/modint/code.hpp").unwrap();
    live_code.push_str("// edited after freeze\n");
    ws.write("snippets/modint/code.hpp", live_code.as_bytes())
        .unwrap();

    let frozen_trd = ws.load_trd("icpc-regional").unwrap();
    let snap_chapters = ws.trd_snippets(&frozen_trd).unwrap();
    let snap_mod = &snap_chapters[0].1[0];
    assert!(
        !snap_mod
            .code
            .as_ref()
            .unwrap()
            .text
            .contains("edited after freeze")
    );

    let diff = ws.diff_trd_for_unfreeze("icpc-regional").unwrap();
    assert!(diff.has_changes);
    assert_eq!(diff.diffs.len(), 1);
    assert_eq!(diff.diffs[0].slug, "modint");
    assert_eq!(diff.diffs[0].kinds, vec![TrdSnippetDiffKind::CodeChanged]);

    // Unfreeze restores editability and removes snapshot/.
    let unfrozen = ws.unfreeze_trd("icpc-regional").unwrap();
    assert!(!unfrozen.meta.frozen);
    assert!(!unfrozen.has_snapshot);
}

#[test]
fn example_workspaces_seed_snippets_and_trd_and_watcher_observes_them() {
    let dir_zh = tempfile::tempdir().unwrap();
    let ws_zh = init_example_workspace(dir_zh.path()).unwrap();
    assert_eq!(
        ws_zh.list_snippets().unwrap(),
        vec!["dsu", "modint", "ntt", "number-theory-facts"]
    );
    assert_eq!(ws_zh.list_trds().unwrap(), vec!["icpc-regional"]);
    let card = ws_zh.load_snippet("number-theory-facts").unwrap();
    assert!(card.code.is_none());
    assert!(card.notes.is_some());

    let dir_en = tempfile::tempdir().unwrap();
    let ws_en =
        init_example_workspace_with_options(dir_en.path(), None, xcpc_domain::Language::English)
            .unwrap();
    assert_eq!(
        ws_en.list_snippets().unwrap(),
        vec!["dsu", "modint", "ntt", "number-theory-facts"]
    );
    assert_eq!(ws_en.list_trds().unwrap(), vec!["icpc-regional"]);

    // Watcher observes external edits in `snippets/` and `trds/`.
    let (_tracked, _watcher, rx) = ws_zh.watch().unwrap();
    fs::write(
        ws_zh.root().join("snippets/dsu/notes.md"),
        "External edit to DSU notes.\n",
    )
    .unwrap();
    let mut saw_snippet = false;
    for _ in 0..10 {
        if let Ok(WatchEvent::Changed(paths)) = rx.recv_timeout(Duration::from_millis(400))
            && paths.iter().any(|p| p == "snippets/dsu/notes.md")
        {
            saw_snippet = true;
            break;
        }
    }
    assert!(
        saw_snippet,
        "watcher should report snippets/ external edits"
    );
}
