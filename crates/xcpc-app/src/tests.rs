use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::Value as JsonValue;
use xcpc_domain::Patch;

use crate::AppController as Preview;
use crate::cache::{CancelToken, Job, LayerKind, PixelCache, RgbaPixels, render_chunk, tiles};
use crate::types::{
    AppPrefsJs, BatchExportRequestJs, ContestMetaPatchJs, OverridePatchJs, PreviewRequest,
    PreviewView, ProblemMetaPatchJs, ProblemSearchQuery, ProblemSourceJs, SamplePairJs,
};

fn rgba_src(w: u32, h: u32) -> (RgbaPixels, usize) {
    let bytes = (w as usize) * (h as usize) * 4;
    (RgbaPixels::new(w, h, vec![255u8; bytes]), bytes)
}

fn fixture_preview() -> Preview {
    let preview = Preview::new(None);
    let info = preview.open_workspace(Preview::fixture(), true);
    assert!(info.error.is_none(), "fixture error: {:?}", info.error);
    preview
}

fn contest_request(path: &str, text: &str) -> PreviewRequest {
    PreviewRequest {
        target: "contest".into(),
        id: "contest-zh".into(),
        book: "statements".into(),
        path: path.into(),
        text: text.into(),
    }
}

fn page_view(page: u32) -> PreviewView {
    PreviewView {
        page,
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
    }
}

struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("xcpc-desktop-{label}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn init_temp_preview(label: &str) -> (TempDir, Preview) {
    let dir = TempDir::new(label);
    let preview = Preview::new(None);
    let info = preview.init_workspace(dir.0.display().to_string(), None);
    assert!(info.error.is_none(), "init error: {:?}", info.error);
    (dir, preview)
}

// ---- Wave 1 regression tests (must remain 100% green) ----------------------------------

#[test]
fn pixel_cache_enforces_byte_budget_and_lru_order() {
    // Each 10x10 RGBA buffer is exactly 400 bytes; budget 1000 fits at most 2.
    let mut cache = PixelCache::with_budget(1000);
    let (s1, b1) = rgba_src(10, 10);
    let (s2, b2) = rgba_src(10, 10);
    let (s3, b3) = rgba_src(10, 10);
    assert_eq!(b1, 400);

    cache.insert("k1".into(), s1, b1, LayerKind::Tile, 1);
    cache.insert("k2".into(), s2, b2, LayerKind::Tile, 1);
    assert_eq!(cache.total_bytes, 800);

    // Touch k1 so k2 becomes the least recently used entry.
    assert!(cache.get("k1").is_some());

    cache.insert("k3".into(), s3, b3, LayerKind::Tile, 1);
    assert_eq!(cache.total_bytes, 1200);
    cache.evict(&HashSet::new());

    assert_eq!(cache.total_bytes, 800);
    assert!(cache.entries.contains_key("k1"));
    assert!(!cache.entries.contains_key("k2"));
    assert!(cache.entries.contains_key("k3"));

    // Overwriting an existing key adjusts total_bytes without double counting.
    let (s3_big, b3_big) = rgba_src(10, 15);
    assert_eq!(b3_big, 600);
    cache.insert("k3".into(), s3_big, b3_big, LayerKind::Tile, 1);
    assert_eq!(cache.total_bytes, 1000);
}

#[test]
fn pixel_cache_protects_active_and_evicts_tiles_before_base() {
    let mut cache = PixelCache::with_budget(1000);
    let (sb, bb) = rgba_src(10, 10);
    let (st1, bt1) = rgba_src(10, 10);
    let (st_act, bt_act) = rgba_src(10, 10);
    let (st2, bt2) = rgba_src(10, 10);

    // Insert base first (oldest tick), then 3 tiles (newer ticks).
    cache.insert("base-old".into(), sb, bb, LayerKind::Base, 1);
    cache.insert("tile-old".into(), st1, bt1, LayerKind::Tile, 1);
    cache.insert("tile-active".into(), st_act, bt_act, LayerKind::Tile, 1);
    cache.insert("tile-new".into(), st2, bt2, LayerKind::Tile, 1);
    assert_eq!(cache.total_bytes, 1600);

    let mut protected = HashSet::from(["tile-active".to_string()]);
    cache.evict(&protected);

    // Non-protected tiles are evicted first even though base-old has an older tick.
    assert_eq!(cache.total_bytes, 800);
    assert!(cache.entries.contains_key("base-old"));
    assert!(cache.entries.contains_key("tile-active"));
    assert!(!cache.entries.contains_key("tile-old"));
    assert!(!cache.entries.contains_key("tile-new"));

    // Adding another active layer forces eviction of base-old once no non-protected tiles remain.
    let (sb_act, bb_act) = rgba_src(10, 10);
    cache.insert("base-active".into(), sb_act, bb_act, LayerKind::Base, 1);
    protected.insert("base-active".into());
    cache.evict(&protected);

    assert_eq!(cache.total_bytes, 800);
    assert!(!cache.entries.contains_key("base-old"));
    assert!(cache.entries.contains_key("tile-active"));
    assert!(cache.entries.contains_key("base-active"));

    // If protected layers alone exceed the budget, eviction keeps them all and terminates.
    let (st_act2, bt_act2) = rgba_src(10, 10);
    cache.insert("tile-active-2".into(), st_act2, bt_act2, LayerKind::Tile, 1);
    protected.insert("tile-active-2".into());
    cache.evict(&protected);
    assert_eq!(cache.total_bytes, 1200);
    assert_eq!(cache.entries.len(), 3);
}

#[test]
fn compiled_cache_hits_and_invalidates_on_save_and_switch() {
    let dir = TempDir::new("cache-hit");
    xcpc_storage::init_workspace(&dir.0).unwrap();
    let pdir = dir.0.join("problems/p1");
    std::fs::create_dir_all(&pdir).unwrap();
    std::fs::write(
        pdir.join("problem.toml"),
        "title = \"Demo\"\n[limits]\ntime = \"1s\"\nmemory = \"256MB\"\n",
    )
    .unwrap();
    let md = "## 题目描述\n\n给定正整数 $n$。\n\n## 输入格式\n\n一个整数。\n\n## 输出格式\n\n一个整数。\n";
    std::fs::write(pdir.join("statement.md"), md).unwrap();

    let preview = Preview::new(None);
    let info = preview.open_workspace(dir.0.display().to_string(), false);
    assert!(info.error.is_none());

    let req = PreviewRequest {
        target: "problem".into(),
        id: "p1".into(),
        book: "statements".into(),
        path: "problems/p1/statement.md".into(),
        text: md.into(),
    };
    let latest_gen = AtomicU64::new(1);
    let token = CancelToken {
        generation: 1,
        latest: &latest_gen,
    };

    // First run compiles from scratch.
    let first = preview.state.run(&req, 1.0, &[page_view(0)], token);
    assert!(first.error.is_none());
    assert_eq!(first.pages.len(), 1);
    assert!(first.compile_ms > 0.0);

    // Changing only scale/view hits the cached Compiled with compile_ms == 0.0.
    let second = preview.state.run(&req, 1.5, &[page_view(0)], token);
    assert!(second.error.is_none());
    assert_eq!(second.compile_ms, 0.0);
    assert_eq!(second.pages.len(), 1);

    // save() with the EXACT same Markdown already compiled preserves the cache (no redundant recompile).
    let rev_before_same_save = preview.state.revision();
    let saved_same = preview.save("problems/p1/statement.md".into(), md.into(), None);
    assert!(saved_same.ok, "save failed: {:?}", saved_same.error);
    assert_eq!(preview.state.revision(), rev_before_same_save);
    assert!(preview.state.snapshot.lock().unwrap().is_some());
    let after_same_save = preview.state.run(&req, 1.5, &[page_view(0)], token);
    assert!(after_same_save.error.is_none());
    assert_eq!(after_same_save.compile_ms, 0.0);

    // Even a disk-only request (`path: ""`) hits the promoted disk-clean cache!
    let disk_req = PreviewRequest {
        path: String::new(),
        text: String::new(),
        ..req.clone()
    };
    let after_same_save_disk = preview.state.run(&disk_req, 1.5, &[page_view(0)], token);
    assert!(after_same_save_disk.error.is_none());
    assert_eq!(after_same_save_disk.compile_ms, 0.0);

    // save() with DIFFERENT text increments revision and invalidates cached Compiled.
    let md2 = format!("{md}\n补充说明。\n");
    let rev_before_diff_save = preview.state.revision();
    let saved_diff = preview.save("problems/p1/statement.md".into(), md2.clone(), None);
    assert!(saved_diff.ok, "save failed: {:?}", saved_diff.error);
    assert_eq!(preview.state.revision(), rev_before_diff_save + 1);
    assert!(preview.state.snapshot.lock().unwrap().is_none());
    let req2 = PreviewRequest {
        text: md2,
        ..req.clone()
    };
    let after_diff_save = preview.state.run(&req2, 1.5, &[page_view(0)], token);
    assert!(after_diff_save.compile_ms > 0.0);

    // switch() increments revision and invalidates cached Compiled.
    let rev_before_switch = preview.state.revision();
    let switched = preview.open_workspace(dir.0.display().to_string(), false);
    assert!(switched.error.is_none());
    assert_eq!(preview.state.revision(), rev_before_switch + 1);
    assert!(preview.state.snapshot.lock().unwrap().is_none());
    let after_switch = preview.state.run(&req2, 1.5, &[page_view(0)], token);
    assert!(after_switch.compile_ms > 0.0);
}

#[test]
fn fingerprints_computed_lazily_only_for_in_view_pages() {
    let preview = fixture_preview();
    let req = contest_request("", "");
    let latest_gen = AtomicU64::new(1);
    let token = CancelToken {
        generation: 1,
        latest: &latest_gen,
    };

    // Request only page 0 in view out of 5 pages.
    let first = preview.state.run(&req, 0.9, &[page_view(0)], token);
    assert_eq!(first.pages.len(), 5);
    assert_eq!(first.rendered, 1);
    assert_eq!(first.pages[0].layers.len(), 1);
    for p in &first.pages[1..] {
        assert!(
            p.layers.is_empty(),
            "out-of-view page should not compute fingerprint"
        );
    }
    {
        let compiler = preview.state.compiler.lock().unwrap();
        let fps = &compiler.cached.as_ref().unwrap().fingerprints;
        assert_eq!(fps.len(), 5);
        assert!(fps[0].is_some());
        assert!(fps[1..].iter().all(Option::is_none));
    }

    // Move view to page 2 at a different scale.
    let second = preview.state.run(&req, 1.2, &[page_view(2)], token);
    assert_eq!(second.compile_ms, 0.0);
    assert_eq!(second.rendered, 1);
    assert_eq!(second.pages[0].layers.len(), 1);
    assert_eq!(second.pages[0].layers[0].key, first.pages[0].layers[0].key);
    assert_eq!(second.pages[2].layers.len(), 1);
    assert!(second.pages[1].layers.is_empty());
    assert!(second.pages[3].layers.is_empty());
    assert!(second.pages[4].layers.is_empty());
    {
        let compiler = preview.state.compiler.lock().unwrap();
        let fps = &compiler.cached.as_ref().unwrap().fingerprints;
        assert!(fps[0].is_some());
        assert!(fps[1].is_none());
        assert!(fps[2].is_some());
        assert!(fps[3].is_none());
        assert!(fps[4].is_none());
    }
}

#[test]
fn generation_counter_cancels_stale_run_and_mid_chunk_tiles() {
    let preview = fixture_preview();
    let req = contest_request("", "");

    // 1. Pre-cancelled generation aborts before compiling or rasterizing.
    let latest = AtomicU64::new(5);
    let stale = preview.state.run(
        &req,
        1.0,
        &[page_view(0)],
        CancelToken {
            generation: 4,
            latest: &latest,
        },
    );
    assert!(stale.pages.is_empty());
    assert_eq!(stale.rendered, 0);
    assert_eq!(stale.compile_ms, 0.0);

    // 2. Mid-chunk cancellation aborts remaining tile jobs immediately once generation bumps.
    let live_token = CancelToken {
        generation: 5,
        latest: &latest,
    };
    let compiled_res = preview.state.run(&req, 1.0, &[page_view(0)], live_token);
    assert_eq!(compiled_res.pages.len(), 5);

    let compiler = preview.state.compiler.lock().unwrap();
    let compiled = &compiler.cached.as_ref().unwrap().compiled;
    let size = compiled.page_size(1).unwrap();
    let regions = tiles(
        &PreviewView {
            page: 1,
            x: 0.0,
            y: 0.0,
            width: size.0,
            height: size.1,
        },
        3.0,
        size,
    );
    assert!(
        regions.len() >= 4,
        "expected multiple tiles, got {}",
        regions.len()
    );
    let jobs: Vec<Job> = regions
        .into_iter()
        .enumerate()
        .map(|(idx, r)| Job {
            page: 1,
            scale: 3.0,
            region: Some(r),
            key: format!("cancel-test:{idx}"),
            kind: LayerKind::Tile,
        })
        .collect();

    let tile_gen = AtomicU64::new(10);
    let rendered = render_chunk(
        compiled,
        &jobs,
        CancelToken {
            generation: 10,
            latest: &tile_gen,
        },
        || {
            tile_gen.fetch_add(1, Ordering::AcqRel);
        },
    );
    assert_eq!(
        rendered.len(),
        1,
        "remaining tiles must abort once generation advances"
    );
}

// ---- Phase 2 tests: lint, locate_click, hash conflicts, watcher, CRUD, freeze, prefs ---

#[test]
fn compile_returns_lint_diagnostics_and_problem_page_ranges() {
    let preview = fixture_preview();
    let req = contest_request("", "");
    let latest = AtomicU64::new(1);
    let token = CancelToken {
        generation: 1,
        latest: &latest,
    };
    let res = preview.state.run(&req, 1.0, &[page_view(0)], token);
    assert!(res.error.is_none());
    assert_eq!(res.starts.len(), 3);
    assert_eq!(res.starts[0].label, "A");
    assert_eq!(res.starts[0].id, "contest-zh-a");
    assert_eq!(res.starts[0].page, 1);
    assert_eq!(res.starts[0].first, 2);
    assert!(res.starts[0].count >= 1);
    assert_eq!(res.starts[2].label, "C");
    assert_eq!(res.starts[2].id, "contest-zh-c");
    assert_eq!(res.starts[2].page, 3);
    assert_eq!(res.starts[2].first, 4);
    assert_eq!(res.starts[2].count, 2);

    // Verify xcpc-lint warnings and errors carry stable `code` fields via publish::check.
    let (_dir, ws_preview) = init_temp_preview("lint-codes");
    assert!(
        ws_preview
            .create_problem("p1".into(), "Lint Demo".into())
            .ok
    );
    let bad_md = "只有描述没有输入输出，且引用缺失配图 ![](assets/missing.png)\n";
    let req = PreviewRequest {
        target: "problem".into(),
        id: "p1".into(),
        book: "statements".into(),
        path: "problems/p1/statement.md".into(),
        text: bad_md.into(),
    };
    let lint_res = ws_preview.state.run(&req, 1.0, &[page_view(0)], token);
    assert!(lint_res.error.is_some());
    assert!(
        lint_res
            .diagnostics
            .iter()
            .any(|d| d.severity == "error" && d.code.as_deref() == Some("missing_image")),
        "expected missing_image error, got {:?}",
        lint_res.diagnostics
    );
    assert!(
        lint_res
            .diagnostics
            .iter()
            .any(|d| d.severity == "warning" && d.code.as_deref() == Some("missing_io_section")),
        "expected missing_io_section warning, got {:?}",
        lint_res.diagnostics
    );
}

#[test]
fn locate_click_maps_preview_point_to_markdown_source() {
    let preview = fixture_preview();
    // Before compiling anything, locate_click returns None.
    assert!(preview.locate_click(0, 100.0, 200.0).is_none());

    let req = contest_request("", "");
    let latest = AtomicU64::new(1);
    let token = CancelToken {
        generation: 1,
        latest: &latest,
    };
    let res = preview.state.run(&req, 1.0, &[page_view(1)], token);
    assert!(res.error.is_none());

    // Probe body area of page 1 (0-based, Problem A) to verify reverse click-to-source.
    let mut hit = None;
    for y in (120..500).step_by(12) {
        for x in (80..450).step_by(30) {
            if let Some(loc) = preview.locate_click(1, f64::from(x), f64::from(y)) {
                hit = Some(loc);
                break;
            }
        }
        if hit.is_some() {
            break;
        }
    }
    let loc = hit.expect("expected locate_click to resolve a point inside problem A body");
    assert_eq!(loc.file, "problems/contest-zh-a/statement.md");
    assert!(loc.line >= 1);
    assert!(loc.column >= 1);

    // Out-of-bounds page and top margin return None.
    assert!(preview.locate_click(99, 100.0, 200.0).is_none());
    assert!(preview.locate_click(1, 50.0, 5.0).is_none());
}

#[test]
fn read_and_save_enforce_content_hash_conflict_detection() {
    let (dir, preview) = init_temp_preview("hash-conflict");
    assert!(preview.create_problem("p1".into(), "Hash Test".into()).ok);

    // Nonexistent file returns empty hash and no error.
    let missing = preview.read("problems/p1/editorial.md".into());
    assert!(missing.error.is_none());
    assert!(missing.text.is_empty());
    assert!(missing.hash.is_empty());
    assert!(!missing.read_only);

    // Existing file returns SHA-256 hex digest.
    let initial = preview.read("problems/p1/statement.md".into());
    assert!(initial.error.is_none());
    assert_eq!(initial.hash.len(), 64);

    // Saving with matching expected_hash succeeds and returns a new hash.
    let v1 = "第一版正文。\n\n## 输入格式\n\n$n$\n\n## 输出格式\n\n$n$\n";
    let saved = preview.save(
        "problems/p1/statement.md".into(),
        v1.into(),
        Some(initial.hash.clone()),
    );
    assert!(saved.ok);
    assert!(!saved.conflict);
    assert_eq!(saved.hash.len(), 64);
    assert_ne!(saved.hash, initial.hash);

    // Simulate an external edit changing disk contents.
    let external_text = "外部修改版本。\n";
    std::fs::write(dir.0.join("problems/p1/statement.md"), external_text).unwrap();

    // Saving with the stale hash refuses with conflict == true and preserves external file.
    let conflict = preview.save(
        "problems/p1/statement.md".into(),
        "我的未保存改动".into(),
        Some(saved.hash.clone()),
    );
    assert!(!conflict.ok);
    assert!(conflict.conflict);
    assert!(conflict.error.is_some());
    assert_eq!(
        std::fs::read_to_string(dir.0.join("problems/p1/statement.md")).unwrap(),
        external_text
    );

    // Explicitly saving without expected_hash ("keep my version") overwrites atomically.
    let forced = preview.save(
        "problems/p1/statement.md".into(),
        "保留我的版本".into(),
        None,
    );
    assert!(forced.ok);
    assert!(!forced.conflict);
    assert_eq!(
        std::fs::read_to_string(dir.0.join("problems/p1/statement.md")).unwrap(),
        "保留我的版本"
    );
}

