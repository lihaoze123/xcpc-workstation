//! End-to-end tests for `Engine::locate_point` (preview coordinate -> `/main.typ` offset).

use std::ops::Range;

use typst::layout::{Abs, Frame, FrameItem, Point, Transform};
use xcpc_engine::{Compiled, Engine, FontOptions};

const CONTEST_ZH: &str = include_str!("contest-zh.typ");

#[derive(Debug, Clone)]
struct RenderedText {
    text: String,
    bounds: [f64; 4],
}

fn collect_texts(frame: &Frame, ts: Transform, out: &mut Vec<RenderedText>) {
    for (pos, item) in frame.items() {
        let at = ts.pre_concat(Transform::translate(pos.x, pos.y));
        match item {
            FrameItem::Group(group) => {
                collect_texts(&group.frame, at.pre_concat(group.transform), out);
            }
            FrameItem::Text(text) => {
                let size = text.size;
                let width: Abs = text.glyphs.iter().map(|g| g.x_advance.at(size)).sum();
                let metrics = text.font.metrics();
                let asc = metrics.ascender.at(size).to_pt();
                let desc = metrics.descender.at(size).to_pt();
                let local = [0.0, -asc, width.to_pt(), -desc];
                out.push(RenderedText {
                    text: text.text.to_string(),
                    bounds: transformed(local, at),
                });
            }
            _ => {}
        }
    }
}

fn collect_images(frame: &Frame, ts: Transform, out: &mut Vec<[f64; 4]>) {
    for (pos, item) in frame.items() {
        let at = ts.pre_concat(Transform::translate(pos.x, pos.y));
        match item {
            FrameItem::Group(group) => {
                collect_images(&group.frame, at.pre_concat(group.transform), out);
            }
            FrameItem::Image(_, size, _) => {
                out.push(transformed([0.0, 0.0, size.x.to_pt(), size.y.to_pt()], at));
            }
            _ => {}
        }
    }
}

fn collect_shapes(frame: &Frame, ts: Transform, out: &mut Vec<[f64; 4]>) {
    for (pos, item) in frame.items() {
        let at = ts.pre_concat(Transform::translate(pos.x, pos.y));
        match item {
            FrameItem::Group(group) => {
                collect_shapes(&group.frame, at.pre_concat(group.transform), out);
            }
            FrameItem::Shape(shape, _) => {
                let b = shape.bbox(true);
                out.push(transformed(
                    [
                        b.min.x.to_pt(),
                        b.min.y.to_pt(),
                        b.max.x.to_pt(),
                        b.max.y.to_pt(),
                    ],
                    at,
                ));
            }
            _ => {}
        }
    }
}

fn transformed(r: [f64; 4], ts: Transform) -> [f64; 4] {
    let mut out = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for (x, y) in [(r[0], r[1]), (r[2], r[1]), (r[0], r[3]), (r[2], r[3])] {
        let p = Point::new(Abs::pt(x), Abs::pt(y)).transform_inf(ts);
        out = [
            out[0].min(p.x.to_pt()),
            out[1].min(p.y.to_pt()),
            out[2].max(p.x.to_pt()),
            out[3].max(p.y.to_pt()),
        ];
    }
    out
}

fn center(b: [f64; 4]) -> (f64, f64) {
    (0.5 * (b[0] + b[2]), 0.5 * (b[1] + b[3]))
}

/// Finds the nth (0-based) rendered `TextItem` on `page` whose plain text contains `needle`.
fn find_text(compiled: &Compiled, page: usize, needle: &str, nth: usize) -> [f64; 4] {
    let mut texts = Vec::new();
    collect_texts(
        &compiled.document.pages()[page].frame,
        Transform::identity(),
        &mut texts,
    );
    texts
        .into_iter()
        .filter(|t| t.text.contains(needle))
        .nth(nth)
        .unwrap_or_else(|| panic!("text {needle:?} (#{nth}) not found on page {page}"))
        .bounds
}

/// Byte range of the entire line in `src` containing `needle`.
fn line_range(src: &str, needle: &str) -> Range<usize> {
    let pos = src
        .find(needle)
        .unwrap_or_else(|| panic!("needle {needle:?} not found in source"));
    let start = src[..pos].rfind('\n').map_or(0, |i| i + 1);
    let end = src[pos..].find('\n').map_or(src.len(), |i| pos + i);
    start..end
}

