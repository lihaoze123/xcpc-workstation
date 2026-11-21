//! Span-accurate Markdown `assets/` reference parsing, rewriting, and width-attribute
//! updates powered by `pulldown-cmark`, plus convenience wrappers over `xcpc_storage`.

use std::collections::BTreeMap;
use std::ops::Range;

use pulldown_cmark::{CodeBlockKind, Event, LinkType, Parser, Tag, TagEnd};
use xcpc_domain::ASSET_MARKDOWN_DOCS;
use xcpc_storage::{
    AssetEntry, CleanUnusedResult, RenameAssetResult, Result as StorageResult, ScannedAssetRef,
    Workspace,
};

use crate::line_column;
use crate::markdown::{options, parse_image_width_attr};

/// One `assets/<name>` image reference found in a Markdown document.
#[derive(Debug, Clone, PartialEq)]
pub struct AssetReference {
    /// Decoded filename inside `assets/` (e.g. `"fig.png"` or `"my pic.png"`).
    pub asset_name: String,
    /// 1-based line number of the image reference.
    pub line: u32,
    /// 1-based column number (Unicode scalar values) of the image reference.
    pub col: u32,
    /// Byte start offset of the image syntax (`![...](...)`) in `markdown`.
    pub start_byte: usize,
    /// Byte end offset of the image syntax in `markdown` (before any trailing `{width=...}`).
    pub end_byte: usize,
    /// Optional width percentage parsed from a trailing `{width=NN%}` attribute.
    pub width_pct: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OccurrenceKind {
    MarkdownImage,
    TypstCode,
}

struct AssetOccurrence {
    reference: AssetReference,
    kind: OccurrenceKind,
    /// Byte range of the URL destination token (inline `![alt](dest)` or reference
    /// definition `[r]: dest`), including `<...>` if angle-bracketed.
    dest_span: Option<Range<usize>>,
    /// Whether the destination used `<...>` syntax.
    angle_bracketed: bool,
    /// Byte range of a trailing `{width=NN%}` attribute immediately after `end_byte`.
    width_span: Option<Range<usize>>,
}

/// Finds every `assets/<name>` image reference in `markdown` using `pulldown-cmark` spans,
/// plus conservative detection of `assets/<name>` literals inside ```` ```typst ```` blocks.
///
/// Correctly decodes percent-encoded URLs (`assets/my%20pic.png`) and angle-bracketed
/// destinations (`<assets/my pic.png>`), normalizes leading `./`, and ignores non-`assets/`
/// paths such as `other/assets/a.png` or nested subdirectories `assets/sub/a.png`.
pub fn find_asset_references(markdown: &str) -> Vec<AssetReference> {
    find_asset_occurrences(markdown)
        .into_iter()
        .map(|occ| occ.reference)
        .collect()
}

/// Rewrites every `assets/<old_name>` image destination (inline or reference definition) in
/// `markdown` to `assets/<new_name>`, returning `Ok((updated_markdown, replacement_count))`.
///
/// Fails with a descriptive error if any reference to `old_name` cannot be safely rewritten
/// (for example, inside a ```` ```typst ```` block or an unlocatable span), so callers can
/// abort a rename before touching disk and never leave broken links.
pub fn try_rewrite_asset_reference(
    markdown: &str,
    old_name: &str,
    new_name: &str,
) -> Result<(String, usize), String> {
    if old_name == new_name {
        return Ok((markdown.to_owned(), 0));
    }
    let occurrences = find_asset_occurrences(markdown);
    let mut edits: BTreeMap<usize, (Range<usize>, String)> = BTreeMap::new();
    for occ in occurrences {
        if occ.reference.asset_name != old_name {
            continue;
        }
        let Some(dest_span) = occ.dest_span else {
            return Err(match occ.kind {
                OccurrenceKind::TypstCode => format!(
                    "第 {} 行的 Typst 代码块中引用了 assets/{old_name}，无法自动改写，请先手动修改后再重命名",
                    occ.reference.line
                ),
                OccurrenceKind::MarkdownImage => format!(
                    "第 {} 行对 assets/{old_name} 的引用无法精确定位，已取消重命名以防断链",
                    occ.reference.line
                ),
            });
        };
        let replacement = format_dest_replacement(new_name, occ.angle_bracketed);
        edits
            .entry(dest_span.start)
            .or_insert((dest_span, replacement));
    }
    Ok(apply_edits(markdown, edits))
}

/// Rewrites every rewritable `assets/<old_name>` image destination (inline or reference
/// definition) in `markdown` to `assets/<new_name>`, returning `(updated_markdown, count)`.
///
/// Matches exact asset filenames only (so `assets/a.png.bak` and `other/assets/a.png` are
/// never touched), and automatically wraps `new_name` in `<...>` if it contains whitespace
/// or parentheses (or if the original destination used `<...>`).
pub fn rewrite_asset_reference(markdown: &str, old_name: &str, new_name: &str) -> (String, usize) {
    if old_name == new_name {
        return (markdown.to_owned(), 0);
    }
    let occurrences = find_asset_occurrences(markdown);
    let mut edits: BTreeMap<usize, (Range<usize>, String)> = BTreeMap::new();
    for occ in occurrences {
        if occ.reference.asset_name != old_name {
            continue;
        }
        let Some(dest_span) = occ.dest_span else {
            continue;
        };
        let replacement = format_dest_replacement(new_name, occ.angle_bracketed);
        edits
            .entry(dest_span.start)
            .or_insert((dest_span, replacement));
    }
    apply_edits(markdown, edits)
}

fn format_dest_replacement(new_name: &str, was_angle_bracketed: bool) -> String {
    let needs_angles = was_angle_bracketed
        || new_name
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '(' | ')' | '<' | '>'));
    if needs_angles {
        format!("<assets/{new_name}>")
    } else {
        format!("assets/{new_name}")
    }
}

