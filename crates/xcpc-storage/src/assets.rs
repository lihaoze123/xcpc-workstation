//! Problem `assets/` directory management: listing with image dimensions and reference
//! metadata, atomic rename with Markdown rewrite & rollback, single deletion, and
//! batch cleanup of unreferenced assets.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

pub use xcpc_domain::ASSET_MARKDOWN_DOCS;

use crate::watcher::SELF_DELETE_SENTINEL;
use crate::{Error, Result, Workspace, content_hash, write_atomic};

/// Maximum UTF-8 byte length for an asset filename.
const MAX_ASSET_NAME_BYTES: usize = 200;
/// Maximum Unicode scalar count for an asset filename.
const MAX_ASSET_NAME_CHARS: usize = 128;
/// Maximum width/height in pixels for cached Gallery thumbnails (covers 168px@2x).
const THUMB_MAX_DIM: u32 = 336;

/// One reference to an asset inside a problem's Markdown file.
#[derive(Debug, Clone, PartialEq)]
pub struct AssetRef {
    /// Workspace-relative file path (e.g. `"problems/a/statement.md"`).
    pub file: String,
    /// 1-based line number.
    pub line: u32,
    /// Optional `{width=NN%}` percentage.
    pub width_pct: Option<f64>,
}

/// Parsed reference returned by a caller-supplied Markdown reference scanner.
#[derive(Debug, Clone, PartialEq)]
pub struct ScannedAssetRef {
    /// Decoded filename inside `assets/` (e.g. `"fig.png"`).
    pub asset_name: String,
    /// 1-based line number.
    pub line: u32,
    /// Optional `{width=NN%}` percentage.
    pub width_pct: Option<f64>,
}

/// One file inside a problem's `assets/` directory.
#[derive(Debug, Clone, PartialEq)]
pub struct AssetEntry {
    pub name: String,
    /// Path relative to the problem directory, e.g. `"assets/fig.png"`.
    pub rel_path: String,
    /// Absolute path on disk.
    pub abs_path: PathBuf,
    /// Absolute path to a cached 336px PNG thumbnail under `.xcpc/thumbs/`, if available.
    pub thumb_path: Option<PathBuf>,
    pub bytes: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub references: Vec<AssetRef>,
}

/// Result of renaming an asset and updating Markdown references.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameAssetResult {
    pub old_ws_rel: String,
    pub new_ws_rel: String,
    /// Workspace-relative Markdown files that were rewritten.
    pub updated_docs: Vec<String>,
}

/// One asset that failed to be removed during batch cleanup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanUnusedFailure {
    pub name: String,
    pub message: String,
}

/// Result of cleaning unreferenced assets from a problem's `assets/` directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanUnusedResult {
    /// Filenames removed (e.g. `["old.png"]`).
    pub removed: Vec<String>,
    /// Workspace-relative paths removed.
    pub removed_paths: Vec<String>,
    pub freed_bytes: u64,
    /// Assets that could not be removed (cleanup continues for the remaining assets).
    pub failed: Vec<CleanUnusedFailure>,
}

/// Returns `true` if `name` (ignoring any extension after the first `.`) is a Windows
/// reserved device name (`CON`, `PRN`, `AUX`, `NUL`, `COM1`–`COM9`, `LPT1`–`LPT9`).
pub fn is_windows_reserved_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).trim();
    if stem.is_empty() {
        return false;
    }
    let upper = stem.to_ascii_uppercase();
    matches!(
        upper.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

/// Validates that `name` is a safe single-component filename usable on Linux, macOS, and Windows.
///
/// Rejects empty names, hidden names (leading `.`), trailing `.` or whitespace, path
/// separators, `..`, control characters, Windows illegal characters (`: * ? " < > |`),
/// Windows reserved device names (`CON`, `NUL`, etc.), and overly long names.
pub fn is_safe_asset_name(name: &str) -> bool {
    if name.is_empty()
        || name.len() > MAX_ASSET_NAME_BYTES
        || name.chars().count() > MAX_ASSET_NAME_CHARS
        || name.starts_with('.')
        || name.ends_with('.')
        || name.ends_with(' ')
        || name.contains("..")
    {
        return false;
    }
    if name.chars().any(|c| {
        matches!(
            c,
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0'
        ) || c.is_control()
    }) {
        return false;
    }
    !is_windows_reserved_name(name)
}

/// Validates an existing asset filename for deletion or as the source of a rename.
///
/// Unlike [`is_safe_asset_name`] (which enforces cross-platform safety for newly created
/// filenames), this only prevents empty names and path traversal so externally placed
/// files (e.g. containing `:`, `a..b.png`, or Windows reserved names) can still be
/// renamed or deleted.
pub fn is_existing_asset_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains('\0')
}