fn substr_range(src: &str, needle: &str) -> Range<usize> {
    let start = src
        .find(needle)
        .unwrap_or_else(|| panic!("needle {needle:?} not found in source"));
    start..start + needle.len()
}

#[test]
fn contest_zh_paragraphs_lists_formulas_and_margins() {
    let mut engine = Engine::new(
        &FontOptions::from_env(),
        Some(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))),
    );
    engine.set_main(CONTEST_ZH);
    let compiled = engine.compile().expect("contest-zh compiles");
    assert_eq!(compiled.page_count(), 5);

    // Page 0 (cover): template-only content must return None everywhere.
    let (cx, cy) = center(find_text(&compiled, 0, "试题册", 0));
    assert_eq!(engine.locate_point(&compiled, 0, cx, cy), None);
    let (cx, cy) = center(find_text(&compiled, 0, "A + B Problem", 0));
    assert_eq!(engine.locate_point(&compiled, 0, cx, cy), None);
    assert_eq!(engine.locate_point(&compiled, 0, 200.0, 200.0), None);

    // Out-of-bounds page and non-finite coordinates return None.
    assert_eq!(engine.locate_point(&compiled, 99, 100.0, 100.0), None);
    assert_eq!(engine.locate_point(&compiled, 1, f64::NAN, 100.0), None);
    assert_eq!(
        engine.locate_point(&compiled, 1, 100.0, f64::INFINITY),
        None
    );

    // Page 1 (Problem A): running header, footer, margins, and far blank area return None.
    let (hx, hy) = center(find_text(&compiled, 1, "工作站测试赛", 0));
    assert_eq!(engine.locate_point(&compiled, 1, hx, hy), None);
    let (fx, fy) = center(find_text(&compiled, 1, "Page 1 of 4", 0));
    assert_eq!(engine.locate_point(&compiled, 1, fx, fy), None);
    assert_eq!(engine.locate_point(&compiled, 1, 10.0, 200.0), None);
    assert_eq!(engine.locate_point(&compiled, 1, 580.0, 200.0), None);
    assert_eq!(engine.locate_point(&compiled, 1, 300.0, 620.0), None);

    // Page 1 (Problem A): prose paragraphs locate into their exact source lines.
    for (rendered, source_needle) in [
        ("给定两个整数", "给定两个整数"),
        ("可能为负数", "可能为负数"),
        ("一行两个整数", "一行两个整数"),
        ("输出一个整数", "输出一个整数"),
        ("在第一组样例中", "在第一组样例中"),
    ] {
        let (x, y) = center(find_text(&compiled, 1, rendered, 0));
        let off = engine
            .locate_point(&compiled, 1, x, y)
            .unwrap_or_else(|| panic!("expected hit for {rendered:?}"));
        assert!(CONTEST_ZH.is_char_boundary(off));
        let range = line_range(CONTEST_ZH, source_needle);
        assert!(
            range.contains(&off),
            "{rendered:?}: offset {off} not in {range:?} ({:?})",
            &CONTEST_ZH[range.clone()]
        );
    }

    // Character-level precision inside a single TextItem via glyph x_advance + span.1:
    // clicking early vs late inside "，计算它们的和。这是一道签到题，请注意" moves forward.
    let long_run = find_text(&compiled, 1, "这是一道签到题", 0);
    let run_y = 0.5 * (long_run[1] + long_run[3]);
    let early = engine
        .locate_point(&compiled, 1, long_run[0] + 15.0, run_y)
        .expect("early glyph");
    let late = engine
        .locate_point(&compiled, 1, long_run[2] - 15.0, run_y)
        .expect("late glyph");
    let p1_line = line_range(CONTEST_ZH, "这是一道签到题");
    assert!(p1_line.contains(&early) && p1_line.contains(&late));
    assert!(
        early < late,
        "expected glyph-level progression: early={early} late={late}"
    );

    // Page 1 inline math `#m(...)`: clicking on detached `⩽`, superscript `9`, and the
    // math operator space between `⩽` and `𝑎` inside `-10^9 <= a, b <= 10^9` falls back
    // to the same line in `/main.typ`.
    let input_line = line_range(CONTEST_ZH, "一行两个整数");
    let le_box = find_text(&compiled, 1, "⩽", 0);
    let (mx, my) = center(le_box);
    let off_le = engine
        .locate_point(&compiled, 1, mx, my)
        .expect("inline math <= should locate");
    assert!(
        input_line.contains(&off_le),
        "inline math <= offset {off_le} not in input line {input_line:?}"
    );
    let math_a_box = find_text(&compiled, 1, "𝑎", 2);
    let gap_x = 0.5 * (le_box[2] + math_a_box[0]);
    let off_math_gap = engine
        .locate_point(&compiled, 1, gap_x, my)
        .expect("inline math operator gap should locate");
    assert!(
        input_line.contains(&off_math_gap),
        "inline math gap offset {off_math_gap} not in input line {input_line:?}"
    );
    let (sx, sy) = center(find_text(&compiled, 1, "9", 0));
    let off_sup = engine
        .locate_point(&compiled, 1, sx, sy)
        .expect("inline math superscript 9 should locate");
    assert!(
        input_line.contains(&off_sup),
        "inline math superscript offset {off_sup} not in input line {input_line:?}"
    );

    // Page 2 (Problem B): wrapped line 2 of paragraph 1, list items, inline raw, and display math.
    let p2_desc_line = line_range(CONTEST_ZH, "这是一道交互题");
    let (wx, wy) = center(find_text(&compiled, 2, "询问找出它", 0));
    let off_wrap = engine
        .locate_point(&compiled, 2, wx, wy)
        .expect("wrapped line");
    assert!(p2_desc_line.contains(&off_wrap));
    assert!(
        CONTEST_ZH[off_wrap..].starts_with("询问")
            || CONTEST_ZH[off_wrap..].starts_with("问")
            || CONTEST_ZH[off_wrap..].starts_with("找")
            || CONTEST_ZH[off_wrap..].starts_with("出")
            || CONTEST_ZH[off_wrap..].starts_with("它"),
        "wrapped line offset pointed at unexpected slice: {:?}",
        &CONTEST_ZH[off_wrap..off_wrap.saturating_add(18).min(CONTEST_ZH.len())]
    );

    // Detached `log` inside inline math on Problem B line 1 locates to the same line.
    let (lx, ly) = center(find_text(&compiled, 2, "log", 0));
    let off_log = engine
        .locate_point(&compiled, 2, lx, ly)
        .expect("inline math log");
    assert!(p2_desc_line.contains(&off_log));

    // Distinct list items inside `#list([若 #m("x < y")...], [若 #m("x > y")...], [若 #m("x = y")...])`:
    let item1 = substr_range(CONTEST_ZH, r#"[若 #m("x < y");，返回 #raw("<");；]"#);
    let item2 = substr_range(CONTEST_ZH, r#"[若 #m("x > y");，返回 #raw(">");；]"#);
    let item3 = substr_range(
        CONTEST_ZH,
        r#"[若 #m("x = y");，返回 #raw("=");，此时你需要立即结束程序。]"#,
    );
    for (idx, expected) in [(0, item1), (1, item2), (2, item3)] {
        let (x, y) = center(find_text(&compiled, 2, "，返回", idx));
        let off = engine
            .locate_point(&compiled, 2, x, y)
            .unwrap_or_else(|| panic!("list item {idx}"));
        assert!(
            expected.contains(&off),
            "list item {idx}: offset {off} not in {expected:?}"
        );
    }

    // Standalone display math `#dm("q <= lr(⌈ log _(2 ) n ⌉ ) + 1 .")` on page 2:
    // clicking `𝑞` or detached `log` inside the centered display equation maps to line 32.
    let dm_line = line_range(CONTEST_ZH, "询问次数的上界满足");
    let (dx, dy) = center(find_text(&compiled, 2, "𝑞", 0));
    let off_dm_q = engine
        .locate_point(&compiled, 2, dx, dy)
        .expect("display math q");
    assert!(
        dm_line.contains(&off_dm_q),
        "display math q offset {off_dm_q} not in {dm_line:?}"
    );
    let (dlx, dly) = center(find_text(&compiled, 2, "log", 1));
    let off_dm_log = engine
        .locate_point(&compiled, 2, dlx, dly)
        .expect("display math log");
    assert!(
        dm_line.contains(&off_dm_log),
        "display math log offset {off_dm_log} not in {dm_line:?}"
    );

    // Inline `#raw("? y")` on page 2.
    let raw_line = line_range(CONTEST_ZH, r#"#raw("? y")"#);
    let (rx, ry) = center(find_text(&compiled, 2, "? y", 0));
    let off_raw = engine
        .locate_point(&compiled, 2, rx, ry)
        .expect("inline raw");
    assert!(raw_line.contains(&off_raw));

    // Pages 3 & 4 (Problem C): prose on page 3 and note after long samples on page 4.
    let c_desc = line_range(CONTEST_ZH, "上的排布。");
    let (cx, cy) = center(find_text(&compiled, 3, "上的排布。", 0));
    let off_c_wrap = engine
        .locate_point(&compiled, 3, cx, cy)
        .expect("Problem C wrapped text");
    assert!(c_desc.contains(&off_c_wrap));

    let c_note = line_range(CONTEST_ZH, "注意答案可能超过");
    let (nx, ny) = center(find_text(&compiled, 4, "位整数。", 0));
    let off_c_note = engine
        .locate_point(&compiled, 4, nx, ny)
        .expect("Problem C page 4 note");
    assert!(c_note.contains(&off_c_note));

    // Inline math `2^{31}-1` on page 4 note.
    let (mx, my) = center(find_text(&compiled, 4, "−", 0));
    let off_c_math = engine
        .locate_point(&compiled, 4, mx, my)
        .expect("Problem C page 4 inline math");
    assert!(c_note.contains(&off_c_math));

    // Template-generated sample cells on page 4 are far from body text -> None.
    let (tx, ty) = center(find_text(&compiled, 4, "1 37", 0));
    assert_eq!(engine.locate_point(&compiled, 4, tx, ty), None);
}

#[test]
fn continuation_page_header_never_leaks_into_first_body_line() {
    let mut body = String::new();
    for i in 1..=55 {
        body.push_str(&format!(
            "第 {i} 段测试正文内容，用于撑满第一页并自然换页到第二页顶部的首行位置。\n\n"
        ));
    }
    let src = format!(
        r#"#import "/templates/olymp-classic.typ": *
#show: contest.with(name: "延续页页眉测试", date: "2026", language: "chinese", show-cover: false, blank-pages: false)
#problem(label: "A", title: "长题面", time-limit: "1 秒", memory-limit: "256 MB")[
{body}]
"#
    );
    let mut engine = Engine::new(
        &FontOptions::from_env(),
        Some(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))),
    );
    engine.set_main(&src);
    let compiled = engine.compile().expect("long statement compiles");
    assert!(compiled.page_count() >= 2, "expected at least 2 pages");

    // On page 1 (second physical page, continuation page), body text starts right at text-top.
    let mut texts = Vec::new();
    collect_texts(
        &compiled.document.pages()[1].frame,
        Transform::identity(),
        &mut texts,
    );
    let first_body = texts
        .iter()
        .find(|t| t.text.contains("第") && t.bounds[1] > 60.0)
        .expect("body text on continuation page");
    let (bx, by) = center(first_body.bounds);
    let off = engine
        .locate_point(&compiled, 1, bx, by)
        .expect("continuation page body line locates");
    assert!(src[off..].contains("段测试正文") || src[..off + 12].contains("第"));

    // Clicking the running header text or header rule right above page 2's first line returns None.
    let (hx, hy) = center(find_text(&compiled, 1, "延续页页眉测试", 0));
    assert_eq!(engine.locate_point(&compiled, 1, hx, hy), None);

    let mut shapes = Vec::new();
    collect_shapes(
        &compiled.document.pages()[1].frame,
        Transform::identity(),
        &mut shapes,
    );
    let header_rule = shapes
        .iter()
        .copied()
        .find(|b| b[1] > 45.0 && b[1] < 60.0)
        .expect("header rule on continuation page");
    let (rx, ry) = center(header_rule);
    assert_eq!(
        engine.locate_point(&compiled, 1, rx, ry),
        None,
        "clicking header rule must not leak into continuation body line"
    );
}