fn apply_edits(markdown: &str, edits: BTreeMap<usize, (Range<usize>, String)>) -> (String, usize) {
    if edits.is_empty() {
        return (markdown.to_owned(), 0);
    }
    let count = edits.len();
    let mut out = markdown.to_owned();
    for (_, (range, repl)) in edits.into_iter().rev() {
        out.replace_range(range, &repl);
    }
    (out, count)
}

/// Sets or clears the trailing `{width=NN%}` attribute on every image reference to
/// `asset_name` in `markdown`.
///
/// - When `width_pct` is `Some(pct)` (must be finite and in `10.0..=100.0`), replaces an
///   existing `{width=...%}` (including decimals like `{width=60.5%}`) or inserts one right
///   after the image syntax, never appending duplicates.
/// - When `width_pct` is `None`, removes any existing `{width=...%}` attribute.
/// - Returns `(updated_markdown, updated_count)`.
pub fn set_asset_width_in_markdown(
    markdown: &str,
    asset_name: &str,
    width_pct: Option<f64>,
) -> (String, usize) {
    let formatted = match width_pct {
        Some(pct) => {
            if !pct.is_finite() || !(10.0..=100.0).contains(&pct) {
                return (markdown.to_owned(), 0);
            }
            Some(format!("{{width={}%}}", format_width_number(pct)))
        }
        None => None,
    };

    let occurrences = find_asset_occurrences(markdown);
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    for occ in occurrences {
        if occ.kind != OccurrenceKind::MarkdownImage || occ.reference.asset_name != asset_name {
            continue;
        }
        match (&formatted, occ.width_span) {
            (Some(attr), Some(span)) => {
                if &markdown[span.clone()] != attr.as_str() {
                    edits.push((span, attr.clone()));
                }
            }
            (Some(attr), None) => {
                edits.push((occ.reference.end_byte..occ.reference.end_byte, attr.clone()));
            }
            (None, Some(span)) => {
                edits.push((span, String::new()));
            }
            (None, None) => {}
        }
    }
    if edits.is_empty() {
        return (markdown.to_owned(), 0);
    }
    edits.sort_by_key(|b| std::cmp::Reverse(b.0.start));
    let mut out = markdown.to_owned();
    let count = edits.len();
    for (range, repl) in edits {
        out.replace_range(range, &repl);
    }
    (out, count)
}