/// Reads pixel `(width, height)` from a raster image (via `imagesize`) or SVG header.
/// Returns `None` for unsupported formats (e.g. PDF) or malformed files.
pub fn read_image_dimensions(path: &Path) -> Option<(u32, u32)> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    if ext.as_deref() == Some("svg") {
        let mut file = fs::File::open(path).ok()?;
        use std::io::Read;
        let mut buf = vec![0u8; 64 * 1024];
        let n = file.read(&mut buf).ok()?;
        buf.truncate(n);
        return svg_dimensions(&buf);
    }
    let dims = imagesize::size(path).ok()?;
    let w = u32::try_from(dims.width).ok()?;
    let h = u32::try_from(dims.height).ok()?;
    (w > 0 && h > 0).then_some((w, h))
}

fn svg_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    let text = std::str::from_utf8(data).ok()?;
    let lower = text.to_ascii_lowercase();
    let svg_start = lower.find("<svg")?;
    let tag_end = lower[svg_start..].find('>')? + svg_start;
    let tag = &text[svg_start..=tag_end];

    let w_attr = extract_xml_attr(tag, "width").and_then(parse_svg_length);
    let h_attr = extract_xml_attr(tag, "height").and_then(parse_svg_length);
    if let (Some(w), Some(h)) = (w_attr, h_attr) {
        return Some((w, h));
    }

    let vb = extract_xml_attr(tag, "viewBox").or_else(|| extract_xml_attr(tag, "viewbox"))?;
    let nums: Vec<f64> = vb
        .split(|c: char| c.is_ascii_whitespace() || c == ',')
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse::<f64>().ok())
        .collect();
    if nums.len() == 4
        && nums[2].is_finite()
        && nums[3].is_finite()
        && nums[2] > 0.0
        && nums[3] > 0.0
    {
        return Some((nums[2].round() as u32, nums[3].round() as u32));
    }
    None
}

fn extract_xml_attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let lower = tag.to_ascii_lowercase();
    let needle = name.to_ascii_lowercase();
    let mut offset = 0usize;
    while let Some(pos) = lower[offset..].find(&needle) {
        let abs = offset + pos;
        let before_ok = abs == 0
            || lower[..abs]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_ascii_whitespace());
        let after = abs + needle.len();
        if before_ok {
            let rest = tag[after..].trim_start();
            if let Some(rest) = rest.strip_prefix('=') {
                let rest = rest.trim_start();
                if let Some(quote) = rest.chars().next().filter(|&c| c == '"' || c == '\'') {
                    let inner = &rest[1..];
                    if let Some(end) = inner.find(quote) {
                        return Some(&inner[..end]);
                    }
                }
            }
        }
        offset = after;
    }
    None
}

fn parse_svg_length(raw: &str) -> Option<u32> {
    let s = raw.trim();
    if s.ends_with('%') {
        return None;
    }
    let num_part = s.trim_end_matches(|c: char| c.is_ascii_alphabetic()).trim();
    let val: f64 = num_part.parse().ok()?;
    (val.is_finite() && val > 0.0).then_some(val.round() as u32)
}