#[test]
fn proximity_prefers_same_row_and_rejects_far_whitespace() {
    let src = r#"#import "/templates/olymp-classic.typ": *
#show: contest.with(name: "近邻测试", language: "chinese", show-cover: false, blank-pages: false)
#problem(label: "A", title: "近邻", time-limit: "1 秒", memory-limit: "256 MB")[
第一行非常长用于延伸到右侧很远的地方，以便验证第二行右侧边缘的点击优先命中第二行。

短行二。

第三行正文。
]
"#;
    let mut engine = Engine::new(
        &FontOptions::from_env(),
        Some(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))),
    );
    engine.set_main(src);
    let compiled = engine.compile().expect("compiles");

    let line1 = find_text(&compiled, 0, "第一行非常长", 0);
    let line2 = find_text(&compiled, 0, "短行二。", 0);
    let line3 = find_text(&compiled, 0, "第三行正文。", 0);

    // Click 10pt to the right of short line 2 (within 16pt, same row as line 2, while
    // vertically also near line 1 above): must hit line 2, not line 1.
    let off_edge = engine
        .locate_point(&compiled, 0, line2[2] + 10.0, 0.5 * (line2[1] + line2[3]))
        .expect("near right edge of line 2");
    assert!(
        line_range(src, "短行二。").contains(&off_edge),
        "expected line 2, got offset {off_edge} ({:?})",
        &src[off_edge..off_edge.saturating_add(24).min(src.len())]
    );

    // Click in the inter-line gap between line 2 and line 3 (closer to line 3).
    let gap_y = line3[1] - 1.5;
    let off_gap = engine
        .locate_point(&compiled, 0, line3[0] + 12.0, gap_y)
        .expect("inter-line gap near line 3");
    assert!(line_range(src, "第三行正文。").contains(&off_gap));

    // Click 30pt to the right of short line 2 and 30pt below line 1 -> outside 16pt radius -> None.
    assert!(line1[2] > line2[2] + 60.0);
    assert_eq!(
        engine.locate_point(&compiled, 0, line2[2] + 60.0, line3[3] + 40.0),
        None
    );
}