/// Lists all assets in `<problem_dir>/assets/` with dimensions and exact Markdown references.
pub fn list_problem_assets(ws: &Workspace, problem_dir: &str) -> StorageResult<Vec<AssetEntry>> {
    ws.list_assets_with(problem_dir, scan_for_storage)
}

/// Renames `<problem_dir>/assets/<old_name>` to `<new_name>` and updates Markdown references
/// with full rollback on failure. Refuses to rename if any reference cannot be safely rewritten.
pub fn rename_problem_asset(
    ws: &Workspace,
    problem_dir: &str,
    old_name: &str,
    new_name: &str,
) -> StorageResult<RenameAssetResult> {
    ws.rename_asset_with(problem_dir, old_name, new_name, try_rewrite_asset_reference)
}

/// Deletes `<problem_dir>/assets/<name>`, returning its workspace-relative path.
pub fn delete_problem_asset(
    ws: &Workspace,
    problem_dir: &str,
    name: &str,
) -> StorageResult<String> {
    ws.delete_asset(problem_dir, name)
}

/// Removes all unreferenced assets in `<problem_dir>/assets/`.
pub fn clean_unused_problem_assets(
    ws: &Workspace,
    problem_dir: &str,
) -> StorageResult<CleanUnusedResult> {
    ws.clean_unused_assets_with(problem_dir, scan_for_storage)
}

/// Updates `{width=NN%}` for `asset_name` in `doc_rel` (or across all `statement*.md` and
/// `editorial.md` files in `problem_dir` when `doc_rel` is `None`), returning the list of
/// workspace-relative Markdown paths written.
pub fn set_problem_asset_width(
    ws: &Workspace,
    problem_dir: &str,
    asset_name: &str,
    width_pct: Option<f64>,
    doc_rel: Option<&str>,
) -> StorageResult<Vec<String>> {
    let docs: Vec<String> = match doc_rel {
        Some(path) => vec![path.to_owned()],
        None => ASSET_MARKDOWN_DOCS
            .iter()
            .map(|name| format!("{problem_dir}/{name}"))
            .collect(),
    };
    let mut updated_docs = Vec::new();
    for rel in docs {
        let Ok(orig) = ws.read_text(&rel) else {
            continue;
        };
        let (updated, count) = set_asset_width_in_markdown(&orig, asset_name, width_pct);
        if count > 0 && updated != orig {
            ws.write(&rel, updated.as_bytes())?;
            updated_docs.push(rel);
        }
    }
    Ok(updated_docs)
}

fn scan_for_storage(markdown: &str) -> Vec<ScannedAssetRef> {
    find_asset_references(markdown)
        .into_iter()
        .map(|r| ScannedAssetRef {
            asset_name: r.asset_name,
            line: r.line,
            width_pct: r.width_pct,
        })
        .collect()
}

