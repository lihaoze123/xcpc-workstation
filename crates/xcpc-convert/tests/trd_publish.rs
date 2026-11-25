//! Integration tests for Phase 5 TRD & single-snippet publishing (`xcpc-convert` + `xcpc-engine`).

use std::collections::BTreeMap;

use xcpc_convert::publish;
use xcpc_domain::{Patch, TemplateKind, TemplateOptionValue, TrdMetaPatch};
use xcpc_engine::FontOptions;
use xcpc_storage::init_example_workspace;

#[test]
fn example_workspace_trd_compiles_with_trd_compact_and_trd_book() {
    let dir = tempfile::tempdir().unwrap();
    let ws = init_example_workspace(dir.path()).unwrap();
    let mut engine = publish::engine(&ws, &FontOptions::from_env());

    // TemplateKind isolation: statement templates vs TRD templates.
    let stmt_templates = publish::list_templates_by_kind(&ws, &engine, TemplateKind::Statement);
    let trd_templates = publish::list_templates_by_kind(&ws, &engine, TemplateKind::Trd);
    assert!(stmt_templates.iter().any(|t| t.id == "olymp-classic"));
    assert!(!stmt_templates.iter().any(|t| t.id == "trd-compact"));
    assert!(trd_templates.iter().any(|t| t.id == "trd-compact"));
    assert!(trd_templates.iter().any(|t| t.id == "trd-book"));
    assert!(!trd_templates.iter().any(|t| t.id == "olymp-classic"));

    // 1. Compile & export example `trds/icpc-regional` with `trd-compact`.
    let (pdf_compact, doc_compact, report_compact) =
        publish::compile_trd_pdf_bytes(&ws, &mut engine, "icpc-regional").unwrap();
    assert!(
        report_compact.ok,
        "trd-compact failed: {:?}",
        report_compact.diagnostics
    );
    let pdf_compact = pdf_compact.expect("expected PDF bytes for trd-compact");
    assert!(pdf_compact.starts_with(b"%PDF-"));
    assert!(report_compact.pages.unwrap_or(0) >= 1);
    assert!(doc_compact.source.contains("/templates/trd-compact.typ"));
    assert!(doc_compact.source.contains("sed -n '1,Np' a.cpp"));

    let (png_compact, png_report) =
        publish::render_trd_page_png(&ws, &mut engine, "icpc-regional", 1, 1.5).unwrap();
    assert!(png_report.ok);
    let png_compact = png_compact.expect("expected PNG page 1");
    assert!(png_compact.starts_with(b"\x89PNG"));

    // 2. Switch TRD template to `trd-book` and compile & export.
    ws.update_trd_meta(
        "icpc-regional",
        &TrdMetaPatch {
            template: Patch::Set("trd-book".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let (pdf_book, doc_book, report_book) =
        publish::compile_trd_pdf_bytes(&ws, &mut engine, "icpc-regional").unwrap();
    assert!(
        report_book.ok,
        "trd-book failed: {:?}",
        report_book.diagnostics
    );
    let pdf_book = pdf_book.expect("expected PDF bytes for trd-book");
    assert!(pdf_book.starts_with(b"%PDF-"));
    assert!(report_book.pages.unwrap_or(0) >= 1);
    assert!(doc_book.source.contains("/templates/trd-book.typ"));
}

#[test]
fn single_snippet_preview_works_for_code_and_pure_formula_card() {
    let dir = tempfile::tempdir().unwrap();
    let ws = init_example_workspace(dir.path()).unwrap();
    let mut engine = publish::engine(&ws, &FontOptions::from_env());

    // Code snippet `ntt` (depends on `modint`, has `code.hpp` + `notes.md`).
    let (compiled_ntt, doc_ntt, report_ntt) =
        publish::compile_snippet_preview(&ws, &mut engine, "ntt", Some("trd-compact"), None)
            .unwrap();
    assert!(
        report_ntt.ok,
        "ntt snippet preview failed: {:?}",
        report_ntt.diagnostics
    );
    assert!(compiled_ntt.is_some());
    assert!(doc_ntt.source.contains("deps: (\"modint\",)"));
    assert!(doc_ntt.source.contains("has-code: true"));

    let (png_ntt, _) = publish::render_snippet_page_png(
        &ws,
        &mut engine,
        "ntt",
        Some("trd-compact"),
        1,
        1.5,
        None,
    )
    .unwrap();
    assert!(png_ntt.unwrap().starts_with(b"\x89PNG"));

    // Unsaved live overlay for `ntt/code.hpp`.
    let (_, doc_unsaved, report_unsaved) = publish::compile_snippet_preview(
        &ws,
        &mut engine,
        "ntt",
        Some("trd-compact"),
        Some((
            "snippets/ntt/code.hpp",
            "#pragma once\nint fast_ntt_live_edit() { return 42; }\n",
        )),
    )
    .unwrap();
    assert!(report_unsaved.ok);
    assert!(doc_unsaved.source.contains("fast_ntt_live_edit"));

    // Pure formula card `number-theory-facts` (no `code.hpp`, only `notes.md` with math + table).
    let (compiled_nt, doc_nt, report_nt) = publish::compile_snippet_preview(
        &ws,
        &mut engine,
        "number-theory-facts",
        Some("trd-book"),
        None,
    )
    .unwrap();
    assert!(
        report_nt.ok,
        "number-theory-facts preview failed: {:?}",
        report_nt.diagnostics
    );
    assert!(compiled_nt.is_some());
    assert!(doc_nt.source.contains("has-code: false"));

    let (png_nt, _) = publish::render_snippet_page_png(
        &ws,
        &mut engine,
        "number-theory-facts",
        Some("trd-book"),
        1,
        1.5,
        None,
    )
    .unwrap();
    assert!(png_nt.unwrap().starts_with(b"\x89PNG"));
}

#[test]
fn trd_local_template_override_and_frozen_snapshot_reproduction() {
    let dir = tempfile::tempdir().unwrap();
    let ws = init_example_workspace(dir.path()).unwrap();
    let mut engine = publish::engine(&ws, &FontOptions::from_env());

    // Local `trds/<id>/template.typ` takes priority over `templates/trd-compact.typ`.
    let custom_src = xcpc_engine::preset_source("trd-book").unwrap();
    ws.write("trds/icpc-regional/template.typ", custom_src.as_bytes())
        .unwrap();
    assert!(publish::inspect_trd_template(&ws, &engine, "icpc-regional").is_some());

    let doc_local = publish::document_trd(&ws, "icpc-regional").unwrap();
    assert!(
        doc_local
            .source
            .contains("/trds/icpc-regional/template.typ")
    );

    // Freeze TRD with PDF + template archive.
    let statuses = BTreeMap::new();
    let (frozen_res, report) =
        publish::freeze_trd_with_pdf(&ws, &mut engine, "icpc-regional", false, &statuses).unwrap();
    assert!(report.ok, "freeze compile failed: {:?}", report.diagnostics);
    let frozen_res = frozen_res.expect("expected TrdFreezeResult");
    assert!(frozen_res.pdf_archived);
    assert!(
        dir.path()
            .join("trds/icpc-regional/snapshot/icpc-regional.pdf")
            .is_file()
    );
    assert!(
        dir.path()
            .join("trds/icpc-regional/snapshot/template.typ")
            .is_file()
    );

    // Mutate live `modint` after freeze; frozen TRD still reads from snapshot/.
    ws.write(
        "snippets/modint/code.hpp",
        b"int mutated_after_freeze() {}\n",
    )
    .unwrap();
    let (compiled_snap, doc_snap, report_snap) =
        publish::compile_trd(&ws, &mut engine, "icpc-regional").unwrap();
    assert!(
        report_snap.ok,
        "frozen compile failed: {:?}",
        report_snap.diagnostics
    );
    assert!(compiled_snap.is_some());
    assert!(
        doc_snap
            .source
            .contains("/trds/icpc-regional/snapshot/template.typ")
    );
    assert!(!doc_snap.source.contains("mutated_after_freeze"));

    // Strip-comments option strips comments before hashing.
    let printed_raw =
        xcpc_convert::prepare_printed_code("// comment\nint f() { return 1; }\n", 10, false);
    let printed_stripped =
        xcpc_convert::prepare_printed_code("// comment\nint f() { return 1; }\n", 10, true);
    assert!(printed_raw.code.contains("// comment"));
    assert!(!printed_stripped.code.contains("// comment"));
    assert_eq!(printed_raw.total_hash, printed_stripped.total_hash);
    assert_eq!(
        TemplateOptionValue::Bool(true),
        TemplateOptionValue::Bool(true)
    );
}