#[test]
fn images_shapes_tables_editorial_and_transforms() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(dir.path().join("templates")).expect("templates dir");
    std::fs::write(
        dir.path().join("templates/olymp-classic.typ"),
        xcpc_engine::preset_source("olymp-classic").expect("olymp-classic preset"),
    )
    .expect("write olymp-classic");
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="40"><rect width="80" height="40" fill="black"/></svg>"#;
    std::fs::write(dir.path().join("fig.svg"), svg).expect("write svg");

    let src = r#"#import "/templates/olymp-classic.typ": *
#import "/xcpc/markdown.typ": m, dm

#show: contest.with(name: "元素测试", language: "chinese", show-cover: false, blank-pages: false, book: "editorial")

#tutorial(label: "A", title: "综合题解", meta: ("dp",))[
#subsection[配图与图形]

配图说明：#box(image("/fig.svg", width: 80pt));

#rect(fill: luma(92%), inset: 8pt)[方框内文字]

#table(columns: 2, [左格内容], [右格内容])

#raw(block: true, lang: "cpp", "int solve() { return 42; }")

#move(dx: 24pt, dy: 6pt)[平移后文字]

#rotate(90deg, reflow: true)[旋转文字]

#scale(x: 150%, y: 150%, reflow: true)[缩放文字]