#[test]
fn watcher_suppresses_self_writes_and_surfaces_external_edits() {
    let (dir, preview) = init_temp_preview("watcher-events");
    assert!(
        preview
            .create_problem("p1".into(), "Initial Title".into())
            .ok
    );

    // Drain any creation events from create_problem (directory creation / setup).
    let session = preview
        .state
        .watch_session()
        .expect("watcher should be active");
    while session
        .rx
        .lock()
        .unwrap()
        .recv_timeout(Duration::from_millis(180))
        .is_ok()
    {}

    // Self-writes via Preview::save and Preview::save_samples must be suppressed by SelfWriteTracker.
    let f = preview.read("problems/p1/statement.md".into());
    let saved = preview.save(
        "problems/p1/statement.md".into(),
        "自写内容\n\n## 输入格式\n\n1\n\n## 输出格式\n\n1\n".into(),
        Some(f.hash),
    );
    assert!(saved.ok);
    let sample_res = preview.save_samples(
        "p1".into(),
        vec![SamplePairJs {
            name: String::new(),
            input: "1\n".into(),
            output: "1\n".into(),
        }],
    );
    assert!(sample_res.ok);
    assert!(
        session
            .rx
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_millis(260))
            .is_err(),
        "self-writes should be suppressed by SelfWriteTracker"
    );

    // External modification must surface and incrementally update WorkspaceIndex + revision.
    let rev_before = preview.state.revision();
    std::fs::write(
        dir.0.join("problems/p1/problem.toml"),
        "title = \"External Updated Title\"\n[limits]\ntime = \"1s\"\nmemory = \"256MB\"\n",
    )
    .unwrap();

    let ev = session
        .rx
        .lock()
        .unwrap()
        .recv_timeout(Duration::from_secs(3))
        .expect("expected WatchEvent for external edit");
    let js_ev = preview.state.apply_watch_event(session.id, ev);
    assert_eq!(js_ev.kind, "changed");
    assert!(
        js_ev.paths.iter().any(|p| p == "problems/p1/problem.toml"),
        "expected problem.toml in watch paths, got {:?}",
        js_ev.paths
    );
    assert_eq!(preview.state.revision(), rev_before + 1);

    let hits = preview.search_problems(ProblemSearchQuery {
        text: Some("External Updated".into()),
        ..Default::default()
    });
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].title, "External Updated Title");
}

#[test]
fn watcher_shuts_down_cleanly_on_switch_and_drop() {
    let (_d1, preview) = init_temp_preview("watch-stop-1");
    let d2 = TempDir::new("watch-stop-2");
    xcpc_storage::init_workspace(&d2.0).unwrap();

    let s1 = preview.state.watch_session().expect("session 1");
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let rx1 = s1.rx.clone();
    std::thread::spawn(move || {
        let res = rx1.lock().unwrap().recv();
        let _ = done_tx.send(res.is_err());
    });

    // Switching workspace drops old watcher and wakes the blocked receiver immediately.
    let info = preview.open_workspace(d2.0.display().to_string(), false);
    assert!(info.error.is_none());
    assert!(
        done_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("watcher thread should wake on workspace switch"),
        "old receiver should disconnect on switch"
    );

    // Dropping Preview also wakes a blocked receiver immediately.
    let s2 = preview.state.watch_session().expect("session 2");
    let (drop_tx, drop_rx) = std::sync::mpsc::channel();
    let rx2 = s2.rx.clone();
    std::thread::spawn(move || {
        let res = rx2.lock().unwrap().recv();
        let _ = drop_tx.send(res.is_err());
    });

    drop(preview);
    assert!(
        drop_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("watcher thread should wake on Preview drop"),
        "receiver should disconnect when Preview is dropped"
    );
}

#[test]
fn workspace_init_recent_workspaces_and_mcp_snippet() {
    let data = TempDir::new("prefs-data");
    let ws1 = TempDir::new("ws-init-1");
    let ws2 = TempDir::new("ws-init-2");

    let preview = Preview::new(Some(data.0.display().to_string()));
    let info1 = preview.init_workspace(ws1.0.display().to_string(), None);
    assert!(info1.error.is_none());
    assert!(ws1.0.join("workspace.toml").is_file());
    assert!(ws1.0.join("AGENTS.md").is_file());
    assert!(ws1.0.join(".gitignore").is_file());

    // Re-initializing never overwrites an existing user AGENTS.md.
    std::fs::write(ws1.0.join("AGENTS.md"), "# Custom Guide\n").unwrap();
    let info1_again = preview.init_workspace(ws1.0.display().to_string(), None);
    assert!(info1_again.error.is_none());
    assert_eq!(
        std::fs::read_to_string(ws1.0.join("AGENTS.md")).unwrap(),
        "# Custom Guide\n"
    );

    // Opening a second workspace moves it to the front of recent_workspaces.
    let info2 = preview.init_workspace(ws2.0.display().to_string(), None);
    assert!(info2.error.is_none());
    let recent = preview.recent_workspaces();
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0], info2.root);
    assert_eq!(recent[1], info1.root);

    // Verify MCP config snippet is valid JSON with the active workspace path.
    let snippet = preview.mcp_config_snippet();
    let parsed: JsonValue = snippet.parse().expect("valid JSON snippet");
    assert_eq!(parsed["mcpServers"]["xcpc"]["command"], "xcpc");
    let args = parsed["mcpServers"]["xcpc"]["args"].as_array().unwrap();
    assert_eq!(args.len(), 3);
    assert_eq!(args[0], "-w");
    assert_eq!(args[1], info2.root);
    assert_eq!(args[2], "mcp");
}

#[test]
fn problem_library_crud_search_facets_samples_and_solutions() {
    let (dir, preview) = init_temp_preview("problem-crud");

    assert!(
        preview
            .create_problem("two-sum".into(), "Two Sum".into())
            .ok
    );
    assert!(
        preview
            .create_problem("seg-tree".into(), "Segment Tree".into())
            .ok
    );

    // Inject a comment and an unknown Phase 3 key to verify toml_edit preserves them.
    let toml_path = dir.0.join("problems/two-sum/problem.toml");
    let raw = std::fs::read_to_string(&toml_path).unwrap();
    std::fs::write(
        &toml_path,
        format!("# keep this comment\n{raw}\ncustom_phase3_key = 42\n"),
    )
    .unwrap();

    let upd = preview.update_problem_meta(
        "two-sum".into(),
        ProblemMetaPatchJs {
            title: Some("Two Sum Revised".into()),
            rating: Patch::Set(1200),
            tags: Some(vec!["dp".into(), "math".into()]),
            author: Patch::Set("Alice".into()),
            time: Patch::Set("2s".into()),
            memory: Patch::Keep,
            input: Patch::Keep,
            output: Patch::Keep,
            sources: Some(vec![ProblemSourceJs {
                platform: "codeforces".into(),
                id: "1999A".into(),
                url: Some("https://codeforces.com/problemset/problem/1999/A".into()),
            }]),
            ..Default::default()
        },
    );
    assert!(upd.ok, "update_problem_meta failed: {:?}", upd.error);
    let updated_toml = std::fs::read_to_string(&toml_path).unwrap();
    assert!(updated_toml.contains("# keep this comment"));
    assert!(updated_toml.contains("custom_phase3_key = 42"));

    assert!(
        preview
            .update_problem_meta(
                "seg-tree".into(),
                ProblemMetaPatchJs {
                    rating: Patch::Set(2200),
                    tags: Some(vec!["data-structures".into(), "math".into()]),
                    sources: Some(vec![ProblemSourceJs {
                        platform: "atcoder".into(),
                        id: "abc300_g".into(),
                        url: None,
                    }]),
                    ..Default::default()
                },
            )
            .ok
    );

    // Search with structured filters and normalized text.
    let by_tag = preview.search_problems(ProblemSearchQuery {
        tags: vec!["math".into()],
        rating_min: Some(1500),
        ..Default::default()
    });
    assert_eq!(by_tag.len(), 1);
    assert_eq!(by_tag[0].id, "seg-tree");

    let by_platform = preview.search_problems(ProblemSearchQuery {
        platform: Some("codeforces".into()),
        ..Default::default()
    });
    assert_eq!(by_platform.len(), 1);
    assert_eq!(by_platform[0].id, "two-sum");

    let facets = preview.workspace_facets();
    assert_eq!(facets.tags, vec!["data-structures", "dp", "math"]);
    assert_eq!(facets.platforms, vec!["atcoder", "codeforces"]);

    // Save samples with BOM & \r, then reorder & shrink.
    let save_s = preview.save_samples(
        "two-sum".into(),
        vec![
            SamplePairJs {
                name: String::new(),
                input: "\u{FEFF}1 2\r\n".into(),
                output: "3\r\n".into(),
            },
            SamplePairJs {
                name: String::new(),
                input: "4 5\n".into(),
                output: "9\n".into(),
            },
        ],
    );
    assert!(save_s.ok);

    // Add a solution file and test read_solution (read-only + path traversal blocked).
    std::fs::create_dir_all(dir.0.join("problems/two-sum/solutions")).unwrap();
    std::fs::write(
        dir.0.join("problems/two-sum/solutions/std.cpp"),
        "int main() { return 0; }\n",
    )
    .unwrap();

    let detail = preview.problem_detail("two-sum".into());
    assert!(detail.error.is_none());
    assert_eq!(detail.title, "Two Sum Revised");
    assert_eq!(detail.rating, Some(1200));
    assert_eq!(detail.author.as_deref(), Some("Alice"));
    assert_eq!(detail.limits.time.as_deref(), Some("2s"));
    assert_eq!(detail.samples.len(), 2);
    assert_eq!(detail.samples[0].name, "01");
    assert_eq!(detail.samples[0].input, "1 2\n");
    assert_eq!(detail.samples[0].output, "3\n");
    assert_eq!(detail.solutions, vec!["std.cpp"]);
    assert_eq!(detail.statement.hash.len(), 64);

    let sol = preview.read_solution("two-sum".into(), "std.cpp".into());
    assert!(sol.error.is_none());
    assert!(sol.read_only);
    assert!(sol.text.contains("int main()"));

    let bad_sol = preview.read_solution("two-sum".into(), "../problem.toml".into());
    assert!(bad_sol.error.is_some());

    // Deleting a problem referenced by an unfrozen contest fails with referencing_contests.
    assert!(preview.create_contest("c1".into(), "Round 1".into()).ok);
    assert!(
        preview
            .compose_contest("c1".into(), vec!["two-sum".into()])
            .ok
    );

    let detail_ref = preview.problem_detail("two-sum".into());
    assert_eq!(detail_ref.referencing_contests, vec!["c1"]);

    let del_blocked = preview.delete_problem("two-sum".into());
    assert!(!del_blocked.ok);
    assert_eq!(del_blocked.code.as_deref(), Some("problem_referenced"));
    assert_eq!(del_blocked.referencing_contests, vec!["c1"]);

    // Removing from contest unblocks deletion.
    assert!(preview.compose_contest("c1".into(), vec![]).ok);
    let del_ok = preview.delete_problem("two-sum".into());
    assert!(del_ok.ok);
    assert!(
        preview
            .search_problems(ProblemSearchQuery::default())
            .iter()
            .all(|p| p.id != "two-sum")
    );
}

#[test]
fn contest_compose_override_stats_freeze_and_unfreeze() {
    let (dir, preview) = init_temp_preview("contest-flow");

    for (id, title, rating, tags) in [
        ("pa", "Alpha", Some(1100), vec!["greedy"]),
        ("pb", "Beta", Some(1700), vec!["dp", "greedy"]),
        ("pc", "Gamma", None, vec!["graphs"]),
    ] {
        assert!(preview.create_problem(id.into(), title.into()).ok);
        assert!(
            preview
                .update_problem_meta(
                    id.into(),
                    ProblemMetaPatchJs {
                        rating: match rating {
                            Some(r) => Patch::Set(r),
                            None => Patch::Clear,
                        },
                        tags: Some(tags.into_iter().map(str::to_owned).collect()),
                        ..Default::default()
                    },
                )
                .ok
        );
        let stmt = format!(
            "题目 {title} 正文。\n\n## 输入格式\n\n一个整数 $n$。\n\n## 输出格式\n\n输出答案。\n"
        );
        assert!(
            preview
                .save(format!("problems/{id}/statement.md"), stmt, None)
                .ok
        );
    }
    assert!(
        preview
            .save(
                "problems/pa/editorial.md".into(),
                "Alpha 题解。\n".into(),
                None
            )
            .ok
    );

    // Create contest & reject duplicate creation.
    assert!(
        preview
            .create_contest("r1".into(), "Spring Round".into())
            .ok
    );
    let dup = preview.create_contest("r1".into(), "Other".into());
    assert!(!dup.ok);
    assert_eq!(dup.code.as_deref(), Some("conflict"));

    // Compose A=pa, B=pb, C=pc.
    assert!(
        preview
            .compose_contest("r1".into(), vec!["pa".into(), "pb".into(), "pc".into()])
            .ok
    );

    // Update contest metadata.
    assert!(
        preview
            .update_contest_meta(
                "r1".into(),
                ContestMetaPatchJs {
                    location: Patch::Set("上海".into()),
                    date: Patch::Set("2026 年 11 月 20 日".into()),
                    blank_pages: Some(true),
                    ..Default::default()
                },
            )
            .ok
    );

    // Set overrides on B (pb): title, time, show_metadata.
    assert!(
        preview
            .set_override(
                "r1".into(),
                "B".into(),
                OverridePatchJs {
                    title: Patch::Set("Beta Custom".into()),
                    time: Patch::Set("3s".into()),
                    show_metadata: Patch::Set(false),
                    ..Default::default()
                },
            )
            .ok
    );

    let detail = preview.contest_detail("r1".into());
    assert!(detail.error.is_none());
    assert_eq!(detail.location.as_deref(), Some("上海"));
    assert!(detail.blank_pages);
    assert!(detail.has_cover);
    assert_eq!(detail.items.len(), 3);
    assert_eq!(detail.items[1].label, "B");
    assert_eq!(detail.items[1].title, "Beta Custom");
    assert_eq!(detail.items[1].original_title, "Beta");
    assert_eq!(detail.items[1].limits.time.as_deref(), Some("3s"));
    assert_eq!(detail.items[1].original_limits.time.as_deref(), Some("1s"));
    assert!(!detail.items[1].show_metadata);
    assert!(detail.items[1].default_show_metadata);
    assert!(detail.items[1].overridden_title);
    assert!(detail.items[1].overridden_time);
    assert!(!detail.items[1].overridden_memory);
    assert!(detail.items[1].overridden_show_metadata);

    // Reorder: move pb to A -> overrides stay attached to pb!
    assert!(
        preview
            .compose_contest("r1".into(), vec!["pb".into(), "pa".into(), "pc".into()])
            .ok
    );
    let reordered = preview.contest_detail("r1".into());
    assert_eq!(reordered.items[0].label, "A");
    assert_eq!(reordered.items[0].id, "pb");
    assert_eq!(reordered.items[0].title, "Beta Custom");
    assert!(reordered.items[0].overridden_title);
    assert!(reordered.items[0].overridden_time);

    // Restoring title to library value & clearing time removes both override flags.
    assert!(
        preview
            .set_override(
                "r1".into(),
                "A".into(),
                OverridePatchJs {
                    title: Patch::Set("Beta".into()),
                    time: Patch::Clear,
                    ..Default::default()
                },
            )
            .ok
    );
    let restored = preview.contest_detail("r1".into());
    assert!(!restored.items[0].overridden_title);
    assert!(!restored.items[0].overridden_time);
    assert!(restored.items[0].overridden_show_metadata);

    // Contest stats: 8 tiers + sorted tag counts.
    let stats = preview.contest_stats("r1".into());
    assert!(stats.error.is_none());
    assert_eq!(stats.problem_count, 3);
    assert_eq!(stats.rating_distribution.len(), 8);
    let count_of = |tier: &str| {
        stats
            .rating_distribution
            .iter()
            .find(|b| b.tier == tier)
            .unwrap()
            .count
    };
    assert_eq!(count_of("newbie"), 1);
    assert_eq!(count_of("expert"), 1);
    assert_eq!(count_of("unrated"), 1);
    assert_eq!(stats.tag_counts[0].tag, "greedy");
    assert_eq!(stats.tag_counts[0].count, 2);

    // Break an image in pa -> freeze must refuse without writing snapshot/.
    assert!(
        preview
            .save(
                "problems/pa/statement.md".into(),
                "坏图 ![](assets/nope.png)\n\n## 输入格式\n\n1\n\n## 输出格式\n\n1\n".into(),
                None,
            )
            .ok
    );
    let fail_freeze = preview.state.freeze_contest("r1");
    assert!(!fail_freeze.ok);
    assert!(!fail_freeze.frozen);
    assert!(
        fail_freeze
            .diagnostics
            .iter()
            .any(|d| d.code.as_deref() == Some("missing_image"))
    );
    assert!(!dir.0.join("contests/r1/snapshot").exists());

    // Fix pa -> freeze succeeds, writes statements & editorial PDFs, enforces read-only.
    assert!(
        preview
            .save(
                "problems/pa/statement.md".into(),
                "正常题面。\n\n## 输入格式\n\n1\n\n## 输出格式\n\n1\n".into(),
                None,
            )
            .ok
    );
    let ok_freeze = preview.state.freeze_contest("r1");
    assert!(ok_freeze.ok, "freeze failed: {:?}", ok_freeze.error);
    assert!(ok_freeze.frozen);
    assert_eq!(ok_freeze.outputs.len(), 2);
    assert!(dir.0.join("contests/r1/snapshot/r1.pdf").is_file());
    assert!(
        dir.0
            .join("contests/r1/snapshot/r1-editorial.pdf")
            .is_file()
    );

    let frozen_detail = preview.contest_detail("r1".into());
    assert!(frozen_detail.frozen);
    assert!(!frozen_detail.items[0].overridden_show_metadata);

    let frozen_write = preview.compose_contest("r1".into(), vec!["pa".into()]);
    assert!(!frozen_write.ok);
    assert_eq!(frozen_write.code.as_deref(), Some("contest_frozen"));

    // Modify library problem pa -> unfreeze_diff detects statement_changed.
    assert!(
        preview
            .save(
                "problems/pa/statement.md".into(),
                "修改后的题面。\n\n## 输入格式\n\n1\n\n## 输出格式\n\n1\n".into(),
                None,
            )
            .ok
    );
    let diff = preview.unfreeze_diff("r1".into());
    assert!(diff.ok);
    assert_eq!(diff.diffs.len(), 1);
    assert_eq!(diff.diffs[0].problem_id, "pa");
    assert_eq!(diff.diffs[0].kinds, vec!["statement_changed"]);
    assert_eq!(diff.diffs[0].changed_files, vec!["statement.md"]);

    // Unfreeze deletes snapshot/ and restores mutability.
    let unfrozen = preview.unfreeze_contest("r1".into());
    assert!(unfrozen.ok);
    assert!(!dir.0.join("contests/r1/snapshot").exists());
    assert!(!preview.contest_detail("r1".into()).frozen);
    assert!(preview.compose_contest("r1".into(), vec!["pa".into()]).ok);
}

