//! Cold-start preview snapshot persistence under `<dataDir>/snapshots/`.
//!
//! Stores lightweight JSON metadata (`cold-start.json`) alongside 1–2 visible pages'
//! PNG-compressed RGBA images (`page-<n>.png`). On startup,
//! [`load_cold_start_snapshot`] verifies the active workspace and recomputes the target's
//! lightweight on-disk metadata fingerprint (`path + mtime + len`, plus app version,
//! commit, and font source); if anything changed, the stale snapshot is ignored.

use std::collections::HashSet;
use std::io::BufReader;
use std::path::Path;
use std::time::UNIX_EPOCH;

use nsh_core::JsonValue;
use nsh_core::element::ImageSrc;
use xcpc_domain::ASSET_MARKDOWN_DOCS;
use xcpc_engine::Compiled;
use xcpc_storage::{Workspace, content_hash, write_atomic};

use crate::cache::{LayerKind, PIXELS};
use crate::prefs::snapshots_dir;
use crate::types::{ColdStartSnapshotJs, PreviewLayer, PreviewPage};

/// Snapshot metadata schema version (bumped whenever fingerprint inputs or format change).
const SNAPSHOT_FORMAT_VERSION: u64 = 3;

/// Clamped rasterization scale range for cold-start snapshots (`scale * devicePixelRatio`).
const MIN_SNAPSHOT_SCALE: f32 = 1.0;
const MAX_SNAPSHOT_SCALE: f32 = 2.0;

/// Maximum number of visible pages persisted in a cold-start snapshot.
const MAX_SNAPSHOT_PAGES: usize = 2;

const SNAPSHOT_META_FILE: &str = "cold-start.json";

fn font_source_fingerprint() -> String {
    let env_dirs = std::env::var_os("XCPC_FONT_DIRS")
        .map(|v| v.to_string_lossy().into_owned())
        .unwrap_or_default();
    let has_sibling_fonts = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("fonts").is_dir()))
        .unwrap_or(false);
    format!(
        "env={env_dirs};sibling={has_sibling_fonts};bundled={}",
        xcpc_engine::HAS_BUNDLED_FONTS
    )
}

/// Computes a fast deterministic SHA-256 metadata fingerprint covering all on-disk inputs
/// (`path + len + mtime` for docs, TOML manifests, `samples/`, and `assets/`), plus app
/// version, git commit, and font source for `(target, id, book)` in `ws`.
///
/// Uses `fs::metadata` instead of reading every asset's bytes so cold-start validation
/// and background snapshot saves stay fast even for asset-heavy workspaces.
pub(crate) fn target_content_hash(
    ws: &Workspace,
    target: &str,
    id: &str,
    book: &str,
) -> Option<String> {
    let mut acc = String::new();
    acc.push_str("app=");
    acc.push_str(env!("CARGO_PKG_VERSION"));
    acc.push('+');
    acc.push_str(env!("XCPC_GIT_COMMIT"));
    acc.push('\n');
    acc.push_str("fonts=");
    acc.push_str(&font_source_fingerprint());
    acc.push('\n');
    acc.push_str(target);
    acc.push('\0');
    acc.push_str(id);
    acc.push('\0');
    acc.push_str(book);
    acc.push('\0');

    match target {
        "problem" => {
            append_problem_stat(ws, &format!("problems/{id}"), &mut acc)?;
        }
        "contest" => {
            let contest = ws.load_contest(id).ok()?;
            let toml_rel = format!("contests/{id}/contest.toml");
            append_file_stat(ws, &toml_rel, &mut acc)?;
            append_tree_stat(ws, &format!("contests/{id}/assets"), &mut acc);
            for item in &contest.meta.problems {
                let dir = if contest.meta.frozen {
                    format!("contests/{id}/snapshot/{}", item.label)
                } else {
                    format!("problems/{}", item.id)
                };
                append_problem_stat(ws, &dir, &mut acc)?;
            }
        }
        _ => return None,
    }

    Some(content_hash(acc.as_bytes()))
}