fn encode_rgba_png(width: u32, height: u32, rgba: &[u8]) -> Option<Vec<u8>> {
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

fn generate_thumbnail_png(abs: &Path, ext: &str) -> Option<Vec<u8>> {
    match ext {
        "png" | "jpg" | "jpeg" | "webp" => {
            let img = image::open(abs).ok()?;
            let thumb = img.thumbnail(THUMB_MAX_DIM, THUMB_MAX_DIM);
            let rgba = thumb.to_rgba8();
            encode_rgba_png(rgba.width(), rgba.height(), rgba.as_raw())
        }
        "svg" => {
            let bytes = fs::read(abs).ok()?;
            let tree =
                resvg::usvg::Tree::from_data(&bytes, &resvg::usvg::Options::default()).ok()?;
            let size = tree.size();
            let (sw, sh) = (size.width(), size.height());
            if !sw.is_finite() || !sh.is_finite() || sw <= 0.0 || sh <= 0.0 {
                return None;
            }
            let scale = THUMB_MAX_DIM as f32 / sw.max(sh);
            let tw = (sw * scale).round().clamp(1.0, THUMB_MAX_DIM as f32) as u32;
            let th = (sh * scale).round().clamp(1.0, THUMB_MAX_DIM as f32) as u32;
            let mut pixmap = tiny_skia::Pixmap::new(tw, th)?;
            let sx = tw as f32 / sw;
            let sy = th as f32 / sh;
            resvg::render(
                &tree,
                tiny_skia::Transform::from_scale(sx, sy),
                &mut pixmap.as_mut(),
            );
            let mut rgba = pixmap.take();
            for px in rgba.as_chunks_mut::<4>().0 {
                let a = px[3];
                if a > 0 && a < 255 {
                    let a32 = a as u32;
                    let half = a32 / 2;
                    px[0] = ((px[0] as u32 * 255 + half) / a32).min(255) as u8;
                    px[1] = ((px[1] as u32 * 255 + half) / a32).min(255) as u8;
                    px[2] = ((px[2] as u32 * 255 + half) / a32).min(255) as u8;
                }
            }
            encode_rgba_png(tw, th, &rgba)
        }
        _ => None,
    }
}

fn metadata_mtime(meta: &fs::Metadata) -> (u64, u32) {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| (d.as_secs(), d.subsec_nanos()))
        .unwrap_or((0, 0))
}