#[test]
fn fuzzy_jump_frecency_and_prefs_persistence() {
    let data = TempDir::new("prefs-frecency");
    let ws = TempDir::new("ws-frecency");

    let preview = Preview::new(Some(data.0.display().to_string()));
    assert!(
        preview
            .init_workspace(ws.0.display().to_string(), None)
            .error
            .is_none()
    );
    assert!(
        preview
            .create_problem("graph-a".into(), "Graph Walk".into())
            .ok
    );
    assert!(
        preview
            .create_problem("graph-b".into(), "Graph Walk".into())
            .ok
    );
    assert!(preview.create_contest("c1".into(), "Contest One".into()).ok);
    assert!(
        preview
            .compose_contest("c1".into(), vec!["graph-a".into(), "graph-b".into()])
            .ok
    );

    // Inside active contest `c1`, typing `"B"` ranks contest item B first.
    let label_hits = preview.fuzzy_jump("B".into(), Some("c1".into()));
    assert!(!label_hits.is_empty());
    assert_eq!(label_hits[0].kind, "contest_item");
    assert_eq!(label_hits[0].label.as_deref(), Some("B"));
    assert_eq!(label_hits[0].id, "graph-b");
    assert_eq!(label_hits[0].contest_id.as_deref(), Some("c1"));

    // Visiting `problem:graph-b` boosts it above `problem:graph-a` for identical query.
    preview.record_visit("problem:graph-b".into());
    preview.record_visit("problem:graph-b".into());
    let boosted = preview.fuzzy_jump("graph".into(), None);
    assert_eq!(boosted[0].kind, "problem");
    assert_eq!(boosted[0].id, "graph-b");

    // Save preferences (and verify split_ratio clamping + preserving frecency).
    let mut prefs = preview.load_prefs();
    prefs.split_ratio = 0.62;
    prefs.last_space = "contests".into();
    prefs.last_target = Some("contest".into());
    prefs.last_id = Some("c1".into());
    prefs.last_label = Some("B".into());
    prefs.last_book = "editorial".into();
    prefs.props_open = false;
    preview.save_prefs(prefs.clone());

    // Restarting with the same data_dir restores prefs AND persisted Frecency!
    drop(preview);
    let restarted = Preview::new(Some(data.0.display().to_string()));
    assert!(
        restarted
            .open_workspace(ws.0.display().to_string(), false)
            .error
            .is_none()
    );
    let loaded = restarted.load_prefs();
    assert!((loaded.split_ratio - 0.62).abs() < 1e-9);
    assert_eq!(loaded.last_space, "contests");
    assert_eq!(loaded.last_target.as_deref(), Some("contest"));
    assert_eq!(loaded.last_id.as_deref(), Some("c1"));
    assert_eq!(loaded.last_label.as_deref(), Some("B"));
    assert_eq!(loaded.last_book, "editorial");
    assert!(!loaded.props_open);

    let boosted_after_restart = restarted.fuzzy_jump("graph".into(), None);
    assert_eq!(boosted_after_restart[0].id, "graph-b");

    // Legacy `last-workspace` migration when `prefs.json` is absent.
    let legacy_data = TempDir::new("legacy-prefs");
    std::fs::write(
        legacy_data.0.join("last-workspace"),
        format!("{}\n", ws.0.display()),
    )
    .unwrap();
    let migrated = Preview::new(Some(legacy_data.0.display().to_string()));
    assert_eq!(migrated.recent_workspaces().len(), 1);
    assert_eq!(migrated.recent_workspaces()[0], ws.0.display().to_string());

    // Verify invalid split_ratio is safely clamped.
    migrated.save_prefs(AppPrefsJs {
        split_ratio: f64::NAN,
        ..Default::default()
    });
    assert!((migrated.load_prefs().split_ratio - 0.5).abs() < 1e-9);
}

#[test]
fn export_status_tracks_freshness_unsaved_edits_and_restart_mtime() {
    let (dir, preview) = init_temp_preview("export-freshness");
    assert!(preview.create_problem("p1".into(), "Fresh Demo".into()).ok);
    let stmt_v1 = "题面第一版。\n\n## 输入格式\n\n一个整数 $n$。\n\n## 输出格式\n\n输出答案。\n";
    let ed_v1 = "题解第一版。\n";
    assert!(
        preview
            .save("problems/p1/statement.md".into(), stmt_v1.into(), None)
            .ok
    );
    assert!(
        preview
            .save("problems/p1/editorial.md".into(), ed_v1.into(), None)
            .ok
    );

    let stmt_req = PreviewRequest {
        target: "problem".into(),
        id: "p1".into(),
        book: "statements".into(),
        path: "problems/p1/statement.md".into(),
        text: stmt_v1.into(),
    };
    let ed_req = PreviewRequest {
        target: "problem".into(),
        id: "p1".into(),
        book: "editorial".into(),
        path: "problems/p1/editorial.md".into(),
        text: ed_v1.into(),
    };

    // 1. Before any export, neither PDF exists nor is fresh.
    let s0 = preview.export_status(stmt_req.clone());
    assert_eq!(s0.statements.rel_path, "build/Fresh Demo.pdf");
    assert!(!s0.statements.exists);
    assert!(!s0.statements.fresh);
    assert_eq!(s0.editorial.rel_path, "build/Fresh Demo-题解.pdf");
    assert!(!s0.editorial.exists);
    assert!(!s0.editorial.fresh);

    // 2. Export statements and editorial -> both become fresh.
    let exp_s = preview.state.export(&stmt_req);
    assert!(
        exp_s.error.is_none(),
        "export stmt error: {:?}",
        exp_s.error
    );
    let s1 = preview.export_status(stmt_req.clone());
    assert!(s1.statements.exists && s1.statements.fresh);
    assert!(!s1.editorial.exists && !s1.editorial.fresh);

    let exp_e = preview.state.export(&ed_req);
    assert!(exp_e.error.is_none(), "export ed error: {:?}", exp_e.error);
    let s2 = preview.export_status(stmt_req.clone());
    assert!(s2.statements.exists && s2.statements.fresh);
    assert!(s2.editorial.exists && s2.editorial.fresh);

    // 3. Unsaved edit to statement.md invalidates statements only; editorial remains fresh.
    let dirty_stmt_req = PreviewRequest {
        text: format!("{stmt_v1}\n额外未保存一行。\n"),
        ..stmt_req.clone()
    };
    let s3 = preview.export_status(dirty_stmt_req);
    assert!(s3.statements.exists && !s3.statements.fresh);
    assert!(s3.editorial.exists && s3.editorial.fresh);

    // Undoing back to disk text immediately restores statements freshness.
    let s3_undo = preview.export_status(stmt_req.clone());
    assert!(s3_undo.statements.fresh);

    // 4. Saving modified statement.md invalidates statements via fingerprint (even within same mtime tick);
    //    reverting disk text back to stmt_v1 restores freshness!
    let stmt_v2 = "题面第二版。\n\n## 输入格式\n\n$n$\n\n## 输出格式\n\n$n$\n";
    assert!(
        preview
            .save("problems/p1/statement.md".into(), stmt_v2.into(), None)
            .ok
    );
    let s4 = preview.export_status(PreviewRequest {
        text: stmt_v2.into(),
        ..stmt_req.clone()
    });
    assert!(s4.statements.exists && !s4.statements.fresh);
    assert!(s4.editorial.exists && s4.editorial.fresh);

    assert!(
        preview
            .save("problems/p1/statement.md".into(), stmt_v1.into(), None)
            .ok
    );
    let s4_revert = preview.export_status(stmt_req.clone());
    assert!(s4_revert.statements.fresh);

    // 5. Restart recovery: fresh Preview without in-session export map uses mtime comparison.
    //    Re-export after a short sleep so pdf_mtime is strictly >= all source mtimes.
    std::thread::sleep(Duration::from_millis(25));
    assert!(preview.state.export(&stmt_req).error.is_none());
    assert!(preview.state.export(&ed_req).error.is_none());

    let restarted = Preview::new(None);
    assert!(
        restarted
            .open_workspace(dir.0.display().to_string(), false)
            .error
            .is_none()
    );
    let r0 = restarted.export_status(stmt_req.clone());
    assert!(r0.statements.exists && r0.statements.fresh);
    assert!(r0.editorial.exists && r0.editorial.fresh);

    // Unsaved edit after restart still marks only that book stale.
    let r_dirty = restarted.export_status(PreviewRequest {
        text: "未保存修改".into(),
        ..stmt_req.clone()
    });
    assert!(!r_dirty.statements.fresh);
    assert!(r_dirty.editorial.fresh);

    // Modifying statement.md on disk advances its mtime past pdf_mtime -> statements stale, editorial fresh.
    std::thread::sleep(Duration::from_millis(25));
    assert!(
        restarted
            .save("problems/p1/statement.md".into(), stmt_v2.into(), None)
            .ok
    );
    let r1 = restarted.export_status(PreviewRequest {
        text: stmt_v2.into(),
        ..stmt_req.clone()
    });
    assert!(r1.statements.exists && !r1.statements.fresh);
    assert!(r1.editorial.exists && r1.editorial.fresh);

    // 6. Frozen contest isolation: editing library problem does not stale a frozen contest's PDF.
    assert!(
        restarted
            .create_contest("c1".into(), "Frozen Fresh".into())
            .ok
    );
    assert!(restarted.compose_contest("c1".into(), vec!["p1".into()]).ok);
    let frozen = restarted.state.freeze_contest("c1");
    assert!(frozen.ok, "freeze error: {:?}", frozen.error);
    let contest_req = PreviewRequest {
        target: "contest".into(),
        id: "c1".into(),
        book: "statements".into(),
        path: String::new(),
        text: String::new(),
    };
    std::thread::sleep(Duration::from_millis(25));
    assert!(restarted.state.export(&contest_req).error.is_none());
    std::thread::sleep(Duration::from_millis(25));
    assert!(
        restarted
            .save("problems/p1/statement.md".into(), stmt_v1.into(), None)
            .ok
    );
    let c_in_session = restarted.export_status(contest_req.clone());
    assert!(c_in_session.statements.exists && c_in_session.statements.fresh);

    let restarted2 = Preview::new(None);
    assert!(
        restarted2
            .open_workspace(dir.0.display().to_string(), false)
            .error
            .is_none()
    );
    let c_after_restart = restarted2.export_status(contest_req);
    assert!(c_after_restart.statements.exists && c_after_restart.statements.fresh);
}

#[test]
fn last_workspace_missing_flags_deleted_or_invalid_remembered_workspace() {
    let empty_cwd = TempDir::new("empty-cwd");

    // 1. Fresh launch with no history -> last_workspace_missing is false.
    let fresh_data = TempDir::new("fresh-data");
    let fresh_state =
        crate::state::State::new_with_sources(Some(fresh_data.0.clone()), None, &empty_cwd.0);
    let fresh_info = fresh_state.info();
    assert!(fresh_info.root.is_empty());
    assert!(!fresh_info.last_workspace_missing);

    // 2. Remembered workspace in prefs.json whose directory was deleted -> true.
    let deleted_data = TempDir::new("deleted-data");
    let gone = empty_cwd.0.join("no-such-workspace");
    std::fs::write(
        deleted_data.0.join("prefs.json"),
        format!(
            r#"{{"recentWorkspaces":[{:?}]}}"#,
            gone.display().to_string()
        ),
    )
    .unwrap();
    let missing_state =
        crate::state::State::new_with_sources(Some(deleted_data.0.clone()), None, &empty_cwd.0);
    let missing_info = missing_state.info();
    assert!(missing_info.root.is_empty());
    assert!(missing_info.last_workspace_missing);

    // 3. Remembered directory exists on disk but lacks workspace.toml -> true.
    let invalid_ws = TempDir::new("no-toml-dir");
    let invalid_data = TempDir::new("invalid-data");
    std::fs::write(
        invalid_data.0.join("prefs.json"),
        format!(
            r#"{{"recentWorkspaces":[{:?}]}}"#,
            invalid_ws.0.display().to_string()
        ),
    )
    .unwrap();
    let invalid_state =
        crate::state::State::new_with_sources(Some(invalid_data.0.clone()), None, &empty_cwd.0);
    let invalid_info = invalid_state.info();
    assert!(invalid_info.root.is_empty());
    assert!(invalid_info.last_workspace_missing);

    // 4. Legacy last-workspace pointing to a deleted path -> true; opening a valid workspace resets to false.
    let legacy_data = TempDir::new("legacy-missing");
    std::fs::write(
        legacy_data.0.join("last-workspace"),
        format!("{}\n", gone.display()),
    )
    .unwrap();
    let legacy_state =
        crate::state::State::new_with_sources(Some(legacy_data.0.clone()), None, &empty_cwd.0);
    assert!(legacy_state.info().last_workspace_missing);

    let valid_ws = TempDir::new("valid-reopen");
    let ws = xcpc_storage::init_workspace(&valid_ws.0).unwrap();
    let old = legacy_state.install_workspace(ws);
    drop(old);
    assert!(!legacy_state.info().last_workspace_missing);
    assert!(!legacy_state.info().root.is_empty());
}

#[test]
fn reveal_in_folder_enforces_workspace_sandbox_and_existence() {
    let (dir, preview) = init_temp_preview("reveal-sandbox");
    assert!(preview.create_problem("p1".into(), "Reveal".into()).ok);

    // Valid relative path inside workspace.
    let rel_ok = preview
        .state
        .resolve_reveal_path("problems/p1/statement.md");
    assert!(rel_ok.is_ok(), "relative reveal failed: {:?}", rel_ok);

    // Valid absolute path inside workspace.
    let abs_target = dir.0.join("problems/p1/problem.toml");
    let abs_ok = preview
        .state
        .resolve_reveal_path(&abs_target.display().to_string());
    assert!(abs_ok.is_ok(), "absolute reveal failed: {:?}", abs_ok);

    // Missing file returns not_found.
    let missing = preview
        .state
        .resolve_reveal_path("build/does-not-exist.pdf")
        .unwrap_err();
    assert!(!missing.ok);
    assert_eq!(missing.code.as_deref(), Some("not_found"));

    // Relative path traversal outside workspace is rejected with path_escape.
    let outside = TempDir::new("reveal-outside");
    let outside_file = outside.0.join("secret.txt");
    std::fs::write(&outside_file, "secret").unwrap();

    let rel_escape = preview
        .state
        .resolve_reveal_path("../escape.txt")
        .unwrap_err();
    assert_eq!(rel_escape.code.as_deref(), Some("path_escape"));

    // Absolute path outside workspace is rejected with path_escape.
    let abs_escape = preview
        .state
        .resolve_reveal_path(&outside_file.display().to_string())
        .unwrap_err();
    assert_eq!(abs_escape.code.as_deref(), Some("path_escape"));

    // Symlink inside workspace pointing outside workspace is rejected with path_escape.
    #[cfg(unix)]
    {
        let link = dir.0.join("problems/p1/evil-link.txt");
        std::os::unix::fs::symlink(&outside_file, &link).unwrap();
        let sym_escape = preview
            .state
            .resolve_reveal_path("problems/p1/evil-link.txt")
            .unwrap_err();
        assert_eq!(sym_escape.code.as_deref(), Some("path_escape"));
    }
}

#[test]
fn prefs_preserves_window_state_and_exposes_snapshots_dir() {
    let data = TempDir::new("prefs-seams");
    assert_eq!(
        crate::prefs::snapshots_dir(&data.0),
        data.0.join("snapshots")
    );

    // Default prefs.json omits `windowState` when absent.
    let preview = Preview::new(Some(data.0.display().to_string()));
    preview.save_prefs(AppPrefsJs {
        split_ratio: 0.48,
        ..Default::default()
    });
    let raw_without = std::fs::read_to_string(data.0.join("prefs.json")).unwrap();
    assert!(
        !raw_without.contains("windowState"),
        "windowState should be omitted when None: {raw_without}"
    );

    // Inject a Phase 2.5 `windowState` payload and ensure `save_prefs` preserves it verbatim.
    let mut parsed: JsonValue = raw_without.parse().unwrap();
    parsed["windowState"] = r#"{"width":1360,"height":860,"maximized":false}"#.parse().unwrap();
    std::fs::write(data.0.join("prefs.json"), format!("{parsed:#}\n")).unwrap();

    let reloaded = Preview::new(Some(data.0.display().to_string()));
    let mut app = reloaded.load_prefs();
    app.split_ratio = 0.55;
    reloaded.save_prefs(app);

    let after: JsonValue = std::fs::read_to_string(data.0.join("prefs.json"))
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(after["windowState"]["width"], 1360);
    assert_eq!(after["windowState"]["height"], 860);
    assert_eq!(after["windowState"]["maximized"], false);
    assert!((after["splitRatio"].as_f64().unwrap() - 0.55).abs() < 1e-9);
}

