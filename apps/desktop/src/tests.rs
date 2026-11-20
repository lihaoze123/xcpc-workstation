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
    let info = preview.init_workspace(dir.0.display().to_string());
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
    let first = preview
        .state
        .lock()
        .unwrap()
        .run(&req, 1.0, &[page_view(0)], token);
    assert!(first.error.is_none());
    assert_eq!(first.pages.len(), 1);
    assert!(first.compile_ms > 0.0);

    // Changing only scale/view hits the cached Compiled with compile_ms == 0.0.
    let second = preview
        .state
        .lock()
        .unwrap()
        .run(&req, 1.5, &[page_view(0)], token);
    assert!(second.error.is_none());
    assert_eq!(second.compile_ms, 0.0);
    assert_eq!(second.pages.len(), 1);

    // save() increments revision and invalidates cached Compiled.
    let rev_before_save = preview.state.lock().unwrap().revision;
    let saved = preview.save("problems/p1/statement.md".into(), md.into(), None);
    assert!(saved.ok, "save failed: {:?}", saved.error);
    {
        let state = preview.state.lock().unwrap();
        assert_eq!(state.revision, rev_before_save + 1);
        assert!(state.cached.is_none());
    }
    let after_save = preview
        .state
        .lock()
        .unwrap()
        .run(&req, 1.5, &[page_view(0)], token);
    assert!(after_save.compile_ms > 0.0);

    // switch() increments revision and invalidates cached Compiled.
    let rev_before_switch = preview.state.lock().unwrap().revision;
    let switched = preview.open_workspace(dir.0.display().to_string(), false);
    assert!(switched.error.is_none());
    {
        let state = preview.state.lock().unwrap();
        assert_eq!(state.revision, rev_before_switch + 1);
        assert!(state.cached.is_none());
    }
    let after_switch = preview
        .state
        .lock()
        .unwrap()
        .run(&req, 1.5, &[page_view(0)], token);
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
    let first = preview
        .state
        .lock()
        .unwrap()
        .run(&req, 0.9, &[page_view(0)], token);
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
        let state = preview.state.lock().unwrap();
        let fps = &state.cached.as_ref().unwrap().fingerprints;
        assert_eq!(fps.len(), 5);
        assert!(fps[0].is_some());
        assert!(fps[1..].iter().all(Option::is_none));
    }

    // Move view to page 2 at a different scale.
    let second = preview
        .state
        .lock()
        .unwrap()
        .run(&req, 1.2, &[page_view(2)], token);
    assert_eq!(second.compile_ms, 0.0);
    assert_eq!(second.rendered, 1);
    assert_eq!(second.pages[0].layers.len(), 1);
    assert_eq!(second.pages[0].layers[0].key, first.pages[0].layers[0].key);
    assert_eq!(second.pages[2].layers.len(), 1);
    assert!(second.pages[1].layers.is_empty());
    assert!(second.pages[3].layers.is_empty());
    assert!(second.pages[4].layers.is_empty());
    {
        let state = preview.state.lock().unwrap();
        let fps = &state.cached.as_ref().unwrap().fingerprints;
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
    let stale = preview.state.lock().unwrap().run(
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
    let compiled_res = preview
        .state
        .lock()
        .unwrap()
        .run(&req, 1.0, &[page_view(0)], live_token);
    assert_eq!(compiled_res.pages.len(), 5);

    let state = preview.state.lock().unwrap();
    let compiled = &state.cached.as_ref().unwrap().compiled;
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
    let res = preview
        .state
        .lock()
        .unwrap()
        .run(&req, 1.0, &[page_view(0)], token);
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
    let lint_res = ws_preview
        .state
        .lock()
        .unwrap()
        .run(&req, 1.0, &[page_view(0)], token);
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
    let res = preview
        .state
        .lock()
        .unwrap()
        .run(&req, 1.0, &[page_view(1)], token);
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
        .lock()
        .unwrap()
        .watch_session
        .clone()
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
    let rev_before = preview.state.lock().unwrap().revision;
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
    let js_ev = preview
        .state
        .lock()
        .unwrap()
        .apply_watch_event(session.id, ev);
    assert_eq!(js_ev.kind, "changed");
    assert!(
        js_ev.paths.iter().any(|p| p == "problems/p1/problem.toml"),
        "expected problem.toml in watch paths, got {:?}",
        js_ev.paths
    );
    assert_eq!(preview.state.lock().unwrap().revision, rev_before + 1);

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

    let s1 = preview
        .state
        .lock()
        .unwrap()
        .watch_session
        .clone()
        .expect("session 1");
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
    let s2 = preview
        .state
        .lock()
        .unwrap()
        .watch_session
        .clone()
        .expect("session 2");
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
    let info1 = preview.init_workspace(ws1.0.display().to_string());
    assert!(info1.error.is_none());
    assert!(ws1.0.join("workspace.toml").is_file());
    assert!(ws1.0.join("AGENTS.md").is_file());
    assert!(ws1.0.join(".gitignore").is_file());

    // Re-initializing never overwrites an existing user AGENTS.md.
    std::fs::write(ws1.0.join("AGENTS.md"), "# Custom Guide\n").unwrap();
    let info1_again = preview.init_workspace(ws1.0.display().to_string());
    assert!(info1_again.error.is_none());
    assert_eq!(
        std::fs::read_to_string(ws1.0.join("AGENTS.md")).unwrap(),
        "# Custom Guide\n"
    );

    // Opening a second workspace moves it to the front of recent_workspaces.
    let info2 = preview.init_workspace(ws2.0.display().to_string());
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
    let fail_freeze = preview.state.lock().unwrap().freeze_contest("r1");
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
    let ok_freeze = preview.state.lock().unwrap().freeze_contest("r1");
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
            .init_workspace(ws.0.display().to_string())
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