fn append_file_stat(ws: &Workspace, rel: &str, acc: &mut String) -> Option<()> {
    let abs = ws.resolve(rel).ok()?;
    let meta = std::fs::metadata(&abs).ok()?;
    if !meta.is_file() {
        return None;
    }
    let (secs, nanos) = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| (d.as_secs(), d.subsec_nanos()))
        .unwrap_or((0, 0));
    use std::fmt::Write as _;
    let _ = writeln!(acc, "{rel}\0{}:{secs}.{nanos}", meta.len());
    Some(())
}

fn append_problem_stat(ws: &Workspace, dir: &str, acc: &mut String) -> Option<()> {
    append_file_stat(ws, &format!("{dir}/problem.toml"), acc)?;

    for name in ASSET_MARKDOWN_DOCS {
        let rel = format!("{dir}/{name}");
        let _ = append_file_stat(ws, &rel, acc);
    }

    append_tree_stat(ws, &format!("{dir}/samples"), acc);
    append_tree_stat(ws, &format!("{dir}/assets"), acc);

    Some(())
}

fn append_tree_stat(ws: &Workspace, rel_dir: &str, acc: &mut String) {
    let Ok(abs_dir) = ws.resolve(rel_dir) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(&abs_dir) else {
        return;
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| !n.starts_with('.'))
        .collect();
    names.sort();
    for name in names {
        let child_rel = format!("{rel_dir}/{name}");
        let Ok(child_abs) = ws.resolve(&child_rel) else {
            continue;
        };
        if child_abs.is_dir() {
            append_tree_stat(ws, &child_rel, acc);
        } else {
            let _ = append_file_stat(ws, &child_rel, acc);
        }
    }
}

fn encode_page_png(width: u32, height: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    if width == 0 || height == 0 {
        return None;
    }
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fast);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(rgba).ok()?;
    }
    Some(out)
}

fn decode_page_png(path: &Path) -> Option<(u32, u32, Vec<u8>)> {
    let file = std::fs::File::open(path).ok()?;
    let decoder = png::Decoder::new(BufReader::new(file));
    let mut reader = decoder.read_info().ok()?;
    let info = reader.info();
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return None;
    }
    let w = info.width;
    let h = info.height;
    let expected_len = (w as usize).checked_mul(h as usize)?.checked_mul(4)?;
    if w == 0 || h == 0 || reader.output_buffer_size() != Some(expected_len) {
        return None;
    }
    let mut buf = vec![0u8; expected_len];
    let frame = reader.next_frame(&mut buf).ok()?;
    if frame.width != w || frame.height != h || frame.buffer_size() != expected_len {
        return None;
    }
    Some((w, h, buf))
}