#[test]
fn ui_methods_do_not_block_while_compiler_lock_is_held() {
    let (_dir, preview) = init_temp_preview("non-blocking-ui");
    assert!(preview.create_problem("p1".into(), "Smooth".into()).ok);
    assert!(preview.create_contest("c1".into(), "Contest".into()).ok);
    assert!(preview.compose_contest("c1".into(), vec!["p1".into()]).ok);

    let md = "## 题目描述\n\n给定整数 $n$。\n\n## 输入格式\n\n一行一个整数。\n\n## 输出格式\n\n一行一个整数。\n";
    assert!(
        preview
            .save("problems/p1/statement.md".into(), md.into(), None)
            .ok
    );

    let req = PreviewRequest {
        target: "contest".into(),
        id: "c1".into(),
        book: "statements".into(),
        path: "problems/p1/statement.md".into(),
        text: md.into(),
    };
    let latest = AtomicU64::new(1);
    let token = CancelToken {
        generation: 1,
        latest: &latest,
    };
    let compiled = preview.state.run(&req, 1.0, &[page_view(0)], token);
    assert!(compiled.error.is_none());

    // Simulate an in-flight background Typst compilation by holding `compiler` lock on the
    // current thread. If ANY UI-thread bridge method attempted to lock `compiler`, it would
    // immediately deadlock here.
    let _compiler_guard = preview.state.compiler.lock().unwrap();

    // Fast UI queries & mutations must all complete immediately without touching `compiler`.
    let ws_info = preview.workspace();
    assert_eq!(ws_info.problems.len(), 1);
    assert_eq!(ws_info.contests.len(), 1);
    let tpl_json: serde_json::Value = serde_json::from_str(&preview.list_templates_json()).unwrap();
    assert!(tpl_json["templates"].is_array());
    assert!(!preview.has_package("p1".into()));
    assert!(
        preview
            .read("problems/p1/statement.md".into())
            .error
            .is_none()
    );
    assert!(preview.problem_detail("p1".into()).error.is_none());
    assert!(preview.contest_detail("c1".into()).error.is_none());
    assert!(
        !preview
            .search_problems(ProblemSearchQuery::default())
            .is_empty()
    );
    let _ = preview.workspace_facets();
    let _ = preview.export_status(req.clone());
    let _ = preview.fuzzy_jump("p1".into(), Some("c1".into()));
    let prefs = preview.load_prefs();
    preview.save_prefs(prefs);

    // `locate_click` must fail-fast (`None`) rather than blocking while `compiler` is busy.
    assert!(preview.locate_click(0, 120.0, 180.0).is_none());

    // `locate_source` for the pre-indexed active file still succeeds from `snapshot`!
    let loc = preview.locate_source("problems/p1/statement.md".into(), 3);
    assert!(
        loc.is_some(),
        "pre-indexed active file should locate without blocking on compiler"
    );

    // Same-text `save()` succeeds without blocking and preserves the compiled snapshot!
    let rev_before = preview.state.revision();
    let saved = preview.save("problems/p1/statement.md".into(), md.into(), None);
    assert!(saved.ok);
    assert_eq!(preview.state.revision(), rev_before);

    // Different-text `save()` also succeeds without blocking and invalidates revision.
    let saved2 = preview.save(
        "problems/p1/statement.md".into(),
        format!("{md}\n新行。\n"),
        Some(saved.hash),
    );
    assert!(saved2.ok);
    assert_eq!(preview.state.revision(), rev_before + 1);

    // Metadata mutation also succeeds without blocking on `compiler`.
    assert!(
        preview
            .update_problem_meta(
                "p1".into(),
                ProblemMetaPatchJs {
                    title: Some("Smooth Updated".into()),
                    ..Default::default()
                },
            )
            .ok
    );
}

#[test]
fn locate_source_maps_markdown_lines_and_falls_back_across_contest_problems() {
    let preview = fixture_preview();
    // Before compiling, locate_source returns None.
    assert!(
        preview
            .locate_source("problems/contest-zh-a/statement.md".into(), 1)
            .is_none()
    );

    let a_file = preview.read("problems/contest-zh-a/statement.md".into());
    let req = contest_request("problems/contest-zh-a/statement.md", &a_file.text);
    let latest = AtomicU64::new(1);
    let token = CancelToken {
        generation: 1,
        latest: &latest,
    };
    let res = preview.state.run(&req, 1.0, &[page_view(0)], token);
    assert!(res.error.is_none());
    assert_eq!(res.starts.len(), 3);

    // Active file (Problem A) was pre-indexed during compile on page 1 (0-based).
    let a_top = preview
        .locate_source("problems/contest-zh-a/statement.md".into(), 1)
        .expect("Problem A line 1 should map");
    assert_eq!(a_top.page, res.starts[0].page);
    assert!(a_top.exact);
    assert!(a_top.y_pt > 0.0);

    let a_later = preview
        .locate_source("problems/contest-zh-a/statement.md".into(), 12)
        .expect("Problem A later line should map");
    assert!(
        a_later.page > a_top.page || (a_later.page == a_top.page && a_later.y_pt >= a_top.y_pt),
        "later line ({a_later:?}) should not be above earlier line ({a_top:?})"
    );

    // Another problem in the same contest (Problem C) is lazily indexed when compiler is idle.
    let c_loc = preview
        .locate_source("problems/contest-zh-c/statement.md".into(), 1)
        .expect("Problem C should map");
    assert!(c_loc.page >= res.starts[2].page);
    assert!(c_loc.exact);

    // Wrong book kind (editorial while viewing statements) returns None.
    assert!(
        preview
            .locate_source("problems/contest-zh-a/editorial.md".into(), 1)
            .is_none()
    );

    // When compiler is busy and a problem (Problem B) has not been lazily indexed yet,
    // locate_source falls back immediately to Problem B's start page (`exact == false`).
    let _compiler_guard = preview.state.compiler.lock().unwrap();
    let b_fallback = preview
        .locate_source("problems/contest-zh-b/statement.md".into(), 5)
        .expect("Problem B should fall back to start page when compiler is busy");
    assert_eq!(b_fallback.page, res.starts[1].page);
    assert_eq!(b_fallback.y_pt, 0.0);
    assert!(!b_fallback.exact);
}

#[test]
fn markdown_spans_utf16_columns_and_fine_grained_kinds() {
    let text = concat!(
        "## 题目描述\n",
        "给定中文 😀 序列 $2 \\le n \\le 2 \\times 10^5$ 和 $a_1, a_2, \\ldots, a_n$。\n",
        "```cpp\n",
        "#include <cstdio>\n",
        "// 注释\n",
        "int x = 42;\n",
        "```\n",
    );
    let spans = Preview::markdown_spans(text.into());
    let lines: Vec<Vec<u16>> = text
        .split('\n')
        .map(|l| l.encode_utf16().collect())
        .collect();
    let slice_utf16 = |s: &crate::types::MarkdownSpanJs| -> String {
        assert_eq!(s.start_line, s.end_line, "expected single-line span: {s:?}");
        let row = &lines[s.start_line as usize];
        String::from_utf16(&row[s.start_column as usize..s.end_column as usize]).unwrap()
    };

    // Heading: `## ` is markup, `题目描述` is heading (no overlap).
    let heading = spans.iter().find(|s| s.kind == "heading").unwrap();
    assert_eq!(heading.level, 2);
    assert_eq!(slice_utf16(heading), "题目描述");
    assert_eq!(heading.start_column, 3);
    let heading_hash = spans
        .iter()
        .find(|s| s.kind == "markup" && s.start_line == 0)
        .unwrap();
    assert_eq!(slice_utf16(heading_hash), "## ");
    assert_eq!(heading_hash.start_column, 0);
    assert_eq!(heading_hash.end_column, 3);

    // Math internal spans after Chinese + astral emoji (UTF-16 surrogate pair = 2 units).
    let math_cmds: Vec<String> = spans
        .iter()
        .filter(|s| s.kind == "mathCommand")
        .map(slice_utf16)
        .collect();
    assert_eq!(math_cmds, ["\\le", "\\le", "\\times", "\\ldots"]);

    let math_ops: Vec<String> = spans
        .iter()
        .filter(|s| s.kind == "mathOperator")
        .map(slice_utf16)
        .collect();
    assert_eq!(math_ops, ["^", "_", "_", "_"]);

    // Code block fine-grained spans.
    let keywords: Vec<String> = spans
        .iter()
        .filter(|s| s.kind == "codeKeyword")
        .map(slice_utf16)
        .collect();
    assert_eq!(keywords, ["#include", "int"]);
    let comments: Vec<String> = spans
        .iter()
        .filter(|s| s.kind == "codeComment")
        .map(slice_utf16)
        .collect();
    assert_eq!(comments, ["// 注释"]);
    let numbers: Vec<String> = spans
        .iter()
        .filter(|s| s.kind == "codeNumber")
        .map(slice_utf16)
        .collect();
    assert_eq!(numbers, ["42"]);
}

#[test]
fn ui_fonts_loads_jetbrains_mono_and_prefers_ttf_over_otf() {
    let dir = TempDir::new("ui-fonts");
    let ttf_dir = dir.0.join("truetype");
    let otf_dir = dir.0.join("opentype");
    std::fs::create_dir_all(&ttf_dir).unwrap();
    std::fs::create_dir_all(&otf_dir).unwrap();
    std::fs::write(ttf_dir.join("JetBrainsMono-Regular.ttf"), b"TTF-REG").unwrap();
    std::fs::write(otf_dir.join("JetBrainsMono-Regular.otf"), b"OTF-REG").unwrap();
    std::fs::write(otf_dir.join("JetBrainsMono-Bold.otf"), b"OTF-BOLD").unwrap();

    let loaded = crate::load_ui_fonts_from(std::slice::from_ref(&dir.0));
    assert_eq!(loaded, vec![b"TTF-REG".to_vec(), b"OTF-BOLD".to_vec()]);

    // In `nix develop`, `XCPC_FONT_DIRS` supplies JetBrains Mono directly.
    if std::env::var_os("XCPC_FONT_DIRS").is_some() {
        let env_fonts = Preview::ui_fonts();
        assert!(
            env_fonts.len() >= 4,
            "expected at least 4 JetBrains Mono faces from XCPC_FONT_DIRS, got {}",
            env_fonts.len()
        );
    }
}

#[test]
fn image_import_enforces_security_boundaries_and_unique_names() {
    let (dir, preview) = init_temp_preview("image-import");
    assert!(preview.create_problem("p1".into(), "Graph".into()).ok);
    assert!(preview.create_contest("c1".into(), "Round 1".into()).ok);
    assert!(preview.compose_contest("c1".into(), vec!["p1".into()]).ok);

    // 1. Import from external file: cleans filename and avoids collisions.
    let src_dir = TempDir::new("image-src");
    let src_png = src_dir.0.join("My Diagram 01.PNG");
    std::fs::write(&src_png, b"\x89PNG\r\n\x1a\nfake1").unwrap();

    let rel1 = preview
        .state
        .import_image_file("problem", "p1", None, &src_png)
        .expect("first import should succeed");
    assert_eq!(rel1, "assets/My-Diagram-01.png");
    assert_eq!(
        std::fs::read(dir.0.join("problems/p1/assets/My-Diagram-01.png")).unwrap(),
        b"\x89PNG\r\n\x1a\nfake1"
    );

    // Second import with same filename creates -2 without overwriting the first.
    std::fs::write(&src_png, b"\x89PNG\r\n\x1a\nfake2").unwrap();
    let rel2 = preview
        .state
        .import_image_file("problem", "p1", None, &src_png)
        .expect("second import should auto-increment");
    assert_eq!(rel2, "assets/My-Diagram-01-2.png");
    assert_eq!(
        std::fs::read(dir.0.join("problems/p1/assets/My-Diagram-01.png")).unwrap(),
        b"\x89PNG\r\n\x1a\nfake1"
    );
    assert_eq!(
        std::fs::read(dir.0.join("problems/p1/assets/My-Diagram-01-2.png")).unwrap(),
        b"\x89PNG\r\n\x1a\nfake2"
    );

    // 2. Import clipboard bytes via contest target + label.
    let pasted = preview
        .import_image_bytes(
            "contest".into(),
            "c1".into(),
            Some("A".into()),
            "png".into(),
            b"\x89PNG\r\n\x1a\npaste".to_vec(),
        )
        .expect("contest image bytes import should succeed");
    assert!(pasted.starts_with("assets/paste-"));
    assert!(pasted.ends_with(".png"));
    assert!(dir.0.join("problems/p1").join(&pasted).is_file());

    // 3. Rejects unsupported extension, empty bytes, path traversal, and missing label.
    assert!(
        preview
            .import_image_bytes(
                "problem".into(),
                "p1".into(),
                None,
                "exe".into(),
                b"bad".to_vec()
            )
            .is_err()
    );
    assert!(
        preview
            .import_image_bytes(
                "problem".into(),
                "p1".into(),
                None,
                "png".into(),
                Vec::new()
            )
            .is_err()
    );
    assert!(
        preview
            .import_image_bytes(
                "problem".into(),
                "../escape".into(),
                None,
                "png".into(),
                b"x".to_vec()
            )
            .is_err()
    );
    assert!(
        preview
            .import_image_bytes(
                "contest".into(),
                "c1".into(),
                None,
                "png".into(),
                b"x".to_vec()
            )
            .is_err()
    );

    // 4. Rejects frozen contest.
    let stmt = "## 题目描述\n\n给定 $n$。\n\n## 输入格式\n\n$n$\n\n## 输出格式\n\n$n$\n";
    assert!(
        preview
            .save("problems/p1/statement.md".into(), stmt.into(), None)
            .ok
    );
    assert!(
        preview
            .save_samples(
                "p1".into(),
                vec![SamplePairJs {
                    name: String::new(),
                    input: "1\n".into(),
                    output: "1\n".into(),
                }]
            )
            .ok
    );
    let frozen = preview.state.freeze_contest("c1");
    assert!(frozen.ok, "freeze failed: {:?}", frozen.error);
    assert!(
        preview
            .import_image_bytes(
                "contest".into(),
                "c1".into(),
                Some("A".into()),
                "png".into(),
                b"x".to_vec()
            )
            .is_err(),
        "frozen contest must reject image import"
    );

    // 5. Rejects read-only workspace.
    let ro_preview = Preview::new(None);
    assert!(
        ro_preview
            .open_workspace(dir.0.display().to_string(), true)
            .error
            .is_none()
    );
    assert!(
        ro_preview
            .import_image_bytes(
                "problem".into(),
                "p1".into(),
                None,
                "png".into(),
                b"x".to_vec()
            )
            .is_err(),
        "read-only workspace must reject image import"
    );
}

#[test]
fn asset_library_list_rename_delete_and_guards() {
    let dir = TempDir::new("asset-lib");
    let preview = Preview::new(None);
    assert!(
        preview
            .init_workspace(dir.0.display().to_string(), None)
            .error
            .is_none()
    );
    assert!(preview.create_problem("p1".into(), "资产测试".into()).ok);
    assert!(
        preview
            .save(
                "problems/p1/statement.md".into(),
                "图：![](assets/fig1.png)\n\n## 输入格式\n\n无。\n\n## 输出格式\n\n无。\n".into(),
                None,
            )
            .ok
    );
    assert!(
        preview
            .save(
                "problems/p1/editorial.md".into(),
                "详见 ![](assets/fig1.png)。\n".into(),
                None,
            )
            .ok
    );
    let assets_dir = dir.0.join("problems/p1/assets");
    std::fs::create_dir_all(&assets_dir).unwrap();
    std::fs::write(assets_dir.join("fig1.png"), b"PNG-ONE").unwrap();
    std::fs::write(assets_dir.join("unused.svg"), b"<svg/>").unwrap();

    // 1. list_problem_assets detects size, referenced flag, and exact reference locations.
    let list = preview.state.list_problem_assets("problem", "p1", None);
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].name, "fig1.png");
    assert_eq!(list[0].rel_path, "assets/fig1.png");
    assert_eq!(list[0].bytes, 7.0);
    assert!(list[0].referenced);
    assert_eq!(list[0].references.len(), 2);
    assert_eq!(list[1].name, "unused.svg");
    assert!(!list[1].referenced);
    assert!(list[1].references.is_empty());

    // 2. rename_problem_asset atomically renames and updates statement.md & editorial.md.
    let ren = preview.rename_problem_asset(
        "problem".into(),
        "p1".into(),
        None,
        "fig1.png".into(),
        "diagram.png".into(),
    );
    assert!(ren.ok, "rename failed: {:?}", ren.error);
    assert!(!assets_dir.join("fig1.png").exists());
    assert!(assets_dir.join("diagram.png").is_file());
    assert!(
        preview
            .read("problems/p1/statement.md".into())
            .text
            .contains("assets/diagram.png")
    );
    assert!(
        preview
            .read("problems/p1/editorial.md".into())
            .text
            .contains("assets/diagram.png")
    );

    // 2b. set_asset_width updates width attribute without duplicating it.
    let w1 = preview.set_asset_width(
        "problem".into(),
        "p1".into(),
        None,
        "diagram.png".into(),
        Some(60.5),
        Some("problems/p1/statement.md".into()),
    );
    assert!(w1.ok, "set_asset_width failed: {:?}", w1.error);
    assert!(
        preview
            .read("problems/p1/statement.md".into())
            .text
            .contains("assets/diagram.png){width=60.5%}")
    );
    let w2 = preview.set_asset_width(
        "problem".into(),
        "p1".into(),
        None,
        "diagram.png".into(),
        Some(75.0),
        Some("problems/p1/statement.md".into()),
    );
    assert!(w2.ok);
    let st_after = preview.read("problems/p1/statement.md".into()).text;
    assert!(st_after.contains("assets/diagram.png){width=75%}"));
    assert!(!st_after.contains("60.5"));

    // 3. Refuses overwrite, Windows illegal/reserved names, & path traversal.
    let dup = preview.rename_problem_asset(
        "problem".into(),
        "p1".into(),
        None,
        "diagram.png".into(),
        "unused.svg".into(),
    );
    assert!(!dup.ok);
    let win_reserved = preview.rename_problem_asset(
        "problem".into(),
        "p1".into(),
        None,
        "diagram.png".into(),
        "CON.png".into(),
    );
    assert!(!win_reserved.ok);
    let win_illegal = preview.rename_problem_asset(
        "problem".into(),
        "p1".into(),
        None,
        "diagram.png".into(),
        "bad:name.png".into(),
    );
    assert!(!win_illegal.ok);
    let escape = preview.delete_problem_asset(
        "problem".into(),
        "p1".into(),
        None,
        "../problem.toml".into(),
    );
    assert!(!escape.ok);

    // 4. clean_unused_assets removes unreferenced assets and keeps referenced ones.
    let cleaned = preview.clean_unused_assets("problem".into(), "p1".into(), None);
    assert!(
        cleaned.ok,
        "clean_unused_assets failed: {:?}",
        cleaned.error
    );
    assert_eq!(cleaned.removed, vec!["unused.svg"]);
    assert!(!assets_dir.join("unused.svg").exists());
    assert!(assets_dir.join("diagram.png").is_file());

    // 5. Read-only workspace rejects rename/delete/clean, but still allows list.
    let ro = Preview::new(None);
    assert!(
        ro.open_workspace(dir.0.display().to_string(), true)
            .error
            .is_none()
    );
    assert_eq!(ro.state.list_problem_assets("problem", "p1", None).len(), 1);
    let ro_del = ro.delete_problem_asset("problem".into(), "p1".into(), None, "diagram.png".into());
    assert!(!ro_del.ok);
    assert_eq!(ro_del.code.as_deref(), Some("read_only"));
}