fn find_asset_occurrences(markdown: &str) -> Vec<AssetOccurrence> {
    struct OpenImage {
        dest_url: String,
        link_type: LinkType,
        ref_id: String,
        start_byte: usize,
        alt_end: usize,
    }

    let mut stack: Vec<OpenImage> = Vec::new();
    let mut out: Vec<AssetOccurrence> = Vec::new();
    let mut pending_ref_resolutions: Vec<(usize, String)> = Vec::new();
    let mut in_typst_block = false;

    let mut iter = Parser::new_ext(markdown, options()).into_offset_iter();
    for (event, range) in &mut iter {
        match event {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) => {
                in_typst_block = info
                    .split_whitespace()
                    .next()
                    .is_some_and(|l| l.eq_ignore_ascii_case("typst"));
            }
            Event::End(TagEnd::CodeBlock) => {
                in_typst_block = false;
            }
            Event::Text(_) if in_typst_block => {
                scan_typst_assets(markdown, range.start, range.end, &mut out);
            }
            Event::Start(Tag::Image {
                link_type,
                dest_url,
                id,
                ..
            }) => {
                stack.push(OpenImage {
                    dest_url: dest_url.into_string(),
                    link_type,
                    ref_id: id.into_string(),
                    start_byte: range.start,
                    alt_end: range.start.saturating_add(2),
                });
            }
            Event::End(TagEnd::Image) => {
                let Some(open) = stack.pop() else {
                    continue;
                };
                let Some(asset_name) = extract_asset_name(&open.dest_url) else {
                    continue;
                };
                let (line, col) = line_column(markdown, open.start_byte);
                let (width_pct, width_span) = match parse_image_width_attr(&markdown[range.end..]) {
                    Some((num_str, consumed)) => (
                        num_str.parse::<f64>().ok(),
                        Some(range.end..range.end + consumed),
                    ),
                    None => (None, None),
                };
                let (dest_span, angle_bracketed) = if open.link_type == LinkType::Inline {
                    match locate_inline_dest_span(markdown, open.alt_end, range.end) {
                        Some((span, angles)) => (Some(span), angles),
                        None => (None, false),
                    }
                } else {
                    if matches!(
                        open.link_type,
                        LinkType::Reference | LinkType::Collapsed | LinkType::Shortcut
                    ) {
                        pending_ref_resolutions.push((out.len(), open.ref_id));
                    }
                    (None, false)
                };
                out.push(AssetOccurrence {
                    reference: AssetReference {
                        asset_name,
                        line: line as u32,
                        col: col as u32,
                        start_byte: open.start_byte,
                        end_byte: range.end,
                        width_pct,
                    },
                    kind: OccurrenceKind::MarkdownImage,
                    dest_span,
                    angle_bracketed,
                    width_span,
                });
            }
            _ => {
                if let Some(top) = stack.last_mut() {
                    top.alt_end = top.alt_end.max(range.end);
                }
            }
        }
    }

    let refdefs = iter.reference_definitions();
    for (occ_idx, ref_id) in pending_ref_resolutions {
        if let Some(def) = refdefs.get(&ref_id)
            && let Some((span, angles)) = locate_refdef_dest_span(markdown, def.span.clone())
        {
            out[occ_idx].dest_span = Some(span);
            out[occ_idx].angle_bracketed = angles;
        }
    }

    out
}