复杂公式前一段：

#dm("sum _(i = 1 )^(n ) frac(a _(i ), b _(i )) <= 1 0 ^(9 )")

复杂公式后一段。

#line(length: 100%)
]
"#;
    let mut engine = Engine::new(&FontOptions::from_env(), Some(dir.path().to_path_buf()));
    engine.set_main(src);
    let compiled = engine.compile().expect("editorial compiles");

    // 1. Image hit locates to `image("/fig.svg", ...)`.
    let mut images = Vec::new();
    collect_images(
        &compiled.document.pages()[0].frame,
        Transform::identity(),
        &mut images,
    );
    assert_eq!(images.len(), 1);
    let (ix, iy) = center(images[0]);
    let off_img = engine
        .locate_point(&compiled, 0, ix, iy)
        .expect("image hit");
    assert!(
        line_range(src, r#"image("/fig.svg""#).contains(&off_img),
        "image offset {off_img} not on image line"
    );

    // 2. Text inside a filled `#rect(...)` prefers inner text over enclosing rect shape.
    let (rx, ry) = center(find_text(&compiled, 0, "方框内文字", 0));
    let off_rect_text = engine
        .locate_point(&compiled, 0, rx, ry)
        .expect("text inside rect");
    assert!(
        substr_range(src, "方框内文字").contains(&off_rect_text),
        "expected inner text span, got offset {off_rect_text} ({:?})",
        &src[off_rect_text..off_rect_text.saturating_add(24).min(src.len())]
    );

    // 3. Table cell text hits each cell accurately.
    let (tx1, ty1) = center(find_text(&compiled, 0, "左格内容", 0));
    let (tx2, ty2) = center(find_text(&compiled, 0, "右格内容", 0));
    assert!(
        substr_range(src, "左格内容")
            .contains(&engine.locate_point(&compiled, 0, tx1, ty1).unwrap())
    );
    assert!(
        substr_range(src, "右格内容")
            .contains(&engine.locate_point(&compiled, 0, tx2, ty2).unwrap())
    );

    // 4. Subsection heading, code block, and `#move(...)` transformed text all locate cleanly.
    let (sx, sy) = center(find_text(&compiled, 0, "配图与图形", 0));
    assert!(
        line_range(src, "配图与图形").contains(&engine.locate_point(&compiled, 0, sx, sy).unwrap())
    );

    let (cx, cy) = center(find_text(&compiled, 0, "solve", 0));
    assert!(
        line_range(src, "int solve()")
            .contains(&engine.locate_point(&compiled, 0, cx, cy).unwrap())
    );

    let (mx, my) = center(find_text(&compiled, 0, "平移后文字", 0));
    assert!(
        line_range(src, "平移后文字").contains(&engine.locate_point(&compiled, 0, mx, my).unwrap())
    );

    let (rotx, roty) = center(find_text(&compiled, 0, "旋转文字", 0));
    assert!(
        line_range(src, "旋转文字")
            .contains(&engine.locate_point(&compiled, 0, rotx, roty).unwrap())
    );

    let (scx, scy) = center(find_text(&compiled, 0, "缩放文字", 0));
    assert!(
        line_range(src, "缩放文字").contains(&engine.locate_point(&compiled, 0, scx, scy).unwrap())
    );

    // 5. Tall display math (clicking top, middle fraction, or bottom subscript) locates
    //    consistently to the preceding paragraph.
    let before_dm = line_range(src, "复杂公式前一段");
    for sym in ["∑", "𝑛", "𝑖"] {
        let (fx, fy) = center(find_text(&compiled, 0, sym, 0));
        let off = engine
            .locate_point(&compiled, 0, fx, fy)
            .unwrap_or_else(|| panic!("display math symbol {sym:?}"));
        assert!(
            before_dm.contains(&off),
            "display math {sym:?} offset {off} not in {before_dm:?}"
        );
    }

    // 6. Standalone `#line(length: 100%)` shape from `/main.typ` locates to its line.
    let after_dm_bounds = find_text(&compiled, 0, "复杂公式后一段", 0);
    let mut shapes = Vec::new();
    collect_shapes(
        &compiled.document.pages()[0].frame,
        Transform::identity(),
        &mut shapes,
    );
    let hr = shapes
        .iter()
        .copied()
        .find(|b| b[1] > after_dm_bounds[3] && b[1] < 700.0 && (b[2] - b[0]) > 400.0)
        .expect("horizontal line shape");
    let (lx, ly) = center(hr);
    let off_line = engine
        .locate_point(&compiled, 0, lx, ly)
        .expect("horizontal rule hit");
    assert!(line_range(src, "#line(length: 100%)").contains(&off_line));

    // 7. Calling locate_point with a stale Compiled after main shrinks never panics.
    engine.set_main("short");
    let _ = engine.locate_point(&compiled, 0, ix, iy);
}