#[test]
fn cold_start_snapshot_roundtrips_and_invalidates_on_content_and_asset_change() {
    let ws_dir = TempDir::new("snap-ws");
    let data_dir = TempDir::new("snap-data");

    let preview = Preview::new(Some(data_dir.0.display().to_string()));
    let info = preview.init_example_workspace(ws_dir.0.display().to_string(), None, None);
    assert!(
        info.error.is_none(),
        "init example failed: {:?}",
        info.error
    );
    assert_eq!(info.problems.len(), 12);
    assert_eq!(info.contests.len(), 2);

    let req = PreviewRequest {
        target: "problem".into(),
        id: "crc-check".into(),
        book: "statements".into(),
        path: String::new(),
        text: String::new(),
    };
    let latest = AtomicU64::new(1);
    let res = preview.state.run(
        &req,
        1.0,
        &[PreviewView::default()],
        CancelToken {
            generation: 1,
            latest: &latest,
        },
    );
    assert!(res.error.is_none(), "compile error: {:?}", res.error);

    let meta_json = format!(
        r#"{{"workspace":{:?},"target":"problem","id":"crc-check","book":"statements","zoom":1.25,"scrollY":42.5}}"#,
        info.root
    );
    // When compiler lock is held, save_cold_start_snapshot skips immediately without blocking.
    {
        let _guard = preview.state.compiler.lock().unwrap();
        assert!(
            !preview
                .state
                .save_cold_start_snapshot(&meta_json, &[0, 1], 2.0)
        );
    }
    assert!(
        preview
            .state
            .save_cold_start_snapshot(&meta_json, &[0, 1], 3.0)
    );

    // Snapshot stores PNG-compressed pages (no `.rgba`) and clamps render_scale to 2.0.
    let snap_dir = data_dir.0.join("snapshots");
    let page0_png = snap_dir.join("page-0.png");
    assert!(page0_png.is_file(), "expected page-0.png in snapshot dir");
    assert!(!snap_dir.join("page-0.rgba").exists());
    let (pw, _ph) = xcpc_storage::read_image_dimensions(&page0_png).unwrap();
    // A4 width is ~595pt; at 2.0x it is ~1191px (well below 3.0x = ~1786px).
    assert!(
        pw <= 1250,
        "expected render_scale clamped to 2.0, got width {pw}"
    );

    let loaded = preview
        .load_cold_start_snapshot()
        .expect("expected valid cold-start snapshot");
    assert_eq!(loaded.target, "problem");
    assert_eq!(loaded.id, "crc-check");
    assert_eq!(loaded.book, "statements");
    assert_eq!(loaded.zoom, 1.25);
    assert_eq!(loaded.scroll_y, 42.5);
    assert!(!loaded.pages.is_empty());
    assert!(!loaded.pages[0].layers.is_empty());

    // Mutate assets/ -> content hash changes -> snapshot is automatically ignored.
    let assets_dir = ws_dir.0.join("problems/crc-check/assets");
    std::fs::create_dir_all(&assets_dir).unwrap();
    std::fs::write(assets_dir.join("new.png"), b"img-bytes").unwrap();
    assert!(
        preview.load_cold_start_snapshot().is_none(),
        "stale snapshot must be ignored after assets/ change"
    );

    // Re-save with the new asset, then mutate statement.md -> invalidated again.
    assert!(
        preview
            .state
            .save_cold_start_snapshot(&meta_json, &[0], 1.5)
    );
    assert!(preview.load_cold_start_snapshot().is_some());
    assert!(
        preview
            .save(
                "problems/crc-check/statement.md".into(),
                "修改后的内容。\n\n## 输入格式\n\n无。\n\n## 输出格式\n\n无。\n".into(),
                None,
            )
            .ok
    );
    assert!(
        preview.load_cold_start_snapshot().is_none(),
        "stale snapshot must be ignored after content change"
    );
}

#[test]
fn resolve_example_target_dir_uses_subdir_when_non_empty() {
    let dir = TempDir::new("ex-pick");
    assert_eq!(crate::resolve_example_target_dir(&dir.0), dir.0);

    std::fs::write(dir.0.join("existing.txt"), b"hi").unwrap();
    assert_eq!(
        crate::resolve_example_target_dir(&dir.0),
        dir.0.join("xcpc-example")
    );

    let sub = dir.0.join("xcpc-example");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join("other.txt"), b"hi").unwrap();
    assert_eq!(
        crate::resolve_example_target_dir(&dir.0),
        dir.0.join("xcpc-example-2")
    );
}

#[test]
fn about_info_exposes_build_and_runtime_diagnostics() {
    let info = Preview::about_info();
    assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
    assert!(!info.commit.is_empty());
    assert!(!info.build_date.is_empty());
    assert_eq!(info.is_dev, cfg!(debug_assertions));
    assert_eq!(info.os, std::env::consts::OS);
    assert_eq!(info.arch, std::env::consts::ARCH);
    assert!(!info.font_source.is_empty());
}

#[test]
fn theme_overrides_load_global_only() {
    let data = TempDir::new("theme-global");
    std::fs::write(
        data.0.join("theme.toml"),
        "[colors.light]\nprimary = \"#112233\"\nbackground = \"#fafafa\"\n[editor]\nfont_family = \"JetBrains Mono\"\nfont_size = 15\n[radius]\nbase = 10\n",
    )
    .unwrap();

    let loaded = crate::theme_config::load_theme_overrides(Some(&data.0));
    let find_light = |k: &str| {
        loaded
            .light_colors
            .iter()
            .find(|c| c.key == k)
            .map(|c| c.value.as_str())
    };
    assert_eq!(find_light("primary"), Some("#112233"));
    assert_eq!(find_light("background"), Some("#fafafa"));
    assert_eq!(loaded.editor_font_family.as_deref(), Some("JetBrains Mono"));
    assert_eq!(loaded.editor_font_size, Some(15.0));
    assert_eq!(loaded.radius, Some(10.0));
    assert_eq!(
        loaded.active_path.as_deref(),
        Some(data.0.join("theme.toml").to_string_lossy().as_ref())
    );
}

#[test]
fn math_preview_renders_formula_and_reports_syntax_errors() {
    let (_dir, preview) = init_temp_preview("math-preview");
    let ok = preview
        .state
        .render_math_preview("\\frac{a}{b} + \\sum_{i=1}^n i", true, false, 2.0);
    assert!(ok.error.is_none(), "unexpected error: {:?}", ok.error);
    assert!(ok.key.contains(":math:"));
    assert!(ok.width > 0.0 && ok.height > 0.0);
    assert!(crate::cache::PIXELS.lock().unwrap().get(&ok.key).is_some());

    let bad = preview
        .state
        .render_math_preview("\\frac{a", false, false, 2.0);
    assert!(bad.error.is_some());
    assert!(bad.key.is_empty());
}

#[test]
fn template_presets_contest_local_and_image_bridge() {
    let (dir, preview) = init_temp_preview("tpl-bridge");

    // list_templates_json returns materialized templates and all bundled presets.
    let raw = preview.list_templates_json();
    let catalog: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let presets = catalog["presets"].as_array().unwrap();
    assert!(presets.iter().any(|p| p["id"] == "olymp-classic"));
    assert!(presets.iter().any(|p| p["id"] == "cnoi-classic"));
    assert!(presets.iter().any(|p| p["id"] == "minimal"));
    let templates = catalog["templates"].as_array().unwrap();
    assert!(
        templates
            .iter()
            .any(|t| t["id"] == "olymp-classic" && t["path"].is_string())
    );

    // Installing a preset materializes templates/<target>.typ.
    let res = preview.install_preset_template("minimal".into(), Some("clean".into()));
    assert!(res.ok, "install_preset_template failed: {:?}", res.error);
    assert!(dir.0.join("templates/clean.typ").is_file());

    // Create contest, detach local template, inspect contest_detail, then remove it.
    assert!(preview.create_contest("round1".into(), "Round 1".into()).ok);
    let before = preview.contest_detail("round1".into());
    assert!(!before.has_contest_template);
    assert!(before.contest_template_json.is_none());

    let detached = preview.detach_contest_template("round1".into());
    assert!(detached.ok, "detach failed: {:?}", detached.error);
    assert!(dir.0.join("contests/round1/template.typ").is_file());

    let after = preview.contest_detail("round1".into());
    assert!(after.has_contest_template);
    let local_json = after.contest_template_json.as_deref().unwrap();
    let local_info: serde_json::Value = serde_json::from_str(local_json).unwrap();
    assert!(local_info["schema"]["fields"].is_array());

    // Importing a template image copies into contests/round1/assets/.
    let src_img = dir.0.join("sample-logo.svg");
    std::fs::write(&src_img, "<svg xmlns=\"http://www.w3.org/2000/svg\"/>").unwrap();
    let rel = preview
        .state
        .import_template_image(Some("round1"), &src_img)
        .unwrap();
    assert!(rel.starts_with("contests/round1/assets/"));
    assert!(dir.0.join(&rel).is_file());

    let removed = preview.remove_contest_template("round1".into());
    assert!(removed.ok, "remove failed: {:?}", removed.error);
    assert!(!dir.0.join("contests/round1/template.typ").exists());
    assert!(!preview.contest_detail("round1".into()).has_contest_template);
}

#[test]
fn duplicate_problem_and_contest_archive_bridge() {
    let (_dir, preview) = init_temp_preview("spec3-bridge");
    assert!(
        preview
            .create_problem("two-sum".into(), "两数之和".into())
            .ok
    );

    // Duplicate problem with custom title.
    let dup = preview.duplicate_problem(
        "two-sum".into(),
        "two-sum-copy".into(),
        Some("两数之和（副本）".into()),
    );
    assert!(dup.ok, "duplicate_problem failed: {:?}", dup.error);
    let ws = preview.workspace();
    assert!(
        ws.problems
            .iter()
            .any(|p| p.id == "two-sum-copy" && p.title == "两数之和（副本）")
    );

    // Conflict on duplicate target ID.
    let conflict = preview.duplicate_problem("two-sum".into(), "two-sum-copy".into(), None);
    assert!(!conflict.ok);
    assert_eq!(conflict.code.as_deref(), Some("conflict"));

    // Create two contests and archive one.
    assert!(
        preview
            .create_contest("alpha".into(), "Alpha Contest".into())
            .ok
    );
    assert!(
        preview
            .create_contest("beta".into(), "Beta Contest".into())
            .ok
    );
    assert!(!preview.contest_detail("alpha".into()).archived);

    let arch = preview.set_contest_archived("alpha".into(), true);
    assert!(arch.ok, "archive failed: {:?}", arch.error);
    assert!(preview.contest_detail("alpha".into()).archived);
    assert!(
        preview
            .workspace()
            .contests
            .iter()
            .find(|c| c.id == "alpha")
            .unwrap()
            .archived
    );

    // Writes to an archived contest return `contest_archived`.
    let blocked = preview.compose_contest("alpha".into(), vec!["two-sum".into()]);
    assert!(!blocked.ok);
    assert_eq!(blocked.code.as_deref(), Some("contest_archived"));

    // Fuzzy jump still finds archived contest, flags it, and ranks active contests higher.
    let hits = preview.fuzzy_jump("contest".into(), None);
    let alpha_pos = hits
        .iter()
        .position(|h| h.kind == "contest" && h.id == "alpha")
        .unwrap();
    let beta_pos = hits
        .iter()
        .position(|h| h.kind == "contest" && h.id == "beta")
        .unwrap();
    assert!(hits[alpha_pos].archived);
    assert!(!hits[beta_pos].archived);
    assert!(beta_pos < alpha_pos);

    // Unarchive restores writability.
    let unarch = preview.set_contest_archived("alpha".into(), false);
    assert!(unarch.ok);
    assert!(!preview.contest_detail("alpha".into()).archived);
    assert!(
        preview
            .compose_contest("alpha".into(), vec!["two-sum".into()])
            .ok
    );
}

#[test]
fn normalize_system_locale_maps_zh_and_fallbacks_to_en() {
    use crate::normalize_system_locale;

    assert_eq!(normalize_system_locale("zh_CN.UTF-8"), "zh-CN");
    assert_eq!(normalize_system_locale("zh-TW"), "zh-CN");
    assert_eq!(normalize_system_locale("ZH_HK.UTF-8@euro"), "zh-CN");
    assert_eq!(normalize_system_locale("zh_Hans"), "zh-CN");
    assert_eq!(normalize_system_locale(":zh_CN:en_US"), "zh-CN");
    assert_eq!(normalize_system_locale("en_US.UTF-8"), "en");
    assert_eq!(normalize_system_locale("en_GB:zh_CN"), "en");
    assert_eq!(normalize_system_locale("C"), "en");
    assert_eq!(normalize_system_locale("POSIX"), "en");
    assert_eq!(normalize_system_locale("ja_JP.UTF-8"), "en");
    assert_eq!(normalize_system_locale(""), "en");
    assert_eq!(normalize_system_locale("   "), "en");
}

#[test]
fn app_prefs_language_persists_and_validates() {
    let data = TempDir::new("prefs-lang");
    let mut stored = crate::prefs::StoredPrefs::load(Some(&data.0));
    assert_eq!(stored.app.language, None);

    // Valid values persist across save/load.
    for lang in ["en", "zh-CN", "system"] {
        let mut next = stored.app.clone();
        next.language = Some(lang.to_owned());
        stored.apply_app_prefs(next);
        stored.persist(Some(&data.0));

        let reloaded = crate::prefs::StoredPrefs::load(Some(&data.0));
        assert_eq!(reloaded.app.language.as_deref(), Some(lang));
    }

    // Invalid language values are sanitized to None.
    let mut bad = stored.app.clone();
    bad.language = Some("fr-FR".to_owned());
    stored.apply_app_prefs(bad);
    assert_eq!(stored.app.language, None);
}