impl Workspace {
    /// Returns a cached 336px PNG thumbnail under `<workspace>/.xcpc/thumbs/<sha256_prefix>.png`
    /// for PNG/JPEG/WEBP/SVG assets. Generates the thumbnail lazily on cache miss unless the
    /// workspace is read-only. Returns `None` on unsupported formats or decode/render failure.
    fn ensure_asset_thumbnail(
        &self,
        file_ws_rel: &str,
        abs: &Path,
        meta: &fs::Metadata,
    ) -> Option<PathBuf> {
        let ext = abs
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)?;
        if !matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp" | "svg") {
            return None;
        }
        let (secs, nanos) = metadata_mtime(meta);
        let key_input = format!(
            "v1:{THUMB_MAX_DIM}:{file_ws_rel}:{}:{secs}.{nanos}",
            meta.len()
        );
        let hash = content_hash(key_input.as_bytes());
        let thumbs_dir = self.root.join(".xcpc").join("thumbs");
        let thumb_abs = thumbs_dir.join(format!("{}.png", &hash[..24]));
        if thumb_abs.is_file() {
            return Some(thumb_abs);
        }
        if self.read_only {
            return None;
        }
        let png_bytes = generate_thumbnail_png(abs, &ext)?;
        fs::create_dir_all(&thumbs_dir).ok()?;
        write_atomic(&thumb_abs, &png_bytes).ok()?;
        Some(thumb_abs)
    }

    /// Lists all files inside `<problem_dir>/assets/`, sorted by filename, with byte size,
    /// image dimensions (when readable), cached thumbnail path, and exact references scanned
    /// via `scan_refs`.
    pub fn list_assets_with<F>(
        &self,
        problem_dir: &str,
        mut scan_refs: F,
    ) -> Result<Vec<AssetEntry>>
    where
        F: FnMut(&str) -> Vec<ScannedAssetRef>,
    {
        let assets_rel = format!("{problem_dir}/assets");
        let assets_abs = self.resolve(&assets_rel)?;
        let entries = match fs::read_dir(&assets_abs) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => {
                return Err(Error::Io {
                    path: assets_rel,
                    source: e,
                });
            }
        };

        let mut refs_by_name: HashMap<String, Vec<AssetRef>> = HashMap::new();
        for doc_name in ASSET_MARKDOWN_DOCS {
            let doc_rel = format!("{problem_dir}/{doc_name}");
            if let Ok(text) = self.read_text(&doc_rel) {
                for r in scan_refs(&text) {
                    refs_by_name
                        .entry(r.asset_name)
                        .or_default()
                        .push(AssetRef {
                            file: doc_rel.clone(),
                            line: r.line,
                            width_pct: r.width_pct,
                        });
                }
            }
        }

        let mut names: Vec<String> = entries
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| !n.starts_with('.'))
            .collect();
        names.sort();

        let mut out = Vec::with_capacity(names.len());
        for name in names {
            let file_ws_rel = format!("{assets_rel}/{name}");
            let Ok(abs) = self.resolve(&file_ws_rel) else {
                continue;
            };
            let Ok(meta) = fs::metadata(&abs) else {
                continue;
            };
            if !meta.is_file() {
                continue;
            }
            let dims = read_image_dimensions(&abs);
            let thumb_path = self.ensure_asset_thumbnail(&file_ws_rel, &abs, &meta);
            let references = refs_by_name.remove(&name).unwrap_or_default();
            out.push(AssetEntry {
                rel_path: format!("assets/{name}"),
                abs_path: abs,
                thumb_path,
                bytes: meta.len(),
                width: dims.map(|(w, _)| w),
                height: dims.map(|(_, h)| h),
                references,
                name,
            });
        }
        Ok(out)
    }

    /// Renames `<problem_dir>/assets/<old_name>` to `<new_name>` and atomically rewrites
    /// matching Markdown references in `statement*.md` and `editorial*.md`.
    ///
    /// If any document contains a reference that cannot be safely rewritten (e.g. inside a
    /// raw Typst block), `rewrite_md` returns `Err(reason)` and the rename is aborted before
    /// touching disk. If any Markdown write fails mid-way, all previously rewritten files
    /// and the renamed asset file are rolled back to their original state.
    pub fn rename_asset_with<F>(
        &self,
        problem_dir: &str,
        old_name: &str,
        new_name: &str,
        mut rewrite_md: F,
    ) -> Result<RenameAssetResult>
    where
        F: FnMut(&str, &str, &str) -> std::result::Result<(String, usize), String>,
    {
        if !is_existing_asset_name(old_name) {
            return Err(Error::Invalid {
                path: format!("{problem_dir}/assets/{old_name}"),
                message: format!("原素材文件名无效（{old_name}）"),
            });
        }
        if !is_safe_asset_name(new_name) {
            return Err(Error::Invalid {
                path: format!("{problem_dir}/assets/{new_name}"),
                message: format!("新素材文件名无效（{new_name}），包含非法字符或系统保留名"),
            });
        }
        let assets_rel = format!("{problem_dir}/assets");
        let old_ws_rel = format!("{assets_rel}/{old_name}");
        let new_ws_rel = format!("{assets_rel}/{new_name}");
        if old_name == new_name {
            return Ok(RenameAssetResult {
                old_ws_rel,
                new_ws_rel,
                updated_docs: Vec::new(),
            });
        }

        self.check_writable(&old_ws_rel)?;
        self.check_writable(&new_ws_rel)?;
        let assets_abs = self.resolve(&assets_rel)?;
        let old_abs = self.resolve(&old_ws_rel)?;
        let new_abs = self.resolve(&new_ws_rel)?;
        if old_abs.parent() != Some(assets_abs.as_path()) {
            return Err(Error::PathEscape(old_ws_rel));
        }
        if new_abs.parent() != Some(assets_abs.as_path()) {
            return Err(Error::PathEscape(new_ws_rel));
        }
        if !old_abs.is_file() {
            return Err(Error::NotFound(old_ws_rel));
        }
        if new_abs.symlink_metadata().is_ok() {
            return Err(Error::Invalid {
                path: new_ws_rel,
                message: format!("目标素材「{new_name}」已存在"),
            });
        }

        // Pre-read and compute all Markdown updates before mutating disk.
        let mut pending_writes: Vec<(String, Vec<u8>, String)> = Vec::new();
        for doc_name in ASSET_MARKDOWN_DOCS {
            let doc_rel = format!("{problem_dir}/{doc_name}");
            if let Ok(orig_text) = self.read_text(&doc_rel) {
                let (updated, count) =
                    rewrite_md(&orig_text, old_name, new_name).map_err(|reason| {
                        Error::Invalid {
                            path: doc_rel.clone(),
                            message: reason,
                        }
                    })?;
                if count > 0 && updated != orig_text {
                    self.check_writable(&doc_rel)?;
                    pending_writes.push((doc_rel, orig_text.into_bytes(), updated));
                }
            }
        }

        let moved_hash = fs::read(&old_abs).ok().map(|b| content_hash(&b));
        if let Some(tracker) = &self.self_writes {
            tracker.record(&old_ws_rel, SELF_DELETE_SENTINEL);
            if let Some(ref h) = moved_hash {
                tracker.record(&new_ws_rel, h);
            }
        }

        if let Err(e) = fs::rename(&old_abs, &new_abs) {
            if let Some(tracker) = &self.self_writes {
                tracker.unrecord(&old_ws_rel);
                tracker.unrecord(&new_ws_rel);
            }
            return Err(Error::Io {
                path: old_ws_rel,
                source: e,
            });
        }

        let mut written: Vec<(String, Vec<u8>)> = Vec::with_capacity(pending_writes.len());
        for (doc_rel, orig_bytes, updated) in pending_writes {
            if let Err(err) = self.write(&doc_rel, updated.as_bytes()) {
                // Roll back previously written docs and the renamed asset file.
                for (prev_rel, prev_bytes) in written.into_iter().rev() {
                    let _ = self.write(&prev_rel, &prev_bytes);
                }
                if let Some(tracker) = &self.self_writes {
                    tracker.record(&new_ws_rel, SELF_DELETE_SENTINEL);
                    if let Some(ref h) = moved_hash {
                        tracker.record(&old_ws_rel, h);
                    }
                }
                let _ = fs::rename(&new_abs, &old_abs);
                return Err(err);
            }
            written.push((doc_rel, orig_bytes));
        }

        let updated_docs = written.into_iter().map(|(rel, _)| rel).collect();
        Ok(RenameAssetResult {
            old_ws_rel,
            new_ws_rel,
            updated_docs,
        })
    }

    /// Deletes `<problem_dir>/assets/<name>` while enforcing workspace sandbox,
    /// read-only mode, and frozen-contest guards. Returns the workspace-relative path
    /// of the deleted file.
    pub fn delete_asset(&self, problem_dir: &str, name: &str) -> Result<String> {
        if !is_existing_asset_name(name) {
            return Err(Error::Invalid {
                path: format!("{problem_dir}/assets/{name}"),
                message: format!("素材文件名无效（{name}）"),
            });
        }
        let assets_rel = format!("{problem_dir}/assets");
        let ws_rel = format!("{assets_rel}/{name}");
        self.check_writable(&ws_rel)?;
        let assets_abs = self.resolve(&assets_rel)?;
        let abs = self.resolve(&ws_rel)?;
        if abs.parent() != Some(assets_abs.as_path()) {
            return Err(Error::PathEscape(ws_rel));
        }
        if !abs.is_file() {
            return Err(Error::NotFound(ws_rel));
        }
        if let Some(tracker) = &self.self_writes {
            tracker.record(&ws_rel, SELF_DELETE_SENTINEL);
        }
        if let Err(e) = fs::remove_file(&abs) {
            if let Some(tracker) = &self.self_writes {
                tracker.unrecord(&ws_rel);
            }
            return Err(Error::Io {
                path: ws_rel,
                source: e,
            });
        }
        Ok(ws_rel)
    }

    /// Removes all assets in `<problem_dir>/assets/` that have zero references in
    /// `statement*.md` or `editorial*.md`.
    ///
    /// Attempts each deletion independently and collects both successes and failures
    /// instead of aborting halfway on an individual error.
    pub fn clean_unused_assets_with<F>(
        &self,
        problem_dir: &str,
        scan_refs: F,
    ) -> Result<CleanUnusedResult>
    where
        F: FnMut(&str) -> Vec<ScannedAssetRef>,
    {
        self.check_writable(&format!("{problem_dir}/assets/_probe"))?;
        let assets = self.list_assets_with(problem_dir, scan_refs)?;
        let mut removed = Vec::new();
        let mut removed_paths = Vec::new();
        let mut freed_bytes = 0u64;
        let mut failed = Vec::new();
        for asset in assets {
            if asset.references.is_empty() {
                match self.delete_asset(problem_dir, &asset.name) {
                    Ok(ws_rel) => {
                        removed.push(asset.name);
                        removed_paths.push(ws_rel);
                        freed_bytes = freed_bytes.saturating_add(asset.bytes);
                    }
                    Err(e) => {
                        failed.push(CleanUnusedFailure {
                            name: asset.name,
                            message: e.human_message(),
                        });
                    }
                }
            }
        }
        Ok(CleanUnusedResult {
            removed,
            removed_paths,
            freed_bytes,
            failed,
        })
    }
}