/// Conservatively scans a ```` ```typst ```` code chunk for `assets/<name>` literals
/// (e.g. `image("assets/fig.png")`, `read("assets/data.txt")`), preferring false positives
/// over ever missing a live reference during unused-asset cleanup.
fn scan_typst_assets(markdown: &str, start: usize, end: usize, out: &mut Vec<AssetOccurrence>) {
    let chunk = &markdown[start..end];
    let bytes = chunk.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            let q_start = i;
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    i += 2;
                    continue;
                }
                if bytes[i] == b'"' || bytes[i] == b'\n' || bytes[i] == b'\r' {
                    break;
                }
                i += 1;
            }
            if i < bytes.len() && bytes[i] == b'"' {
                let inner = &chunk[q_start + 1..i];
                if let Some(asset_name) = extract_raw_asset_name(inner) {
                    let abs_start = start + q_start;
                    let abs_end = start + i + 1;
                    let (line, col) = line_column(markdown, abs_start);
                    out.push(AssetOccurrence {
                        reference: AssetReference {
                            asset_name,
                            line: line as u32,
                            col: col as u32,
                            start_byte: abs_start,
                            end_byte: abs_end,
                            width_pct: None,
                        },
                        kind: OccurrenceKind::TypstCode,
                        dest_span: None,
                        angle_bracketed: false,
                        width_span: None,
                    });
                }
                i += 1;
                continue;
            }
            continue;
        }

        // Conservative fallback for unquoted `assets/<name>` tokens inside Typst blocks.
        if bytes[i..].starts_with(b"assets/") {
            let prev_ok = i == 0
                || !matches!(
                    bytes[i - 1],
                    b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'-'
                );
            let is_rel_dot = i >= 2 && &bytes[i - 2..i] == b"./";
            if prev_ok || is_rel_dot {
                let tok_start = i;
                let mut j = i + "assets/".len();
                while j < bytes.len()
                    && !matches!(
                        bytes[j],
                        b' ' | b'\t'
                            | b'\n'
                            | b'\r'
                            | b'"'
                            | b'\''
                            | b')'
                            | b']'
                            | b'}'
                            | b','
                            | b';'
                    )
                {
                    j += 1;
                }
                if let Some(asset_name) = extract_raw_asset_name(&chunk[tok_start..j]) {
                    let abs_start = start + tok_start;
                    let abs_end = start + j;
                    let (line, col) = line_column(markdown, abs_start);
                    out.push(AssetOccurrence {
                        reference: AssetReference {
                            asset_name,
                            line: line as u32,
                            col: col as u32,
                            start_byte: abs_start,
                            end_byte: abs_end,
                            width_pct: None,
                        },
                        kind: OccurrenceKind::TypstCode,
                        dest_span: None,
                        angle_bracketed: false,
                        width_span: None,
                    });
                }
                i = j;
                continue;
            }
        }
        i += 1;
    }
}

/// Extracts the direct filename inside `assets/` from a raw (non-percent-encoded) path.
fn extract_raw_asset_name(raw: &str) -> Option<String> {
    let mut s = raw.trim();
    while let Some(rest) = s.strip_prefix("./") {
        s = rest;
    }
    let name = s.strip_prefix("assets/")?;
    if name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\\') {
        return None;
    }
    Some(name.to_owned())
}

/// Extracts the direct filename inside `assets/` from a Markdown image destination URL.
/// Returns `None` for external URLs, parent escapes, subdirectories, or non-`assets/` paths.
fn extract_asset_name(dest_url: &str) -> Option<String> {
    let decoded = percent_decode(dest_url.trim());
    extract_raw_asset_name(&decoded)
}

/// Locates the URL destination token inside a Markdown reference definition `[label]: dest ...`
/// spanning `def_span`. Returns `(byte_range, is_angle_bracketed)`.
fn locate_refdef_dest_span(markdown: &str, def_span: Range<usize>) -> Option<(Range<usize>, bool)> {
    let bytes = markdown.as_bytes();
    let end = def_span.end.min(bytes.len());
    let mut i = def_span.start.min(end);

    while i < end && bytes[i] == b' ' {
        i += 1;
    }
    if i >= end || bytes[i] != b'[' {
        return None;
    }
    i += 1;
    while i < end {
        if bytes[i] == b'\\' && i + 1 < end {
            i += 2;
            continue;
        }
        if bytes[i] == b']' {
            i += 1;
            break;
        }
        i += 1;
    }
    if i >= end || bytes[i] != b':' {
        return None;
    }
    i += 1;
    while i < end && matches!(bytes[i], b' ' | b'\t' | b'\n' | b'\r') {
        i += 1;
    }
    if i >= end {
        return None;
    }

    if bytes[i] == b'<' {
        let start = i;
        i += 1;
        while i < end {
            if bytes[i] == b'\\' && i + 1 < end {
                i += 2;
                continue;
            }
            if bytes[i] == b'>' {
                return Some((start..i + 1, true));
            }
            i += 1;
        }
        return None;
    }

    let start = i;
    while i < end {
        match bytes[i] {
            b'\\' if i + 1 < end => i += 2,
            b' ' | b'\t' | b'\n' | b'\r' => break,
            _ => i += 1,
        }
    }
    (i > start).then_some((start..i, false))
}