/// Persists `meta_json` and up to 2 PNG-compressed rendered pages from `compiled` into
/// `<data_dir>/snapshots/` at `render_scale` (clamped to `1.0..=2.0`). Returns `true` on success.
pub(crate) fn save_cold_start_snapshot(
    data_dir: Option<&Path>,
    ws: Option<&Workspace>,
    compiled: Option<&Compiled>,
    meta_json: &str,
    pages: &[u32],
    render_scale: f32,
) -> bool {
    let (Some(data_dir), Some(ws), Some(compiled)) = (data_dir, ws, compiled) else {
        return false;
    };
    let Ok(meta_val) = meta_json.parse::<JsonValue>() else {
        return false;
    };
    let Some(obj) = meta_val.as_object() else {
        return false;
    };

    let workspace = obj
        .get("workspace")
        .and_then(JsonValue::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| ws.root().display().to_string());
    let target = obj
        .get("target")
        .and_then(JsonValue::as_str)
        .unwrap_or("")
        .to_owned();
    let id = obj
        .get("id")
        .and_then(JsonValue::as_str)
        .unwrap_or("")
        .to_owned();
    let book = obj
        .get("book")
        .and_then(JsonValue::as_str)
        .unwrap_or("statements")
        .to_owned();
    let zoom = obj
        .get("zoom")
        .and_then(JsonValue::as_f64)
        .filter(|v| v.is_finite() && *v > 0.0)
        .unwrap_or(1.0);
    let scroll_y = obj
        .get("scrollY")
        .and_then(JsonValue::as_f64)
        .filter(|v| v.is_finite() && *v >= 0.0)
        .unwrap_or(0.0);

    let Some(server_hash) = target_content_hash(ws, &target, &id, &book) else {
        return false;
    };

    let total_pages = compiled.page_count();
    if total_pages == 0 {
        return false;
    }

    let dir = snapshots_dir(data_dir);
    if std::fs::create_dir_all(&dir).is_err() {
        return false;
    }

    let mut chosen: Vec<usize> = Vec::with_capacity(MAX_SNAPSHOT_PAGES);
    for &p in pages {
        let idx = p as usize;
        if idx < total_pages && !chosen.contains(&idx) {
            chosen.push(idx);
            if chosen.len() >= MAX_SNAPSHOT_PAGES {
                break;
            }
        }
    }
    if chosen.is_empty() {
        chosen.push(0);
    }

    let mut page_sizes_json = Vec::with_capacity(total_pages);
    for idx in 0..total_pages {
        let (w_pt, h_pt) = compiled.page_size(idx).unwrap_or((595.28, 841.89));
        let mut entry = empty_obj();
        entry["widthPt"] = JsonValue::from(w_pt);
        entry["heightPt"] = JsonValue::from(h_pt);
        page_sizes_json.push(entry);
    }

    let effective_scale = if render_scale.is_finite() {
        render_scale.clamp(MIN_SNAPSHOT_SCALE, MAX_SNAPSHOT_SCALE)
    } else {
        MIN_SNAPSHOT_SCALE
    };

    let mut written_files: HashSet<String> = HashSet::new();
    let mut rendered_json = Vec::with_capacity(chosen.len());
    for idx in chosen {
        let Some(pixmap) = compiled.render(idx, effective_scale) else {
            continue;
        };
        let w = pixmap.width();
        let h = pixmap.height();
        let raw = pixmap.take();
        let Some(png_bytes) = encode_page_png(w, h, &raw) else {
            continue;
        };

        let filename = format!("page-{idx}.png");
        if write_atomic(&dir.join(&filename), &png_bytes).is_err() {
            return false;
        }
        written_files.insert(filename.clone());

        let mut entry = empty_obj();
        entry["page"] = JsonValue::from(idx as u64);
        entry["file"] = JsonValue::from(filename.as_str());
        rendered_json.push(entry);
    }

    if rendered_json.is_empty() {
        return false;
    }

    let mut root = empty_obj();
    root["version"] = JsonValue::from(SNAPSHOT_FORMAT_VERSION);
    root["metaJson"] = JsonValue::from(meta_json);
    root["workspace"] = JsonValue::from(workspace.as_str());
    root["target"] = JsonValue::from(target.as_str());
    root["id"] = JsonValue::from(id.as_str());
    root["book"] = JsonValue::from(book.as_str());
    root["serverHash"] = JsonValue::from(server_hash.as_str());
    root["zoom"] = JsonValue::from(zoom);
    root["scrollY"] = JsonValue::from(scroll_y);
    root["totalPages"] = JsonValue::from(total_pages as u64);
    root["pageSizes"] = JsonValue::Array(page_sizes_json);
    root["rendered"] = JsonValue::Array(rendered_json);

    let serialized = format!("{root:#}\n");
    if write_atomic(&dir.join(SNAPSHOT_META_FILE), serialized.as_bytes()).is_err() {
        return false;
    }

    // Remove stale `.png` and legacy `.rgba` files from earlier snapshots.
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            if let Ok(name) = entry.file_name().into_string()
                && name.starts_with("page-")
                && (name.ends_with(".png") || name.ends_with(".rgba"))
                && !written_files.contains(&name)
            {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    true
}

/// Loads the persisted cold-start snapshot from `<data_dir>/snapshots/` if it matches
/// `ws` and the target's current on-disk content hash. Registers restored page pixels
/// into the shared [`PIXELS`] cache so `<PageImage />` can render them immediately.
pub(crate) fn load_cold_start_snapshot(
    data_dir: Option<&Path>,
    ws: Option<&Workspace>,
    owner_id: u64,
) -> Option<ColdStartSnapshotJs> {
    let (data_dir, ws) = (data_dir?, ws?);
    let dir = snapshots_dir(data_dir);
    let text = std::fs::read_to_string(dir.join(SNAPSHOT_META_FILE)).ok()?;
    let val: JsonValue = text.parse().ok()?;
    let obj = val.as_object()?;

    if obj.get("version").and_then(JsonValue::as_u64) != Some(SNAPSHOT_FORMAT_VERSION) {
        return None;
    }
    let workspace = obj.get("workspace").and_then(JsonValue::as_str)?.to_owned();
    if workspace != ws.root().display().to_string() {
        return None;
    }
    let target = obj.get("target").and_then(JsonValue::as_str)?.to_owned();
    let id = obj.get("id").and_then(JsonValue::as_str)?.to_owned();
    let book = obj
        .get("book")
        .and_then(JsonValue::as_str)
        .unwrap_or("statements")
        .to_owned();
    let server_hash = obj.get("serverHash").and_then(JsonValue::as_str)?;

    // Content hash mismatch -> automatically ignore stale snapshot.
    let current_hash = target_content_hash(ws, &target, &id, &book)?;
    if current_hash != server_hash {
        return None;
    }

    let meta_json = obj
        .get("metaJson")
        .and_then(JsonValue::as_str)
        .unwrap_or("{}")
        .to_owned();
    let zoom = obj.get("zoom").and_then(JsonValue::as_f64).unwrap_or(1.0);
    let scroll_y = obj
        .get("scrollY")
        .and_then(JsonValue::as_f64)
        .unwrap_or(0.0);
    let total_pages = obj.get("totalPages").and_then(JsonValue::as_u64)? as usize;
    if total_pages == 0 {
        return None;
    }

    let mut pages = Vec::with_capacity(total_pages);
    let page_sizes = obj.get("pageSizes").and_then(JsonValue::as_array);
    for idx in 0..total_pages {
        let (w_pt, h_pt) = page_sizes
            .and_then(|arr| arr.get(idx))
            .and_then(JsonValue::as_object)
            .and_then(|o| Some((o.get("widthPt")?.as_f64()?, o.get("heightPt")?.as_f64()?)))
            .unwrap_or((595.28, 841.89));
        pages.push(PreviewPage {
            width: w_pt,
            height: h_pt,
            layers: Vec::new(),
        });
    }

    let rendered = obj.get("rendered").and_then(JsonValue::as_array)?;
    let mut loaded_any = false;
    let mut pixels = PIXELS.lock().unwrap();
    for item in rendered.iter().take(MAX_SNAPSHOT_PAGES) {
        let Some(item_obj) = item.as_object() else {
            continue;
        };
        let Some(page_idx) = item_obj.get("page").and_then(JsonValue::as_u64) else {
            continue;
        };
        let page_idx = page_idx as usize;
        if page_idx >= total_pages {
            continue;
        }
        let Some(file_name) = item_obj.get("file").and_then(JsonValue::as_str) else {
            continue;
        };
        if !file_name.starts_with("page-")
            || !file_name.ends_with(".png")
            || file_name.contains('/')
            || file_name.contains('\\')
        {
            continue;
        }
        let Some((w, h, rgba)) = decode_page_png(&dir.join(file_name)) else {
            continue;
        };
        let byte_len = rgba.len();

        let key = format!("{owner_id}:snapshot:p{page_idx}");
        let src = ImageSrc::rgba(w, h, rgba);
        pixels.insert(key.clone(), src, byte_len, LayerKind::Base, owner_id);
        pages[page_idx].layers.push(PreviewLayer {
            key,
            left: 0.0,
            top: 0.0,
            width: 1.0,
            height: 1.0,
        });
        loaded_any = true;
    }

    if !loaded_any {
        return None;
    }

    Some(ColdStartSnapshotJs {
        meta_json,
        workspace,
        target,
        id,
        book,
        zoom,
        scroll_y,
        pages,
    })
}

fn empty_obj() -> JsonValue {
    "{}".parse().expect("valid empty JSON object")
}