#[test]
fn preset_schemas_emit_bilingual_localized_text() {
    let (_dir, preview) = init_temp_preview("bilingual-schema");
    let raw = preview.list_templates_json();
    let catalog: serde_json::Value = serde_json::from_str(&raw).unwrap();

    let presets = catalog["presets"].as_array().unwrap();
    for preset in presets {
        assert!(
            preset["name"]["zh"].is_string(),
            "missing zh on preset {preset}"
        );
        assert!(
            preset["name"]["en"].is_string(),
            "missing en on preset {preset}"
        );
        assert!(
            preset["description"]["zh"].is_string(),
            "missing zh desc on preset {preset}"
        );
        assert!(
            preset["description"]["en"].is_string(),
            "missing en desc on preset {preset}"
        );
    }

    let olymp = catalog["templates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == "olymp-classic")
        .unwrap();
    assert_eq!(olymp["schema"]["name"]["zh"], "XCPC 经典");
    assert_eq!(olymp["schema"]["name"]["en"], "XCPC Classic");
    let fields = olymp["schema"]["fields"].as_array().unwrap();
    assert!(
        fields
            .iter()
            .all(|f| f["label"]["zh"].is_string() && f["label"]["en"].is_string())
    );
}

#[test]
fn toolchain_override_persists_in_prefs_and_refreshes_status() {
    let data = TempDir::new("toolchain-prefs");
    let ws_dir = TempDir::new("toolchain-ws");
    xcpc_storage::init_workspace(&ws_dir.0).unwrap();

    let state =
        crate::state::State::new_with_sources(Some(data.0.clone()), Some(&ws_dir.0), &ws_dir.0);
    let initial = state.toolchain_status();
    assert!(initial.override_path.is_none());

    // Fix A-5: invalid compiler path is rejected and never persisted to prefs.json.
    let rejected = state.set_cxx_override(Some("  /nonexistent/custom-g++  ".into()));
    assert!(rejected.error.is_some());
    assert!(rejected.override_path.is_none());
    assert!(state.load_prefs().cxx_override.is_none());

    // Valid compiler path (if present on host) persists across reload and clears cleanly.
    if let Some(valid) = initial.compilers.first() {
        let updated = state.set_cxx_override(Some(format!("  {}  ", valid.path)));
        assert!(updated.error.is_none());
        assert_eq!(updated.override_path.as_deref(), Some(valid.path.as_str()));
        assert_eq!(
            updated.active.as_ref().map(|c| c.path.as_str()),
            Some(valid.path.as_str())
        );

        let reloaded =
            crate::state::State::new_with_sources(Some(data.0.clone()), Some(&ws_dir.0), &ws_dir.0);
        assert_eq!(
            reloaded.load_prefs().cxx_override.as_deref(),
            Some(valid.path.as_str())
        );
        assert_eq!(
            reloaded.toolchain_status().override_path.as_deref(),
            Some(valid.path.as_str())
        );

        let cleared = reloaded.set_cxx_override(Some("   ".into()));
        assert!(cleared.error.is_none());
        assert!(cleared.override_path.is_none());
        assert!(reloaded.load_prefs().cxx_override.is_none());
    }
}

#[test]
fn package_inspect_init_and_sandboxed_file_resolution() {
    let ws_dir = TempDir::new("pkg-ops");
    xcpc_storage::init_workspace(&ws_dir.0).unwrap();
    let state = crate::state::State::new_with_sources(None, Some(&ws_dir.0), &ws_dir.0);

    assert!(state.create_problem("sum", "A + B").ok);

    // Before init: not a package.
    let before = state.inspect_package("sum");
    assert!(before.ok);
    assert!(!before.has_package);
    assert_eq!(before.freshness_status, "not_a_package");

    // Init package scaffolds files and refreshes inspection.
    let init = state.init_package("sum");
    assert!(init.ok, "init_package failed: {:?}", init.error);
    assert!(init.created_files.iter().any(|f| f == "validator.cpp"));
    assert!(init.created_files.iter().any(|f| f == "solutions/std.cpp"));
    assert!(init.inspection_json.is_some());

    let after = state.inspect_package("sum");
    assert!(after.ok);
    assert!(after.has_package);
    assert_eq!(after.freshness_status, "unverified");

    // Valid files and directories inside problem directory resolve (including `problem.toml` and `solutions`).
    let resolved = state.resolve_package_file("sum", "solutions/std.cpp");
    assert!(resolved.is_ok(), "expected valid path, got {resolved:?}");
    let resolved_toml = state.resolve_package_file("sum", "problem.toml");
    assert!(
        resolved_toml.is_ok(),
        "expected problem.toml to resolve, got {resolved_toml:?}"
    );
    let resolved_dir = state.resolve_package_file("sum", "solutions");
    assert!(
        resolved_dir.is_ok() && resolved_dir.unwrap().is_dir(),
        "expected solutions directory to resolve"
    );

    // Safe test-case inspection works for expanded tests.
    let preview = state.inspect_test_case("sum", "1", Some(10), Some(10));
    assert!(preview.ok, "inspect_test_case failed: {:?}", preview.error);
    assert_eq!(preview.index, 1);

    // Path traversal outside problem directory is rejected.
    let escape = state.resolve_package_file("sum", "../../workspace.toml");
    assert!(escape.is_err());
    assert_eq!(escape.unwrap_err().code.as_deref(), Some("path_escape"));

    // Structured mutations: checker, solution config, create source, hand/gen test, remove, normalize.
    assert!(state.set_package_checker("sum", "ncmp").ok);
    let created_sol = state.create_package_source("sum", "solution", "wrong_wa");
    assert!(
        created_sol.ok,
        "create solution failed: {:?}",
        created_sol.error
    );
    assert_eq!(created_sol.rel_path, "solutions/wrong_wa.cpp");
    assert!(
        state
            .set_solution_config(
                "sum",
                "wrong_wa",
                Some(false),
                Some(vec!["WA".into()]),
                None
            )
            .ok
    );

    let hand = state.add_hand_test("sum", Some("edge"), Some("hand"), "1 2\r\n");
    assert!(hand.ok, "add_hand_test failed: {:?}", hand.error);
    let hand_rel = hand.file_rel.clone().unwrap();

    // Quick-editor read & conflict-checked save round-trip + dirty input normalization + undo.
    let read_back = state.read_package_file("sum", &hand_rel);
    assert!(read_back.ok && !read_back.too_large && !read_back.content_hash.is_empty());
    let saved = state.save_package_file("sum", &hand_rel, "3 4\r\n", Some(&read_back.content_hash));
    assert!(saved.ok && !saved.conflict && saved.content_hash.is_some());

    // Fix A-3: stale hash returns conflict=true without overwriting.
    let conflict =
        state.save_package_file("sum", &hand_rel, "9 9\n", Some(&read_back.content_hash));
    assert!(!conflict.ok && conflict.conflict);
    assert_eq!(state.read_package_file("sum", &hand_rel).content, "3 4\r\n");

    let norm = state.normalize_dirty_inputs("sum");
    assert!(norm.ok && !norm.fixed.is_empty());
    let after_norm = state.read_package_file("sum", &hand_rel);
    assert_eq!(after_norm.content, "3 4\n");

    // P0-3: Undo restores original CRLF bytes.
    assert!(state.undo_normalize_dirty_inputs("sum").ok);
    assert_eq!(state.read_package_file("sum", &hand_rel).content, "3 4\r\n");

    // Fix A-5: Invalid compiler override is rejected and NOT persisted.
    let bad_cc = state.set_cxx_override(Some("/nonexistent/definitely-not-a-compiler".into()));
    assert!(bad_cc.error.is_some());
    assert!(bad_cc.override_path.is_none());

    // P1-14: Library search reports static package_status for package problems.
    let probs = state.search_problems(ProblemSearchQuery::default());
    let sum_row = probs.iter().find(|p| p.id == "sum").unwrap();
    assert_eq!(sum_row.package_status.as_deref(), Some("unverified"));

    // 256 KiB cap rejects loading oversized files into JS.
    let big_path = ws_dir.0.join("problems/sum/tests/big.in");
    std::fs::write(&big_path, vec![b'a'; 300 * 1024]).unwrap();
    let too_big = state.read_package_file("sum", "tests/big.in");
    assert!(too_big.ok && too_big.too_large && too_big.content.is_empty());

    // Sandbox rejects path traversal on save.
    let save_escape = state.save_package_file("sum", "../../evil.txt", "nope", None);
    assert!(!save_escape.ok);
    assert_eq!(save_escape.code.as_deref(), Some("path_escape"));

    // Generator batch, update, rule removal, and delete APIs (solution, generator, self-test, sample).
    let gen_added =
        state.add_generator_test("sum", "gen_random", "-n 100 {i}", 2, Some("max"), None);
    assert!(
        gen_added.ok,
        "add_generator_test failed: {:?}",
        gen_added.error
    );
    assert!(
        state
            .update_generator_test(
                "sum",
                gen_added.rule_index,
                "gen_random",
                "-n 200 {i}",
                3,
                Some("large"),
                None,
            )
            .ok
    );
    assert!(state.remove_test_rule("sum", gen_added.rule_index).ok);

    // Delete solution removes file & config cleanly.
    assert!(state.delete_solution("sum", "wrong_wa").ok);
    assert!(
        !ws_dir
            .0
            .join("problems/sum/solutions/wrong_wa.cpp")
            .exists()
    );

    // Delete self-test removes file cleanly and rejects path escape.
    let self_created = state.create_package_source("sum", "validator_self_test_valid", "99-temp");
    assert!(self_created.ok);
    assert!(state.delete_self_test("sum", &self_created.rel_path).ok);
    assert!(!state.delete_self_test("sum", "../problem.toml").ok);

    // Delete generator with referencing test rules removes both.
    let gen_ref = state.add_generator_test("sum", "gen_random", "-n 10 {i}", 2, Some("tmp"), None);
    assert!(gen_ref.ok);
    assert!(state.delete_generator("sum", "gen_random.cpp", true).ok);
    assert!(
        !ws_dir
            .0
            .join("problems/sum/generators/gen_random.cpp")
            .exists()
    );

    // Delete sample renumbers remaining samples contiguously.
    assert!(
        state
            .save_samples(
                "sum",
                vec![
                    SamplePairJs {
                        name: "01".into(),
                        input: "1 2\n".into(),
                        output: "3\n".into(),
                    },
                    SamplePairJs {
                        name: "02".into(),
                        input: "4 5\n".into(),
                        output: "9\n".into(),
                    },
                ],
            )
            .ok
    );
    assert!(state.delete_sample("sum", 1).ok);
    let detail_after_del = state.problem_detail("sum");
    assert_eq!(detail_after_del.samples.len(), 1);
    assert_eq!(detail_after_del.samples[0].name, "01");
    assert_eq!(detail_after_del.samples[0].input, "4 5\n");
}

// ---- Phase 3B+: Library groups, batch mutations & async batch task streaming -----------

#[test]
fn prefs_round_trip_library_sidebar_and_group_by() {
    let data = TempDir::new("prefs-library-ui");
    let preview = Preview::new(Some(data.0.display().to_string()));

    // Defaults: sidebar open, no dynamic grouping override.
    let initial = preview.load_prefs();
    assert!(initial.library_sidebar_open);
    assert_eq!(initial.library_group_by, None);

    // Save valid preferences and verify JSON keys & reload round-trip.
    for valid in [
        "none",
        "group",
        "rating",
        "platform",
        "contest",
        "package_status",
    ] {
        let mut p = preview.load_prefs();
        p.library_sidebar_open = false;
        p.library_group_by = Some(valid.to_owned());
        preview.save_prefs(p);

        let reloaded = Preview::new(Some(data.0.display().to_string())).load_prefs();
        assert!(!reloaded.library_sidebar_open);
        assert_eq!(reloaded.library_group_by.as_deref(), Some(valid));
    }

    let raw_json = std::fs::read_to_string(data.0.join("prefs.json")).unwrap();
    let parsed: JsonValue = raw_json.parse().expect("valid prefs.json");
    assert_eq!(parsed["librarySidebarOpen"], false);
    assert_eq!(parsed["libraryGroupBy"], "package_status");

    // Invalid libraryGroupBy values are rejected/cleared to None.
    let mut bad = preview.load_prefs();
    bad.library_sidebar_open = true;
    bad.library_group_by = Some("not_a_valid_mode".to_owned());
    preview.save_prefs(bad);
    let after_bad = Preview::new(Some(data.0.display().to_string())).load_prefs();
    assert!(after_bad.library_sidebar_open);
    assert_eq!(after_bad.library_group_by, None);
}

#[test]
fn group_crud_facets_search_and_detail() {
    let (_dir, preview) = init_temp_preview("group-crud");
    assert!(preview.create_problem("p1".into(), "Alpha".into()).ok);
    assert!(preview.create_problem("p2".into(), "Beta".into()).ok);
    assert!(preview.create_problem("p3".into(), "Gamma".into()).ok);

    // Create groups & enforce validation / uniqueness.
    assert!(preview.create_group("省赛候选".into()).ok);
    assert!(preview.create_group("校赛备选".into()).ok);
    let dup = preview.create_group("省赛候选".into());
    assert!(!dup.ok);
    assert_eq!(dup.code.as_deref(), Some("group_already_exists"));
    let invalid = preview.create_group("   ".into());
    assert!(!invalid.ok);
    assert_eq!(invalid.code.as_deref(), Some("invalid_group_name"));

    let facets = preview.workspace_facets();
    assert_eq!(facets.total_count, 3);
    assert_eq!(facets.ungrouped_count, 3);
    assert_eq!(facets.groups.len(), 2);
    assert_eq!(facets.groups[0].name, "省赛候选");
    assert_eq!(facets.groups[0].count, 0);
    assert!(facets.groups[0].declared);
    assert_eq!(facets.groups[1].name, "校赛备选");

    // Batch assign group and verify facets, search, and detail immediately reflect it.
    let assign = preview.batch_set_group(vec!["p1".into(), "p2".into()], Some("省赛候选".into()));
    assert!(assign.ok);
    assert_eq!(assign.succeeded, 2);
    assert_eq!(assign.skipped, 0);

    // Re-assigning the same group skips unchanged problems.
    let assign_again = preview.batch_set_group(vec!["p1".into()], Some("省赛候选".into()));
    assert!(assign_again.ok);
    assert_eq!(assign_again.succeeded, 0);
    assert_eq!(assign_again.skipped, 1);

    let detail = preview.problem_detail("p1".into());
    assert_eq!(detail.group.as_deref(), Some("省赛候选"));

    let in_group = preview.search_problems(ProblemSearchQuery {
        group: Some("省赛候选".into()),
        ..Default::default()
    });
    assert_eq!(in_group.len(), 2);
    assert!(
        in_group
            .iter()
            .all(|p| p.group.as_deref() == Some("省赛候选"))
    );

    let ungrouped = preview.search_problems(ProblemSearchQuery {
        ungrouped_only: true,
        ..Default::default()
    });
    assert_eq!(ungrouped.len(), 1);
    assert_eq!(ungrouped[0].id, "p3");
    assert_eq!(ungrouped[0].group, None);

    // Reorder groups.
    assert!(
        preview
            .reorder_groups(vec!["校赛备选".into(), "省赛候选".into()])
            .ok
    );
    let facets_reordered = preview.workspace_facets();
    assert_eq!(facets_reordered.groups[0].name, "校赛备选");
    assert_eq!(facets_reordered.groups[1].name, "省赛候选");
    assert_eq!(facets_reordered.groups[1].count, 2);
    assert_eq!(facets_reordered.ungrouped_count, 1);

    // Rename group updates workspace.toml + member problems + index.
    assert!(preview.rename_group("省赛候选".into(), "区域赛".into()).ok);
    let detail_renamed = preview.problem_detail("p1".into());
    assert_eq!(detail_renamed.group.as_deref(), Some("区域赛"));
    let facets_renamed = preview.workspace_facets();
    assert_eq!(facets_renamed.groups[1].name, "区域赛");
    assert_eq!(facets_renamed.groups[1].count, 2);

    // Delete group removes the group and returns members to ungrouped without deleting problems.
    assert!(preview.delete_group("区域赛".into()).ok);
    let facets_deleted = preview.workspace_facets();
    assert_eq!(facets_deleted.total_count, 3);
    assert_eq!(facets_deleted.ungrouped_count, 3);
    assert_eq!(facets_deleted.groups.len(), 1);
    assert_eq!(preview.problem_detail("p1".into()).group, None);
}

#[test]
fn batch_mutations_and_delete_with_contest_guards() {
    let (_dir, preview) = init_temp_preview("batch-ops");
    assert!(preview.create_problem("p1".into(), "One".into()).ok);
    assert!(preview.create_problem("p2".into(), "Two".into()).ok);
    assert!(preview.create_problem("p3".into(), "Three".into()).ok);
    assert!(preview.create_contest("c1".into(), "Contest 1".into()).ok);

    // Batch tags + rating.
    let tag_res = preview.batch_edit_tags(
        vec!["p1".into(), "p2".into()],
        vec!["dp".into(), "math".into()],
        vec![],
    );
    assert!(tag_res.ok);
    assert_eq!(tag_res.succeeded, 2);

    let rating_res = preview.batch_set_rating(vec!["p1".into(), "p2".into()], Some(1800));
    assert!(rating_res.ok);
    assert_eq!(rating_res.succeeded, 2);

    // Batch add to contest populates contest_ids on ProblemSummaryJs with zero N+1.
    let add_res = preview.batch_add_to_contest("c1".into(), vec!["p1".into(), "p2".into()]);
    assert!(add_res.ok);
    assert_eq!(add_res.succeeded, 2);

    let add_dup = preview.batch_add_to_contest("c1".into(), vec!["p1".into()]);
    assert!(add_dup.ok);
    assert_eq!(add_dup.skipped, 1);
    assert_eq!(add_dup.items[0].code.as_deref(), Some("already_in_contest"));

    let hits = preview.search_problems(ProblemSearchQuery::default());
    let p1 = hits.iter().find(|p| p.id == "p1").unwrap();
    let p3 = hits.iter().find(|p| p.id == "p3").unwrap();
    assert_eq!(p1.contest_ids, vec!["c1".to_owned()]);
    assert_eq!(p1.rating, Some(1800));
    assert!(p1.tags.contains(&"dp".to_owned()));
    assert!(p3.contest_ids.is_empty());

    // Batch delete skips problems referenced by unfrozen contest and deletes unreferenced ones.
    let del_res = preview.batch_delete_problems(vec!["p1".into(), "p3".into()]);
    assert!(del_res.ok);
    assert_eq!(del_res.deleted, 1);
    assert_eq!(del_res.skipped, 1);
    assert_eq!(del_res.failed, 0);

    let p1_del = del_res.items.iter().find(|i| i.id == "p1").unwrap();
    assert_eq!(p1_del.status, "skipped");
    assert_eq!(p1_del.code.as_deref(), Some("problem_referenced"));
    assert_eq!(p1_del.referencing_contests, vec!["c1".to_owned()]);

    let p3_del = del_res.items.iter().find(|i| i.id == "p3").unwrap();
    assert_eq!(p3_del.status, "ok");

    // Index is immediately fresh after batch delete.
    let after_del = preview.search_problems(ProblemSearchQuery::default());
    assert_eq!(after_del.len(), 2);
    assert!(after_del.iter().all(|p| p.id != "p3"));
}

#[test]
fn batch_task_streaming_completion_and_cancellation() {
    let (dir, preview) = init_temp_preview("batch-stream");
    assert!(preview.create_problem("p1".into(), "First".into()).ok);
    assert!(preview.create_problem("p2".into(), "Second".into()).ok);

    // 1. Batch OJ export on pure-publishing problems skips them cleanly and emits all_finished.
    let started = preview.start_batch_export(BatchExportRequestJs {
        format: "domjudge".into(),
        problem_ids: vec!["p1".into(), "p2".into()],
        ..Default::default()
    });
    assert!(started.ok, "start_batch_export failed: {:?}", started.error);
    assert!(started.task_id > 0);

    let (rx_opt, _) = preview.state.batch_event_receiver(started.task_id);
    let rx = rx_opt.expect("event receiver should exist for active task");
    let mut kinds = Vec::new();
    let mut final_snap = None;
    loop {
        match rx.lock().unwrap().recv_timeout(Duration::from_secs(10)) {
            Ok(ev) => {
                kinds.push(ev.kind.clone());
                if ev.kind == "all_finished" {
                    final_snap = Some(ev.snapshot);
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                panic!("timed out waiting for batch export stream to close; kinds={kinds:?}");
            }
        }
    }
    assert!(kinds.contains(&"problem_started".to_owned()));
    assert!(kinds.contains(&"problem_finished".to_owned()));
    assert_eq!(kinds.last().map(String::as_str), Some("all_finished"));
    let snap = final_snap.expect("expected all_finished snapshot");
    assert!(!snap.running);
    assert_eq!(snap.skipped, 2);
    assert_eq!(snap.succeeded, 0);

    // 2. Merged PDF batch export compiles and writes a real PDF without creating contests/.
    let pdf_start = preview.start_batch_export(BatchExportRequestJs {
        format: "merged_pdf".into(),
        problem_ids: vec!["p1".into(), "p2".into()],
        title: Some("练习题集".into()),
        language: Some("chinese".into()),
        book: Some("statements".into()),
        output_path: None,
    });
    assert!(
        pdf_start.ok,
        "start_batch_export(merged_pdf) failed: {:?}",
        pdf_start.error
    );
    let (pdf_rx_opt, _) = preview.state.batch_event_receiver(pdf_start.task_id);
    let pdf_rx = pdf_rx_opt.expect("pdf receiver");
    let mut pdf_final = None;
    loop {
        match pdf_rx.lock().unwrap().recv_timeout(Duration::from_secs(20)) {
            Ok(ev) => {
                if ev.kind == "all_finished" || ev.kind == "error" {
                    pdf_final = Some(ev);
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                panic!("timed out waiting for merged PDF batch export");
            }
        }
    }
    let pdf_ev = pdf_final.expect("expected terminal PDF event");
    assert_eq!(
        pdf_ev.kind, "all_finished",
        "merged PDF failed: {:?}",
        pdf_ev.error
    );
    let out_path = pdf_ev
        .snapshot
        .output_path
        .as_deref()
        .expect("output_path should be set");
    assert!(std::path::Path::new(out_path).is_file());
    assert!(
        !dir.0.join("contests").read_dir().unwrap().next().is_some(),
        "merged_pdf must not create any contest directory"
    );

    // 3. Stopping batch task on workspace switch / Preview drop wakes a blocked receiver immediately with "closed".
    let (manual_tx, manual_rx) = std::sync::mpsc::channel();
    let rx_arc = std::sync::Arc::new(std::sync::Mutex::new(manual_rx));
    let cancel_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let mut slot = preview.state.batch_task.lock().unwrap();
        slot.task_id = 999;
        slot.running = true;
        slot.cancel_flag = Some(cancel_flag.clone());
        slot.close_tx = Some(manual_tx);
        slot.event_rx = Some(rx_arc.clone());
    }

    assert!(preview.cancel_batch_task().ok);
    assert!(cancel_flag.load(Ordering::Relaxed));
    assert!(preview.batch_task_status().cancelled);

    let (woke_tx, woke_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let ev = rx_arc.lock().unwrap().recv().ok();
        let _ = woke_tx.send(ev.map(|e| e.kind));
    });

    drop(preview);
    let woke_kind = woke_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("blocked batch receiver should wake immediately on Preview drop");
    assert_eq!(woke_kind.as_deref(), Some("closed"));
}

#[test]
fn phase3c_interactive_runs_subtasks_and_scoring_bridge() {
    let (_dir, preview) = init_temp_preview("phase3c-bridge");
    assert!(preview.create_problem("guess".into(), "Guess".into()).ok);

    // Init with interactive + multi-run + subtasks options.
    let init = preview.init_package_with_options("guess".into(), true, Some(2), true);
    assert!(
        init.ok,
        "init_package_with_options failed: {:?}",
        init.error
    );
    assert!(init.created_files.iter().any(|f| f == "interactor.cpp"));

    let insp_res = preview.inspect_package("guess".into(), None);
    assert!(insp_res.ok && insp_res.has_package);
    let insp: JsonValue = insp_res
        .inspection_json
        .parse()
        .expect("valid inspection json");
    assert_eq!(insp["interactive"], true);
    assert_eq!(insp["runs"], 2);
    assert_eq!(insp["total_score"], 100);
    assert_eq!(insp["files"]["interactor"], "interactor.cpp");
    assert_eq!(insp["subtasks"].as_array().unwrap().len(), 2);

    // Update runs and delete/recreate interactor.
    assert!(preview.set_package_runs("guess".into(), Some(3)).ok);
    let after_runs: JsonValue = preview
        .inspect_package("guess".into(), None)
        .inspection_json
        .parse()
        .unwrap();
    assert_eq!(after_runs["runs"], 3);

    assert!(preview.delete_interactor("guess".into()).ok);
    let after_del_int: JsonValue = preview
        .inspect_package("guess".into(), None)
        .inspection_json
        .parse()
        .unwrap();
    assert_eq!(after_del_int["interactive"], false);

    let recreated =
        preview.create_package_source("guess".into(), "interactor".into(), "interactor".into());
    assert!(
        recreated.ok,
        "recreate interactor failed: {:?}",
        recreated.error
    );
    assert_eq!(recreated.rel_path, "interactor.cpp");

    // Reset runs to single-run default.
    assert!(preview.set_package_runs("guess".into(), None).ok);
    let after_reset_runs: JsonValue = preview
        .inspect_package("guess".into(), None)
        .inspection_json
        .parse()
        .unwrap();
    assert_eq!(after_reset_runs["runs"], 1);

    // Upsert subtask, assign test subtask, set solution scoring, and delete subtask with reference cleanup.
    assert!(
        preview
            .upsert_subtask(
                "guess".into(),
                "3".into(),
                20,
                vec!["1".into()],
                "sum".into(),
            )
            .ok
    );
    assert!(
        preview
            .set_test_subtask("guess".into(), 0, Some("3".into()))
            .ok
    );

    let brute = preview.create_package_source("guess".into(), "solution".into(), "brute".into());
    assert!(brute.ok);
    assert!(
        preview
            .set_solution_scoring(
                "guess".into(),
                "brute".into(),
                Some("[20, 50]".into()),
                Some(r#"{"1":"AC","3":["WA","TLE"]}"#.into()),
            )
            .ok
    );

    let with_sub3: JsonValue = preview
        .inspect_package("guess".into(), None)
        .inspection_json
        .parse()
        .unwrap();
    assert_eq!(with_sub3["total_score"], 120);
    let brute_info = with_sub3["solutions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "brute")
        .expect("brute solution in inspection");
    assert_eq!(brute_info["score"], serde_json::json!([20, 50]));
    assert_eq!(brute_info["subtasks"]["1"], "AC");

    assert!(preview.delete_subtask("guess".into(), "3".into(), true).ok);
    let after_del_sub3: JsonValue = preview
        .inspect_package("guess".into(), None)
        .inspection_json
        .parse()
        .unwrap();
    assert_eq!(after_del_sub3["total_score"], 100);
    assert_eq!(after_del_sub3["subtasks"].as_array().unwrap().len(), 2);
}

// ---- Phase 4A: Training status, URL resolution, Plugins & Sync bridge tests ------------

#[test]
fn phase4a_training_status_derivation_and_manual_override() {
    let ws_dir = TempDir::new("p4a-train-ws");
    let data_dir = TempDir::new("p4a-train-data");
    let preview = Preview::new(Some(data_dir.0.display().to_string()));
    let info = preview.init_workspace(ws_dir.0.display().to_string(), None);
    assert!(info.error.is_none());
    assert!(info.training_enabled);

    // Create two problems: one linked to codeforces 1999A, one pure local.
    let created = preview.create_problem_from_url(
        "cf-1999a".into(),
        "A+B Again?".into(),
        "codeforces".into(),
        "1999A".into(),
        "https://codeforces.com/problemset/problem/1999/A".into(),
        vec!["math".into()],
        Some(800),
    );
    assert!(created.ok && !created.existed);
    assert!(
        preview
            .create_problem("local-p".into(), "Local Problem".into())
            .ok
    );

    // Seed oj_sync.db with an AC submission for codeforces 1999A.
    let mut db = xcpc_sync::SyncDb::open(&data_dir.0).unwrap();
    db.upsert_submissions(
        "codeforces",
        &[xcpc_plugin::OjSubmission {
            submission_id: "sub-1".into(),
            handle: "tourist".into(),
            contest_id: Some("1999".into()),
            problem_index: Some("A".into()),
            problem_key: "1999A".into(),
            problem_title: "A+B Again?".into(),
            problem_url: Some("https://codeforces.com/problemset/problem/1999/A".into()),
            raw_rating: Some(800.0),
            normalized_rating: Some(800),
            tags: vec!["math".into()],
            verdict: "AC".into(),
            language: Some("C++20".into()),
            submitted_at: 1_700_000_000,
        }],
        "2026-11-24T00:00:00Z",
    )
    .unwrap();

    // Synced AC shows as "ac", local shows as "none".
    let list = preview.search_problems(ProblemSearchQuery::default());
    let cf = list.iter().find(|p| p.id == "cf-1999a").unwrap();
    let local = list.iter().find(|p| p.id == "local-p").unwrap();
    assert_eq!(cf.training_status, "ac");
    assert_eq!(local.training_status, "none");

    // Manual override: local -> "todo", synced AC -> "upsolved".
    assert!(
        preview
            .set_training_status("local-p".into(), "todo".into())
            .ok
    );
    assert!(
        preview
            .set_training_status("cf-1999a".into(), "upsolved".into())
            .ok
    );
    let list2 = preview.search_problems(ProblemSearchQuery::default());
    assert_eq!(
        list2
            .iter()
            .find(|p| p.id == "local-p")
            .unwrap()
            .training_status,
        "todo"
    );
    assert_eq!(
        list2
            .iter()
            .find(|p| p.id == "cf-1999a")
            .unwrap()
            .training_status,
        "upsolved"
    );

    // Clear manual override: local returns to "none", cf returns to synced "ac".
    assert!(
        preview
            .set_training_status("local-p".into(), "clear".into())
            .ok
    );
    assert!(
        preview
            .set_training_status("cf-1999a".into(), "clear".into())
            .ok
    );
    let list3 = preview.search_problems(ProblemSearchQuery::default());
    assert_eq!(
        list3
            .iter()
            .find(|p| p.id == "local-p")
            .unwrap()
            .training_status,
        "none"
    );
    assert_eq!(
        list3
            .iter()
            .find(|p| p.id == "cf-1999a")
            .unwrap()
            .training_status,
        "ac"
    );

    // Invalid status is rejected cleanly.
    let bad = preview.set_training_status("local-p".into(), "bogus".into());
    assert!(!bad.ok);
}

#[test]
fn phase4a_training_disabled_hides_status() {
    let ws_dir = TempDir::new("p4a-disabled-ws");
    let data_dir = TempDir::new("p4a-disabled-data");
    let preview = Preview::new(Some(data_dir.0.display().to_string()));
    let info = preview.init_workspace(ws_dir.0.display().to_string(), None);
    assert!(info.training_enabled);

    assert!(preview.create_problem("p1".into(), "Problem 1".into()).ok);
    assert!(preview.set_training_status("p1".into(), "ac".into()).ok);

    // Set `training = false` in workspace.toml and reopen.
    let ws_toml = ws_dir.0.join("workspace.toml");
    let mut raw = std::fs::read_to_string(&ws_toml).unwrap();
    raw.push_str("\ntraining = false\n");
    std::fs::write(&ws_toml, raw).unwrap();

    let reopened = preview.open_workspace(ws_dir.0.display().to_string(), false);
    assert!(reopened.error.is_none());
    assert!(!reopened.training_enabled);

    let list = preview.search_problems(ProblemSearchQuery::default());
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].training_status, "none");
}

#[test]
fn phase4a_resolve_problem_url_and_create_dedup() {
    let ws_dir = TempDir::new("p4a-url-ws");
    let data_dir = TempDir::new("p4a-url-data");
    let preview = Preview::new(Some(data_dir.0.display().to_string()));
    let info = preview.init_workspace(ws_dir.0.display().to_string(), None);
    assert!(info.error.is_none());

    // Pre-populate oj_problem_meta cache so resolve_problem_url stays 100% offline.
    let mut db = xcpc_sync::SyncDb::open(&data_dir.0).unwrap();
    db.upsert_problem_meta(&xcpc_sync::CachedProblemMeta {
        platform: "codeforces".into(),
        problem_key: "1999A".into(),
        title: Some("A+B Again?".into()),
        url: Some("https://codeforces.com/problemset/problem/1999/A".into()),
        raw_rating: Some(800),
        normalized_rating: Some(800),
        tags: vec!["implementation".into(), "math".into()],
        updated_at: "2026-11-24T00:00:00Z".into(),
    })
    .unwrap();

    let resolved = preview
        .state
        .resolve_problem_url("https://codeforces.com/problemset/problem/1999/A");
    assert!(resolved.ok, "resolve error: {:?}", resolved.error);
    assert_eq!(resolved.platform, "codeforces");
    assert_eq!(resolved.problem_key, "1999A");
    assert_eq!(resolved.suggested_slug, "cf-1999a");
    assert_eq!(resolved.title.as_deref(), Some("A+B Again?"));
    assert_eq!(resolved.rating, Some(800));
    assert_eq!(resolved.tags, vec!["implementation", "math"]);
    assert!(resolved.existing_problem_id.is_none());

    // Create from URL.
    let created = preview.create_problem_from_url(
        resolved.suggested_slug.clone(),
        resolved.title.clone().unwrap(),
        resolved.platform.clone(),
        resolved.problem_key.clone(),
        resolved.url.clone(),
        resolved.tags.clone(),
        resolved.rating,
    );
    assert!(created.ok);
    assert!(!created.existed);
    assert_eq!(created.problem_id, "cf-1999a");

    // Re-resolving now detects existing_problem_id, and creating again deduplicates.
    let resolved2 = preview
        .state
        .resolve_problem_url("https://codeforces.com/contest/1999/problem/A");
    assert!(resolved2.ok);
    assert_eq!(resolved2.existing_problem_id.as_deref(), Some("cf-1999a"));

    let dup = preview.create_problem_from_url(
        "another-slug".into(),
        "Dup".into(),
        "codeforces".into(),
        "1999A".into(),
        resolved2.url,
        vec![],
        None,
    );
    assert!(dup.ok);
    assert!(dup.existed);
    assert_eq!(dup.problem_id, "cf-1999a");
}

#[test]
fn phase4a_plugins_overview_handles_repos_and_export() {
    let data_dir = TempDir::new("p4a-plugins-data");
    let preview = Preview::new(Some(data_dir.0.display().to_string()));

    let overview = preview.get_plugins_overview();
    assert!(overview.error.is_none());
    assert!(overview.auto_sync_on_startup);
    assert!(
        overview
            .plugins
            .iter()
            .any(|p| p.id == "codeforces" && p.origin == "builtin")
    );
    assert!(
        overview
            .plugins
            .iter()
            .any(|p| p.id == "atcoder" && p.origin == "builtin")
    );

    // Configure handle, auto-sync, enable/disable, repo, and export built-in copy.
    assert!(
        preview
            .set_sync_handle("codeforces".into(), "tourist".into())
            .ok
    );
    assert!(preview.set_auto_sync_on_startup(false).ok);
    assert!(preview.set_plugin_enabled("atcoder".into(), false).ok);
    assert!(
        preview
            .add_plugin_repo("https://example.com/plugins/index.json".into())
            .ok
    );
    let exported = preview.export_builtin_plugin("codeforces".into());
    assert!(exported.ok, "export failed: {:?}", exported.error);
    assert!(exported.path.is_some());

    let updated = preview.reload_plugins();
    assert!(!updated.auto_sync_on_startup);
    // Fresh data_dir subscribes to the official plugin repo by default, plus the custom repo added above.
    assert_eq!(updated.repos.len(), 2);
    assert!(
        updated
            .repos
            .iter()
            .any(|r| r.url == "https://example.com/plugins/index.json")
    );

    let cf = updated
        .plugins
        .iter()
        .find(|p| p.id == "codeforces")
        .unwrap();
    assert_eq!(cf.origin, "local_override");
    assert_eq!(cf.handle.as_deref(), Some("tourist"));

    let ac = updated.plugins.iter().find(|p| p.id == "atcoder").unwrap();
    assert!(!ac.enabled);

    assert!(
        preview
            .remove_plugin_repo("https://example.com/plugins/index.json".into())
            .ok
    );
    let after_remove = preview.get_plugins_overview();
    assert_eq!(after_remove.repos.len(), 1);
    assert!(
        !after_remove
            .repos
            .iter()
            .any(|r| r.url == "https://example.com/plugins/index.json")
    );
}

// ---- Phase 4B: Training Dashboard, Contests, Submissions & Ingest bridge tests ---------

#[test]
fn phase4b_training_dashboard_contests_submissions_and_ingest() {
    let ws_dir = TempDir::new("p4b-train-ws");
    let data_dir = TempDir::new("p4b-train-data");
    let preview = Preview::new(Some(data_dir.0.display().to_string()));
    let info = preview.init_workspace(ws_dir.0.display().to_string(), None);
    assert!(info.error.is_none());
    assert!(info.training_enabled);

    // Seed oj_sync.db with account, rating history, submissions, and contests.
    let mut db = xcpc_sync::SyncDb::open(&data_dir.0).unwrap();
    db.upsert_account(
        "codeforces",
        &xcpc_plugin::OjAccountSync {
            handle: "alice".into(),
            rating: Some(1950),
            max_rating: Some(2010),
            rank: Some("candidate master".into()),
            rating_history: vec![xcpc_plugin::OjRatingChange {
                contest_id: "2000".into(),
                contest_name: "Codeforces Round 1000".into(),
                rating_before: 1890,
                rating_after: 1950,
                rank: Some(320),
                at: 1_760_000_000,
            }],
        },
        "2026-11-24T12:00:00Z",
    )
    .unwrap();
    db.upsert_submissions(
        "codeforces",
        &[
            xcpc_plugin::OjSubmission {
                submission_id: "s1".into(),
                handle: "alice".into(),
                contest_id: Some("2000".into()),
                problem_index: Some("C".into()),
                problem_key: "2000C".into(),
                problem_title: "Graph Paths".into(),
                problem_url: Some("https://codeforces.com/problemset/problem/2000/C".into()),
                raw_rating: Some(1900.0),
                normalized_rating: Some(1900),
                tags: vec!["graphs".into(), "dp".into()],
                verdict: "WA".into(),
                language: Some("C++20".into()),
                submitted_at: 1_760_000_100,
            },
            xcpc_plugin::OjSubmission {
                submission_id: "s2".into(),
                handle: "alice".into(),
                contest_id: Some("2000".into()),
                problem_index: Some("C".into()),
                problem_key: "2000C".into(),
                problem_title: "Graph Paths".into(),
                problem_url: Some("https://codeforces.com/problemset/problem/2000/C".into()),
                raw_rating: Some(1900.0),
                normalized_rating: Some(1900),
                tags: vec!["graphs".into(), "dp".into()],
                verdict: "AC".into(),
                language: Some("C++20".into()),
                submitted_at: 1_760_000_200,
            },
            xcpc_plugin::OjSubmission {
                submission_id: "s3".into(),
                handle: "alice".into(),
                contest_id: Some("2000".into()),
                problem_index: Some("D".into()),
                problem_key: "2000D".into(),
                problem_title: "Hard Tree".into(),
                problem_url: Some("https://codeforces.com/problemset/problem/2000/D".into()),
                raw_rating: Some(2200.0),
                normalized_rating: Some(2200),
                tags: vec!["trees".into()],
                verdict: "TLE".into(),
                language: Some("C++20".into()),
                submitted_at: 1_760_000_300,
            },
        ],
        "2026-11-24T12:00:00Z",
    )
    .unwrap();
    db.upsert_contests(
        "codeforces",
        &[xcpc_plugin::OjContest {
            contest_id: "2100".into(),
            name: "Codeforces Round 1050".into(),
            start_time: 4_000_000_000,
            duration_sec: 7200,
            format: Some("CF".into()),
            url: Some("https://codeforces.com/contest/2100".into()),
        }],
        "2026-11-24T12:00:00Z",
    )
    .unwrap();

    // 1. Dashboard query.
    let dash_res = preview.training_dashboard(Some("all".into()), Some(2025), Some(1600));
    assert!(dash_res.ok, "dashboard error: {:?}", dash_res.error);
    let dash: JsonValue = dash_res.report_json.unwrap().parse().unwrap();
    assert_eq!(dash["accounts"][0]["handle"], "alice");
    assert_eq!(dash["accounts"][0]["rating"], 1950);
    assert_eq!(dash["kpi"]["total_ac"], 1);
    assert_eq!(dash["kpi"]["total_submissions"], 3);
    assert_eq!(dash["rating_series"][0]["platform"], "codeforces");
    assert_eq!(dash["rating_series"][0]["points"][0]["delta"], 60);

    // 2. Contests query.
    let contests_res = preview.training_contests(Some("codeforces".into()), false);
    assert!(contests_res.ok, "contests error: {:?}", contests_res.error);
    let contests: JsonValue = contests_res.report_json.unwrap().parse().unwrap();
    assert_eq!(contests.as_array().unwrap().len(), 1);
    assert_eq!(contests[0]["contest_id"], "2100");
    assert_eq!(contests[0]["status"], "upcoming");

    // 3. Submissions query (by_problem + unsolved_only).
    let subs_res = preview.training_submissions(crate::TrainingSubmissionsQueryJs {
        by_problem: true,
        unsolved_only: true,
        ..Default::default()
    });
    assert!(subs_res.ok, "submissions error: {:?}", subs_res.error);
    let subs: JsonValue = subs_res.report_json.unwrap().parse().unwrap();
    assert_eq!(subs["total"], 1);
    assert_eq!(subs["items"][0]["problem_key"], "2000D");
    assert!(subs["items"][0]["library_problem_id"].is_null());

    // 4. Ingest problem from submission, verify dedup and workspace refresh.
    let ingested = preview.training_ingest_problem(crate::IngestProblemRequestJs {
        platform: Some("codeforces".into()),
        problem_key: Some("2000D".into()),
        title: Some("Hard Tree".into()),
        rating: Some(2200),
        tags: Some(vec!["trees".into()]),
        ..Default::default()
    });
    assert!(ingested.ok, "ingest error: {:?}", ingested.error);
    assert!(ingested.created);
    assert_eq!(ingested.problem_id, "cf-2000d");
    assert_eq!(ingested.title, "Hard Tree");

    // Workspace index & submissions query now reflect `library_problem_id`.
    let ws_info = preview.workspace();
    assert!(ws_info.problems.iter().any(|p| p.id == "cf-2000d"));
    let subs_after = preview.training_submissions(crate::TrainingSubmissionsQueryJs {
        by_problem: true,
        unsolved_only: true,
        ..Default::default()
    });
    let subs_after_json: JsonValue = subs_after.report_json.unwrap().parse().unwrap();
    assert_eq!(
        subs_after_json["items"][0]["library_problem_id"],
        "cf-2000d"
    );

    // Second ingest deduplicates cleanly.
    let ingested2 = preview.training_ingest_problem(crate::IngestProblemRequestJs {
        platform: Some("codeforces".into()),
        problem_key: Some("2000D".into()),
        ..Default::default()
    });
    assert!(ingested2.ok);
    assert!(!ingested2.created);
    assert_eq!(ingested2.problem_id, "cf-2000d");

    // 5. External URL validation rejects non-http(s) schemes.
    let bad_url = Preview::open_external_url("javascript:alert(1)".into());
    assert!(!bad_url.ok);
    assert_eq!(bad_url.code.as_deref(), Some("invalid_url"));
}

#[test]
fn phase4c_plugin_toolbox_commands_and_render_bridge() {
    let data_dir = TempDir::new("phase4c-toolbox");
    let ws_dir = TempDir::new("phase4c-toolbox-ws");
    let preview = Preview::new(Some(data_dir.0.display().to_string()));
    let ws_info = preview.init_workspace(ws_dir.0.display().to_string(), None);
    assert!(ws_info.error.is_none());
    assert!(
        preview
            .create_problem("alpha".into(), "Alpha Problem".into())
            .ok
    );

    // Install two local tool plugins (official tools live in the remote official repo):
    // 1. `demo-headless`: zero-permission headless tool + global command, no UI entry.
    let headless_src = TempDir::new("phase4c-headless-src");
    std::fs::write(
        headless_src.0.join("manifest.toml"),
        r#"id = "demo-headless"
name = "Demo Headless"
version = "0.1.0"
description = "Headless tool"
capabilities = ["tool", "commands"]
entry = "index.js"

[[tools]]
id = "echo"
name = "Echo"
description = "Echoes text"

[[commands]]
id = "open-echo"
title = "Open Echo"
context = "global"
"#,
    )
    .unwrap();
    std::fs::write(
        headless_src.0.join("index.js"),
        r#"exports.runTool = function(toolId, inputs) {
  return { blocks: [{ type: "text", content: "echo:" + (inputs.text || "") }] };
};
exports.runCommand = function() {
  return { kind: "toast", message: "ok" };
};
"#,
    )
    .unwrap();
    let inst1 = preview
        .state
        .install_plugin_from_path(&headless_src.0, false);
    assert!(inst1.ok, "install demo-headless failed: {:?}", inst1.error);

    // 2. `demo-web-ui`: pure-Web `ui.html` plugin declaring `workspace_read = true` (starts pending_trust).
    let webui_src = TempDir::new("phase4c-webui-src");
    std::fs::write(
        webui_src.0.join("manifest.toml"),
        r#"id = "demo-web-ui"
name = "Demo Web UI"
version = "0.1.0"
description = "WebView tool"
capabilities = ["tool", "commands"]
ui_entry = "ui.html"

[permissions]
workspace_read = true

[[commands]]
id = "open-ui"
title = "Open Web UI"
context = "global"
"#,
    )
    .unwrap();
    std::fs::write(
        webui_src.0.join("index.js"),
        r#"exports.runCommand = function() { return { kind: "open_tool", plugin_id: "demo-web-ui" }; };"#,
    )
    .unwrap();
    std::fs::write(
        webui_src.0.join("ui.html"),
        "<!doctype html><html><body><h1>Demo Web UI</h1></body></html>",
    )
    .unwrap();
    let inst2 = preview.state.install_plugin_from_path(&webui_src.0, false);
    assert!(inst2.ok, "install demo-web-ui failed: {:?}", inst2.error);

    // 1. Before trusting demo-web-ui, plugin_list_tools only returns trusted demo-headless,
    //    and load_plugin_ui_html + plugin_workspace_query return PLUGIN_UNTRUSTED.
    let listed_before = preview.plugin_list_tools();
    assert!(
        listed_before.ok,
        "list tools error: {:?}",
        listed_before.error
    );
    let groups_before: serde_json::Value =
        serde_json::from_str(&listed_before.report_json.unwrap()).unwrap();
    let ids_before: HashSet<&str> = groups_before
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|g| g["plugin_id"].as_str())
        .collect();
    assert!(
        ids_before.contains("demo-headless"),
        "missing demo-headless: {ids_before:?}"
    );
    assert!(
        !ids_before.contains("demo-web-ui"),
        "untrusted demo-web-ui should not appear in list_tools: {ids_before:?}"
    );

    let untrusted_ui = preview.load_plugin_ui_html("demo-web-ui".into());
    assert!(!untrusted_ui.ok);
    assert_eq!(untrusted_ui.error_code.as_deref(), Some("PLUGIN_UNTRUSTED"));

    let untrusted_ws =
        preview.plugin_workspace_query("demo-web-ui".into(), "listProblems".into(), None);
    assert!(!untrusted_ws.ok);
    assert_eq!(untrusted_ws.error_code.as_deref(), Some("PLUGIN_UNTRUSTED"));

    // 2. Trust demo-web-ui: now it appears in plugin_list_tools, and load_plugin_ui_html +
    //    plugin_workspace_query succeed.
    assert!(preview.trust_plugin("demo-web-ui".into()).ok);
    let listed = preview.plugin_list_tools();
    assert!(listed.ok, "list tools error: {:?}", listed.error);
    let groups: serde_json::Value = serde_json::from_str(&listed.report_json.unwrap()).unwrap();
    let arr = groups.as_array().unwrap();
    let ids: HashSet<&str> = arr.iter().filter_map(|g| g["plugin_id"].as_str()).collect();
    assert!(
        ids.contains("demo-headless"),
        "missing demo-headless: {ids:?}"
    );
    assert!(ids.contains("demo-web-ui"), "missing demo-web-ui: {ids:?}");

    // 3. plugin_run_tool runs headless tool cleanly.
    let run_res = preview
        .state
        .plugin_run_tool("demo-headless", "echo", r#"{"text":"hello"}"#);
    assert!(run_res.ok, "run_tool error: {:?}", run_res.error);
    let exec_json: serde_json::Value = serde_json::from_str(&run_res.report_json.unwrap()).unwrap();
    assert_eq!(exec_json["blocks"][0]["content"], "echo:hello");

    let web_ui = preview.load_plugin_ui_html("demo-web-ui".into());
    assert!(web_ui.ok, "load_plugin_ui_html error: {:?}", web_ui.error);
    assert_eq!(web_ui.plugin_id.as_deref(), Some("demo-web-ui"));
    assert_eq!(web_ui.workspace_read, Some(true));
    assert!(
        web_ui
            .html
            .as_deref()
            .unwrap_or("")
            .contains("<h1>Demo Web UI</h1>")
    );

    let ws_list = preview.plugin_workspace_query("demo-web-ui".into(), "listProblems".into(), None);
    assert!(ws_list.ok, "workspace query error: {:?}", ws_list.error);
    let ws_problems: serde_json::Value = serde_json::from_str(&ws_list.data_json.unwrap()).unwrap();
    assert_eq!(ws_problems.as_array().unwrap().len(), 1);
    assert_eq!(ws_problems[0]["id"], "alpha");

    // demo-headless does not declare workspace_read -> PERMISSION_DENIED.
    let denied_ws =
        preview.plugin_workspace_query("demo-headless".into(), "listProblems".into(), None);
    assert!(!denied_ws.ok);
    assert_eq!(denied_ws.error_code.as_deref(), Some("PERMISSION_DENIED"));

    // 4. plugin_kv_{get,set,remove} roundtrip.
    assert!(
        preview
            .plugin_kv_set("demo-web-ui".into(), "state".into(), r#"{"n":42}"#.into())
            .ok
    );
    let got = preview.plugin_kv_get("demo-web-ui".into(), "state".into());
    assert!(got.ok && got.found);
    assert_eq!(got.value_json.as_deref(), Some(r#"{"n":42}"#));
    assert!(
        preview
            .plugin_kv_remove("demo-web-ui".into(), "state".into())
            .ok
    );
    let after_rm = preview.plugin_kv_get("demo-web-ui".into(), "state".into());
    assert!(after_rm.ok && !after_rm.found);

    // 5. plugin_render_svg_png rasterizes valid SVG into non-empty base64 PNG.
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="32"><rect width="64" height="32" fill="#0b6989"/></svg>"##;
    let png_res = Preview::plugin_render_svg_png(svg.into(), Some(2.0));
    assert!(png_res.ok, "svg_to_png error: {:?}", png_res.error);
    let b64 = png_res.png_base64.unwrap();
    assert!(!b64.is_empty());

    // 6. plugin_list_commands lists installed commands and respects disabling.
    let cmds = preview.plugin_list_commands(Some("global".into()));
    assert!(cmds.ok, "list commands error: {:?}", cmds.error);
    let cmds_json: serde_json::Value = serde_json::from_str(&cmds.report_json.unwrap()).unwrap();
    let cmd_arr = cmds_json.as_array().unwrap();
    assert!(cmd_arr.iter().any(|c| c["plugin_id"] == "demo-headless"));
    assert!(cmd_arr.iter().any(|c| c["plugin_id"] == "demo-web-ui"));

    // Disabling demo-web-ui removes it from both list_tools and list_commands.
    let dis = preview.set_plugin_enabled("demo-web-ui".into(), false);
    assert!(dis.ok);
    let listed_after = preview.plugin_list_tools();
    let groups_after: serde_json::Value =
        serde_json::from_str(&listed_after.report_json.unwrap()).unwrap();
    assert!(
        !groups_after
            .as_array()
            .unwrap()
            .iter()
            .any(|g| g["plugin_id"] == "demo-web-ui")
    );
}

#[test]
fn state_info_reads_only_in_memory_index_without_disk_io() {
    let (dir, preview) = init_temp_preview("state-info-in-memory");
    assert!(preview.create_problem("p1".into(), "Alpha".into()).ok);
    assert!(preview.create_contest("c1".into(), "Round 1".into()).ok);
    assert!(preview.compose_contest("c1".into(), vec!["p1".into()]).ok);

    let before = preview.workspace();
    assert_eq!(before.problems.len(), 1);
    assert_eq!(before.problems[0].title, "Alpha");
    assert_eq!(before.contests.len(), 1);
    assert_eq!(before.contests[0].items.len(), 1);
    assert_eq!(before.contests[0].items[0].title, "Alpha");

    // Delete on-disk manifests behind State's back (bypassing index refresh).
    // `State::info` (`preview.workspace()`) must still return the exact same snapshot
    // strictly from the in-memory `core.index` without touching the filesystem.
    std::fs::remove_file(dir.0.join("problems/p1/problem.toml")).unwrap();
    std::fs::remove_file(dir.0.join("contests/c1/contest.toml")).unwrap();
    let _ = std::fs::remove_file(dir.0.join("workspace.toml"));

    let after = preview.workspace();
    assert_eq!(after.problems.len(), 1);
    assert_eq!(after.problems[0].id, "p1");
    assert_eq!(after.problems[0].title, "Alpha");
    assert_eq!(after.contests.len(), 1);
    assert_eq!(after.contests[0].id, "c1");
    assert_eq!(after.contests[0].items.len(), 1);
    assert_eq!(after.contests[0].items[0].id, "p1");
    assert_eq!(after.contests[0].items[0].title, "Alpha");
}

#[test]
fn list_templates_json_hits_cache_on_problem_edits_and_invalidates_on_template_change() {
    let (dir, preview) = init_temp_preview("templates-cache-hit");
    assert!(preview.create_problem("p1".into(), "Cache Test".into()).ok);

    // Warm the template schema cache.
    let initial: serde_json::Value = serde_json::from_str(&preview.list_templates_json()).unwrap();
    assert!(initial["templates"].is_array());

    // Hold `math_engine` lock while editing problem files and querying `list_templates_json`.
    // If `list_templates_json` attempted to re-evaluate templates via `math_engine`, it would
    // deadlock here.
    {
        let _math_guard = preview.state.math_engine.lock().unwrap();
        assert!(
            preview
                .save(
                    "problems/p1/statement.md".into(),
                    "## 题目描述\n\nEdited.\n".into(),
                    None,
                )
                .ok
        );
        assert!(
            preview
                .update_problem_meta(
                    "p1".into(),
                    ProblemMetaPatchJs {
                        title: Some("Cache Test Edited".into()),
                        ..Default::default()
                    },
                )
                .ok
        );
        let hit: serde_json::Value = serde_json::from_str(&preview.list_templates_json()).unwrap();
        assert_eq!(hit["templates"], initial["templates"]);
    }

    // Adding a new template file under `templates/` must invalidate the fingerprint cache
    // and expose the new template in `list_templates_json`.
    let templates_dir = dir.0.join("templates");
    std::fs::create_dir_all(&templates_dir).unwrap();
    std::fs::write(
        templates_dir.join("custom-perf.typ"),
        "#let xcpc-template = (name: (zh: \"自定义\", en: \"Custom\"), options: ())\n#let render(data) = [ok]\n",
    )
    .unwrap();

    let refreshed: serde_json::Value =
        serde_json::from_str(&preview.list_templates_json()).unwrap();
    let ids: Vec<&str> = refreshed["templates"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["id"].as_str())
        .collect();
    assert!(
        ids.contains(&"custom-perf"),
        "expected custom-perf template after cache invalidation, got {ids:?}"
    );
}
