use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use nsh_core::JsonValue;
use nsh_core::element::{ImageSrc, JsValue, TsTypes};
use xcpc_domain::Patch;

use crate::Preview;
use crate::cache::{CancelToken, Job, LayerKind, PixelCache, render_chunk, tiles};
use crate::types::{
    AppPrefsJs, ContestMetaPatchJs, OverridePatchJs, PreviewRequest, PreviewView,
    ProblemMetaPatchJs, ProblemSearchQuery, ProblemSourceJs, SamplePairJs,
};

fn rgba_src(w: u32, h: u32) -> (ImageSrc, usize) {
    let bytes = (w as usize) * (h as usize) * 4;
    (ImageSrc::rgba(w, h, vec![255u8; bytes]), bytes)
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
    std::fs::write(dir.0.join("workspace.toml"), "").unwrap();
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
fn tristate_patch_types_declare_valid_ts_interfaces_and_decode_keep_clear_set() {
    let mut types = TsTypes::default();
    assert_eq!(
        ProblemMetaPatchJs::ts_type(&mut types),
        "ProblemMetaPatchJs"
    );
    assert_eq!(
        ContestMetaPatchJs::ts_type(&mut types),
        "ContestMetaPatchJs"
    );
    assert_eq!(OverridePatchJs::ts_type(&mut types), "OverridePatchJs");

    // Verify tri-state decoding via nsh_core's JsonHost: omitted -> Keep, null -> Clear, value -> Set.
    let ov_json: JsonValue = r#"{"title":"New Title","time":null,"showMetadata":false}"#
        .parse()
        .unwrap();
    let ov: OverridePatchJs = nsh_core::element::from_json(&ov_json).unwrap();
    assert_eq!(ov.title, Patch::Set("New Title".into()));
    assert_eq!(ov.time, Patch::Clear);
    assert_eq!(ov.memory, Patch::Keep);
    assert_eq!(ov.input, Patch::Keep);
    assert_eq!(ov.output, Patch::Keep);
    assert_eq!(ov.show_metadata, Patch::Set(false));

    let pm_json: JsonValue = r#"{"title":"T","rating":null,"author":"Bob","memory":"512MB"}"#
        .parse()
        .unwrap();
    let pm: ProblemMetaPatchJs = nsh_core::element::from_json(&pm_json).unwrap();
    assert_eq!(pm.title.as_deref(), Some("T"));
    assert_eq!(pm.rating, Patch::Clear);
    assert_eq!(pm.author, Patch::Set("Bob".into()));
    assert_eq!(pm.time, Patch::Keep);
    assert_eq!(pm.memory, Patch::Set("512MB".into()));

    let cm_json: JsonValue = r#"{"location":null,"cover":true,"blankPages":false}"#
        .parse()
        .unwrap();
    let cm: ContestMetaPatchJs = nsh_core::element::from_json(&cm_json).unwrap();
    assert_eq!(cm.name, None);
    assert_eq!(cm.location, Patch::Clear);
    assert_eq!(cm.date, Patch::Keep);
    assert_eq!(cm.cover, Patch::Set(true));
    assert_eq!(cm.blank_pages, Some(false));
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
    let info = preview.init_example_workspace(ws_dir.0.display().to_string(), None);
    assert!(
        info.error.is_none(),
        "init example failed: {:?}",
        info.error
    );
    assert_eq!(info.problems.len(), 3);
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
fn theme_overrides_merge_global_and_workspace_layers() {
    let data = TempDir::new("theme-global");
    let ws = TempDir::new("theme-ws");
    std::fs::write(
        data.0.join("theme.toml"),
        "[colors.light]\nprimary = \"#112233\"\nbackground = \"#fafafa\"\n[editor]\nfont_size = 15\n[radius]\nbase = 10\n",
    )
    .unwrap();
    std::fs::create_dir_all(ws.0.join(".xcpc")).unwrap();
    std::fs::write(
        ws.0.join(".xcpc/theme.toml"),
        "[colors.light]\nprimary = \"#445566\"\n[editor]\nfont_family = \"JetBrains Mono\"\n",
    )
    .unwrap();

    let merged = crate::theme_config::load_theme_overrides(Some(&data.0), Some(&ws.0));
    let find_light = |k: &str| {
        merged
            .light_colors
            .iter()
            .find(|c| c.key == k)
            .map(|c| c.value.as_str())
    };
    assert_eq!(find_light("primary"), Some("#445566"));
    assert_eq!(find_light("background"), Some("#fafafa"));
    assert_eq!(merged.editor_font_family.as_deref(), Some("JetBrains Mono"));
    assert_eq!(merged.editor_font_size, Some(15.0));
    assert_eq!(merged.radius, Some(10.0));
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