/// Locates the URL destination token inside an inline image `![alt](dest ...)` between
/// `alt_end` and `img_end`. Returns `(byte_range, is_angle_bracketed)`.
fn locate_inline_dest_span(
    markdown: &str,
    alt_end: usize,
    img_end: usize,
) -> Option<(Range<usize>, bool)> {
    let bytes = markdown.as_bytes();
    let search_start = alt_end.min(img_end);
    // Find `](` after the alt text.
    let mut pos = search_start;
    let open_paren = loop {
        if pos + 1 >= img_end {
            return None;
        }
        if bytes[pos] == b']' && bytes[pos + 1] == b'(' {
            break pos + 1;
        }
        pos += 1;
    };

    let mut i = open_paren + 1;
    while i < img_end && matches!(bytes[i], b' ' | b'\t' | b'\n' | b'\r') {
        i += 1;
    }
    if i >= img_end {
        return None;
    }

    if bytes[i] == b'<' {
        let start = i;
        i += 1;
        while i < img_end {
            if bytes[i] == b'\\' && i + 1 < img_end {
                i += 2;
                continue;
            }
            if bytes[i] == b'>' {
                return Some((start..i + 1, true));
            }
            i += 1;
        }
        return None;
    }

    let start = i;
    let mut paren_depth = 0usize;
    while i < img_end {
        match bytes[i] {
            b'\\' if i + 1 < img_end => {
                i += 2;
            }
            b' ' | b'\t' | b'\n' | b'\r' => break,
            b'(' => {
                paren_depth += 1;
                i += 1;
            }
            b')' => {
                if paren_depth == 0 {
                    break;
                }
                paren_depth -= 1;
                i += 1;
            }
            _ => i += 1,
        }
    }
    (i > start).then_some((start..i, false))
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2]))
        {
            out.push((hi << 4) | lo);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| input.to_owned())
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn format_width_number(pct: f64) -> String {
    if (pct - pct.round()).abs() < 1e-9 {
        format!("{}", pct.round() as i64)
    } else {
        let s = format!("{pct:.2}");
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_asset_references_handles_encoding_angles_and_avoids_false_positives() {
        let md = "\
第一行 ![图1](assets/a.png){width=60.5%} 和 ![图2](assets/a.png.bak)
第二行 ![空格1](assets/my%20pic.png){width=50%}
第三行 ![空格2](<assets/my pic.png>)
第四行 ![其他](other/assets/a.png) ![子目录](assets/sub/a.png) ![相对](./assets/rel.svg)
";
        let refs = find_asset_references(md);
        assert_eq!(refs.len(), 5);

        assert_eq!(refs[0].asset_name, "a.png");
        assert_eq!(refs[0].line, 1);
        assert_eq!(refs[0].width_pct, Some(60.5));

        assert_eq!(refs[1].asset_name, "a.png.bak");
        assert_eq!(refs[1].line, 1);
        assert_eq!(refs[1].width_pct, None);

        assert_eq!(refs[2].asset_name, "my pic.png");
        assert_eq!(refs[2].line, 2);
        assert_eq!(refs[2].width_pct, Some(50.0));

        assert_eq!(refs[3].asset_name, "my pic.png");
        assert_eq!(refs[3].line, 3);
        assert_eq!(refs[3].width_pct, None);

        assert_eq!(refs[4].asset_name, "rel.svg");
        assert_eq!(refs[4].line, 4);
        assert_eq!(refs[4].width_pct, None);
    }

    #[test]
    fn rewrite_asset_reference_is_span_accurate() {
        let md = "![a](assets/a.png) ![bak](assets/a.png.bak) ![other](other/assets/a.png) ![enc](assets/my%20pic.png)";
        let (out1, count1) = rewrite_asset_reference(md, "a.png", "b.png");
        assert_eq!(count1, 1);
        assert_eq!(
            out1,
            "![a](assets/b.png) ![bak](assets/a.png.bak) ![other](other/assets/a.png) ![enc](assets/my%20pic.png)"
        );

        let (out2, count2) = rewrite_asset_reference(&out1, "my pic.png", "new pic.png");
        assert_eq!(count2, 1);
        assert_eq!(
            out2,
            "![a](assets/b.png) ![bak](assets/a.png.bak) ![other](other/assets/a.png) ![enc](<assets/new pic.png>)"
        );
    }

    #[test]
    fn set_asset_width_replaces_decimals_without_duplicating_and_clears() {
        let md = "![a](assets/a.png){width=60.5%} ![bak](assets/a.png.bak) ![b](assets/b.png)";
        let (out1, c1) = set_asset_width_in_markdown(md, "a.png", Some(75.0));
        assert_eq!(c1, 1);
        assert_eq!(
            out1,
            "![a](assets/a.png){width=75%} ![bak](assets/a.png.bak) ![b](assets/b.png)"
        );

        let (out2, c2) = set_asset_width_in_markdown(&out1, "b.png", Some(50.5));
        assert_eq!(c2, 1);
        assert_eq!(
            out2,
            "![a](assets/a.png){width=75%} ![bak](assets/a.png.bak) ![b](assets/b.png){width=50.5%}"
        );

        let (out3, c3) = set_asset_width_in_markdown(&out2, "a.png", None);
        assert_eq!(c3, 1);
        assert_eq!(
            out3,
            "![a](assets/a.png) ![bak](assets/a.png.bak) ![b](assets/b.png){width=50.5%}"
        );
    }

    #[test]
    fn find_asset_references_scans_typst_blocks_conservatively_and_ignores_other_code() {
        let md = "\
![md](assets/md.png)

```typst
// 中文注释：示例示意图与多字节字符（如“图一”）不能触发 UTF-8 切片 panic
#align(center, image(\"assets/typst.png\", width: 50%))
#let data = read(\"./assets/my data.txt\")
#let zh = image(\"assets/示意图.svg\")
// 未加引号的回退识别：assets/bare_中文.png
```

```python
# Non-typst code blocks are not scanned as Typst asset references
path = \"assets/ignored.png\"
```
";
        let refs = find_asset_references(md);
        let names: Vec<&str> = refs.iter().map(|r| r.asset_name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "md.png",
                "typst.png",
                "my data.txt",
                "示意图.svg",
                "bare_中文.png"
            ]
        );
    }

    #[test]
    fn try_rewrite_handles_reference_style_images_and_rejects_typst_blocks() {
        let md = "\
![first][r] and ![second][r] and ![collapsed][] and ![short]

[r]: assets/refstyle.png \"Title\"
[collapsed]: <assets/my pic.png>
[short]: assets/short.png

```typst
#image(\"assets/typst.png\")
```
";
        // Reference-style with shared definition: rewrites `[r]: ...` once without duplicating.
        let (out1, c1) = try_rewrite_asset_reference(md, "refstyle.png", "renamed.png").unwrap();
        assert_eq!(c1, 1);
        assert!(out1.contains("[r]: assets/renamed.png \"Title\""));
        assert!(!out1.contains("assets/refstyle.png"));

        // Collapsed reference with angle brackets and spaces.
        let (out2, c2) = try_rewrite_asset_reference(&out1, "my pic.png", "new pic.png").unwrap();
        assert_eq!(c2, 1);
        assert!(out2.contains("[collapsed]: <assets/new pic.png>"));

        // Shortcut reference.
        let (out3, c3) = try_rewrite_asset_reference(&out2, "short.png", "shorter.png").unwrap();
        assert_eq!(c3, 1);
        assert!(out3.contains("[short]: assets/shorter.png"));

        // Typst block reference cannot be safely rewritten -> returns Err.
        let err = try_rewrite_asset_reference(&out3, "typst.png", "typst2.png").unwrap_err();
        assert!(err.contains("Typst"), "unexpected error: {err}");
    }

    #[test]
    fn problem_asset_ops_protect_typst_refs_rewrite_refstyle_and_handle_colon_names() {
        let dir = tempfile::tempdir().unwrap();
        let ws = xcpc_storage::init_workspace(dir.path()).unwrap();
        ws.write("problems/p1/problem.toml", b"title = \"Assets Test\"\n")
            .unwrap();
        ws.write(
            "problems/p1/statement.md",
            "\
![md](assets/md.png)
![ref][r]

```typst
#image(\"assets/typst.png\")
```

[r]: assets/refstyle.png
"
            .as_bytes(),
        )
        .unwrap();

        let assets_dir = dir.path().join("problems/p1/assets");
        std::fs::create_dir_all(&assets_dir).unwrap();
        for name in [
            "a_unused.png",
            "b:colon.png",
            "a..b.png",
            "c_unused.png",
            "md.png",
            "refstyle.png",
            "typst.png",
        ] {
            std::fs::write(assets_dir.join(name), b"dummy").unwrap();
        }

        // 1. typst.png and refstyle.png must both be counted as referenced.
        let list = list_problem_assets(&ws, "problems/p1").unwrap();
        let refs_of = |name: &str| {
            list.iter()
                .find(|a| a.name == name)
                .unwrap()
                .references
                .len()
        };
        assert_eq!(refs_of("typst.png"), 1);
        assert_eq!(refs_of("refstyle.png"), 1);
        assert_eq!(refs_of("md.png"), 1);
        assert_eq!(refs_of("b:colon.png"), 0);

        // 2. Renaming typst.png is rejected before touching disk.
        assert!(rename_problem_asset(&ws, "problems/p1", "typst.png", "typst2.png").is_err());
        assert!(assets_dir.join("typst.png").is_file());
        assert!(!assets_dir.join("typst2.png").exists());

        // 3. Renaming reference-style image updates `[r]: ...` cleanly.
        let ren = rename_problem_asset(&ws, "problems/p1", "refstyle.png", "ref2.png").unwrap();
        assert_eq!(ren.updated_docs, vec!["problems/p1/statement.md"]);
        assert!(assets_dir.join("ref2.png").is_file());
        assert!(!assets_dir.join("refstyle.png").exists());
        let stmt = ws.read_text("problems/p1/statement.md").unwrap();
        assert!(stmt.contains("[r]: assets/ref2.png"));

        // 4. Externally placed `b:colon.png` can be renamed to a safe name.
        rename_problem_asset(&ws, "problems/p1", "b:colon.png", "b_clean.png").unwrap();
        assert!(assets_dir.join("b_clean.png").is_file());
        assert!(!assets_dir.join("b:colon.png").exists());

        // Put another colon-named unused file back to verify batch cleanup removes it too.
        std::fs::write(assets_dir.join("b:colon2.png"), b"dummy").unwrap();

        // 5. Batch cleanup removes all unused files (including `b:colon2.png` and `a..b.png`)
        //    and preserves `md.png`, `ref2.png`, and `typst.png`.
        let cleaned = clean_unused_problem_assets(&ws, "problems/p1").unwrap();
        assert!(cleaned.failed.is_empty());
        assert_eq!(
            cleaned.removed,
            vec![
                "a..b.png",
                "a_unused.png",
                "b:colon2.png",
                "b_clean.png",
                "c_unused.png"
            ]
        );
        assert!(assets_dir.join("md.png").is_file());
        assert!(assets_dir.join("ref2.png").is_file());
        assert!(assets_dir.join("typst.png").is_file());
    }
}
