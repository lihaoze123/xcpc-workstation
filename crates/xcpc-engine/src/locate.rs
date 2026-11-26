//! Reverse lookup from a point on a rendered page to a byte offset in `/main.typ`.
//!
//! Walks `page.frame` with the same transform composition as `overflow` and `fingerprint`,
//! collects items whose spans map back into `/main.typ`, tracks evaluated formula items
//! (`#m(...)` / `#dm(...)`) so clicks on math fall back to the same line or adjacent
//! `/main.typ` content, and skips template/header/footer artifacts.

use typst::foundations::{Label, Selector, Value};
use typst::introspection::{Introspector, Location, Tag};
use typst::layout::{Abs, Frame, FrameItem, Point, Transform};
use typst::syntax::{Source, Span, SpanKind, SyntaxKind};
use typst::utils::PicoStr;
use typst_layout::{Page, PagedDocument};

use crate::Compiled;

/// Maximum Euclidean distance (pt) from a click to a `/main.typ` item for nearby fallback.
const NEARBY_RADIUS: f64 = 16.0;

/// Small hit padding (pt) around formula glyphs/bars so clicks in math operator spacing
/// (e.g. thin/thick spaces around `+` or `<=`) still register as hitting the formula.
const FORMULA_HIT_PAD: f64 = 4.5;

/// Small hit padding (pt) around header/footer artifacts so clicks directly on running
/// heads, feet, or their rules do not leak into nearby body lines.
const ARTIFACT_HIT_PAD: f64 = 2.0;

/// Tolerance (pt) outside `<xcpc-text-area>` vertical bounds where nearby fallback may
/// still catch the first/last body line without leaking into the running header/footer.
const TEXT_AREA_SLACK: f64 = 4.0;

/// Maximum vertical gap (pt) from a standalone display formula (`#dm(...)`) to an
/// adjacent `/main.typ` text line.
const MAX_DISPLAY_MATH_GAP: f64 = 40.0;

/// Kind of a laid-out leaf item, ordered by hit preference (smaller = higher priority).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ItemKind {
    Text = 0,
    Image = 1,
    Shape = 2,
}

/// A laid-out leaf item whose span resolves to `/main.typ`.
#[derive(Debug, Clone, Copy)]
struct MainItem {
    kind: ItemKind,
    /// Axis-aligned bounding box in page points `[x0, y0, x1, y1]`.
    bounds: [f64; 4],
    /// Local bounding box before `at`, used for exact hit-testing under rotation/skew.
    local: [f64; 4],
    /// World transform from local coords to page coords.
    at: Transform,
    /// Frame nesting depth (deeper = more specific).
    depth: usize,
    /// Byte offset inside `/main.typ`.
    offset: usize,
}

/// A laid-out leaf item belonging to an evaluated formula (`#m(...)` / `#dm(...)`).
#[derive(Debug, Clone, Copy)]
struct FormulaItem {
    bounds: [f64; 4],
    local: [f64; 4],
    at: Transform,
    /// Enclosing `math.equation` tag location, when present.
    equation: Option<Location>,
}

#[derive(Default)]
struct Collected {
    items: Vec<MainItem>,
    formulas: Vec<FormulaItem>,
    artifacts: Vec<[f64; 4]>,
    active_formulas: Vec<Location>,
    active_artifacts: Vec<Location>,
}

impl MainItem {
    fn contains(&self, x_pt: f64, y_pt: f64) -> bool {
        contains_point(self.bounds, self.local, self.at, x_pt, y_pt)
    }

    fn area(&self) -> f64 {
        let w = (self.bounds[2] - self.bounds[0]).max(0.0);
        let h = (self.bounds[3] - self.bounds[1]).max(0.0);
        w * h
    }

    fn center_y(&self) -> f64 {
        0.5 * (self.bounds[1] + self.bounds[3])
    }

    fn height(&self) -> f64 {
        (self.bounds[3] - self.bounds[1]).max(0.0)
    }

    fn distance_to(&self, x_pt: f64, y_pt: f64) -> (f64, f64) {
        let dx = axis_distance(x_pt, self.bounds[0], self.bounds[2]);
        let dy = axis_distance(y_pt, self.bounds[1], self.bounds[3]);
        (dx, dy)
    }
}

impl FormulaItem {
    fn contains(&self, x_pt: f64, y_pt: f64) -> bool {
        contains_point(self.bounds, self.local, self.at, x_pt, y_pt)
    }
}

/// Resolves `(x_pt, y_pt)` (points from top-left of 0-based `page`) to a byte offset in `main`.
pub fn point(
    main: &Source,
    compiled: &Compiled,
    page: usize,
    x_pt: f64,
    y_pt: f64,
) -> Option<usize> {
    if !x_pt.is_finite() || !y_pt.is_finite() {
        return None;
    }
    let page_data = compiled.document.pages().get(page)?;
    let area = text_area(&compiled.document);
    locate_in_page(main, page_data, area, x_pt, y_pt)
}

fn locate_in_page(
    main: &Source,
    page: &Page,
    area: Option<[f64; 4]>,
    x_pt: f64,
    y_pt: f64,
) -> Option<usize> {
    let mut collected = Collected::default();
    walk(main, &page.frame, Transform::identity(), 0, &mut collected);
    if collected.items.is_empty() {
        return None;
    }

    // 1. Exact containment on `/main.typ` items: prefer Text/Image over Shape, then
    //    deepest nesting, then closest glyph center / smallest area.
    let mut best_exact: Option<MainItem> = None;
    for &item in &collected.items {
        if !item.contains(x_pt, y_pt) {
            continue;
        }
        let better = match best_exact {
            None => true,
            Some(best) => compare_exact(item, best, x_pt, y_pt),
        };
        if better {
            best_exact = Some(item);
        }
    }
    if let Some(hit) = best_exact {
        return Some(hit.offset);
    }

    // Do not let fallback searches jump from running headers/footers or their rules into
    // body lines near the top/bottom of a continuation page.
    if collected
        .artifacts
        .iter()
        .any(|&b| box_distance(b, x_pt, y_pt) <= ARTIFACT_HIT_PAD)
    {
        return None;
    }
    if let Some([_, top, _, height]) = area {
        let bottom = top + height;
        if y_pt < top - TEXT_AREA_SLACK || y_pt > bottom + TEXT_AREA_SLACK {
            return None;
        }
    }

    // 2. Click landed on an evaluated formula (`#m(...)` / `#dm(...)`) whose spans belong
    //    to `/xcpc/markdown.typ`, `/xcpc/mitex/*`, or detached math ops: first look for
    //    the horizontally nearest `/main.typ` item on the same line, then fall back to
    //    the vertically nearest `/main.typ` line for standalone display math (`#dm(...)`).
    if let Some(hit_formula) = closest_formula(&collected.formulas, x_pt, y_pt) {
        if let Some(offset) = nearest_on_same_line(&collected.items, hit_formula.bounds, x_pt, y_pt)
        {
            return Some(offset);
        }
        let eq_bounds = equation_bounds(&collected.formulas, hit_formula);
        if let Some(offset) = nearest_line_for_display_math(&collected.items, eq_bounds, x_pt, y_pt)
        {
            return Some(offset);
        }
    }

    // 3. Nearby fallback within `NEARBY_RADIUS` (16pt): prefer primary items (Text/Image)
    //    over Shape, then items on the same vertical line, then smaller dy and Euclidean
    //    distance.
    let mut best_near: Option<NearCandidate> = None;
    for &item in &collected.items {
        let (dx, dy) = item.distance_to(x_pt, y_pt);
        let dist = dx.hypot(dy);
        if dist > NEARBY_RADIUS {
            continue;
        }
        let candidate = NearCandidate {
            is_shape: item.kind == ItemKind::Shape,
            off_row: dy > 2.0,
            dy,
            dist,
            kind: item.kind,
            area: item.area(),
            offset: item.offset,
        };
        if best_near.is_none_or(|best| candidate.is_better_than(best)) {
            best_near = Some(candidate);
        }
    }
    best_near.map(|b| b.offset)
}

#[derive(Clone, Copy)]
struct NearCandidate {
    is_shape: bool,
    off_row: bool,
    dy: f64,
    dist: f64,
    kind: ItemKind,
    area: f64,
    offset: usize,
}

impl NearCandidate {
    fn is_better_than(self, other: Self) -> bool {
        self.is_shape
            .cmp(&other.is_shape)
            .then_with(|| self.off_row.cmp(&other.off_row))
            .then_with(|| self.dy.total_cmp(&other.dy))
            .then_with(|| self.dist.total_cmp(&other.dist))
            .then_with(|| self.kind.cmp(&other.kind))
            .then_with(|| self.area.total_cmp(&other.area))
            .is_lt()
    }
}

fn compare_exact(a: MainItem, b: MainItem, x_pt: f64, y_pt: f64) -> bool {
    let a_shape = a.kind == ItemKind::Shape;
    let b_shape = b.kind == ItemKind::Shape;
    a_shape
        .cmp(&b_shape)
        .then_with(|| a.kind.cmp(&b.kind))
        .then_with(|| b.depth.cmp(&a.depth))
        .then_with(|| {
            if a.kind == ItemKind::Text {
                let a_cy = (a.center_y() - y_pt).abs();
                let b_cy = (b.center_y() - y_pt).abs();
                let a_cx = (0.5 * (a.bounds[0] + a.bounds[2]) - x_pt).abs();
                let b_cx = (0.5 * (b.bounds[0] + b.bounds[2]) - x_pt).abs();
                a_cy.total_cmp(&b_cy).then_with(|| a_cx.total_cmp(&b_cx))
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .then_with(|| a.area().total_cmp(&b.area()))
        .is_lt()
}

fn closest_formula(formulas: &[FormulaItem], x_pt: f64, y_pt: f64) -> Option<FormulaItem> {
    let mut best: Option<(bool, f64, FormulaItem)> = None;
    for &f in formulas {
        let exact = f.contains(x_pt, y_pt);
        let dist = if exact {
            0.0
        } else {
            box_distance(f.bounds, x_pt, y_pt)
        };
        if !exact && dist > FORMULA_HIT_PAD {
            continue;
        }
        let better = match best {
            None => true,
            Some((b_exact, b_dist, _)) => (!exact)
                .cmp(&(!b_exact))
                .then_with(|| dist.total_cmp(&b_dist))
                .is_lt(),
        };
        if better {
            best = Some((exact, dist, f));
        }
    }
    best.map(|b| b.2)
}

fn equation_bounds(formulas: &[FormulaItem], hit: FormulaItem) -> [f64; 4] {
    let Some(loc) = hit.equation else {
        return hit.bounds;
    };
    let mut bounds = hit.bounds;
    for &f in formulas {
        if f.equation == Some(loc) {
            bounds = [
                bounds[0].min(f.bounds[0]),
                bounds[1].min(f.bounds[1]),
                bounds[2].max(f.bounds[2]),
                bounds[3].max(f.bounds[3]),
            ];
        }
    }
    bounds
}

/// Finds the nearest `/main.typ` item sharing the same horizontal line as `formula_bounds`.
fn nearest_on_same_line(
    items: &[MainItem],
    formula_bounds: [f64; 4],
    x_pt: f64,
    y_pt: f64,
) -> Option<usize> {
    let fy0 = formula_bounds[1];
    let fy1 = formula_bounds[3];
    let fcy = 0.5 * (fy0 + fy1);

    let mut matches = Vec::new();
    let mut min_cy_diff = f64::INFINITY;
    for &item in items {
        if item.kind == ItemKind::Shape {
            continue;
        }
        let iy0 = item.bounds[1];
        let iy1 = item.bounds[3];
        let icy = item.center_y();
        let line_tol = (item.height() * 0.6).clamp(5.0, 10.0);
        let overlaps_y = iy0 <= fy1 && iy1 >= fy0;
        let close_y = (icy - fcy).abs() <= line_tol || (icy - y_pt).abs() <= line_tol;
        if !overlaps_y && !close_y {
            continue;
        }
        let cy_diff = (icy - fcy).abs();
        min_cy_diff = min_cy_diff.min(cy_diff);
        matches.push((cy_diff, item));
    }

    // Keep only items on the vertically closest matching line band so an adjacent line
    // directly under `x_pt` cannot beat the formula's own line.
    let mut best: Option<(ItemKind, f64, f64, f64, usize)> = None;
    for (cy_diff, item) in matches {
        if cy_diff > min_cy_diff + 6.0 {
            continue;
        }
        let (dx, dy) = item.distance_to(x_pt, y_pt);
        let area = item.area();
        let better = match best {
            None => true,
            Some((bk, bdx, bdy, barea, _)) => item
                .kind
                .cmp(&bk)
                .then_with(|| dx.total_cmp(&bdx))
                .then_with(|| dy.total_cmp(&bdy))
                .then_with(|| area.total_cmp(&barea))
                .is_lt(),
        };
        if better {
            best = Some((item.kind, dx, dy, area, item.offset));
        }
    }
    best.map(|b| b.4)
}

/// For standalone display math (`#dm(...)`) that sits on its own line with no `/main.typ`
/// text horizontally beside it, finds the nearest `/main.typ` line immediately above (or
/// below) within reasonable display-equation vertical spacing.
fn nearest_line_for_display_math(
    items: &[MainItem],
    eq_bounds: [f64; 4],
    x_pt: f64,
    y_pt: f64,
) -> Option<usize> {
    let fy0 = eq_bounds[1];
    let fy1 = eq_bounds[3];

    let mut best_above: Option<(f64, f64, usize)> = None;
    let mut best_below: Option<(f64, f64, usize)> = None;
    let update = |slot: &mut Option<(f64, f64, usize)>, vgap: f64, dx: f64, offset: usize| {
        let better = match *slot {
            None => true,
            Some((b_vgap, b_dx, _)) => vgap
                .total_cmp(&b_vgap)
                .then_with(|| dx.total_cmp(&b_dx))
                .is_lt(),
        };
        if better {
            *slot = Some((vgap, dx, offset));
        }
    };

    for &item in items {
        if item.kind == ItemKind::Shape {
            continue;
        }
        let (is_below, vgap) = if item.bounds[3] <= fy0 + 2.0 {
            (false, (fy0 - item.bounds[3]).max(0.0))
        } else if item.bounds[1] >= fy1 - 2.0 {
            (true, (item.bounds[1] - fy1).max(0.0))
        } else {
            (false, 0.0)
        };
        if vgap > MAX_DISPLAY_MATH_GAP {
            continue;
        }
        let (dx, _) = item.distance_to(x_pt, y_pt);
        if is_below {
            update(&mut best_below, vgap, dx, item.offset);
        } else {
            update(&mut best_above, vgap, dx, item.offset);
        }
    }

    // Prefer the preceding line above when it is at most 10pt farther than the line below.
    match (best_above, best_below) {
        (Some(above), Some(below)) => {
            if above.0 <= below.0 + 10.0 {
                Some(above.2)
            } else {
                Some(below.2)
            }
        }
        (Some(above), None) => Some(above.2),
        (None, Some(below)) => Some(below.2),
        (None, None) => None,
    }
}

fn walk(main: &Source, frame: &Frame, ts: Transform, depth: usize, out: &mut Collected) {
    for (pos, item) in frame.items() {
        let at = ts.pre_concat(Transform::translate(pos.x, pos.y));
        match item {
            FrameItem::Tag(Tag::Start(elem, _)) => {
                if let Some(loc) = elem.location() {
                    let name = elem.elem().name();
                    if name == "artifact" {
                        out.active_artifacts.push(loc);
                    } else if name == "equation" && is_markdown_math_span(elem.span()) {
                        out.active_formulas.push(loc);
                    }
                }
            }
            FrameItem::Tag(Tag::End(loc, ..)) => {
                if let Some(i) = out.active_artifacts.iter().rposition(|&l| l == *loc) {
                    out.active_artifacts.remove(i);
                }
                if let Some(i) = out.active_formulas.iter().rposition(|&l| l == *loc) {
                    out.active_formulas.remove(i);
                }
            }
            FrameItem::Group(group) => {
                let prev_artifacts = out.active_artifacts.len();
                let prev_formulas = out.active_formulas.len();
                walk(
                    main,
                    &group.frame,
                    at.pre_concat(group.transform),
                    depth + 1,
                    out,
                );
                // Top-level page regions (header, body, footer) are independent; prevent
                // any tag unclosed at a page break from bleeding into another region.
                if depth == 0 {
                    out.active_artifacts.truncate(prev_artifacts);
                    out.active_formulas.truncate(prev_formulas);
                }
            }
            FrameItem::Text(text) => {
                let size = text.size;
                let metrics = text.font.metrics();
                let asc = metrics.ascender.at(size).to_pt();
                let desc = metrics.descender.at(size).to_pt();
                let (base_y0, base_y1) = if -asc < -desc {
                    (-asc, -desc)
                } else {
                    (-size.to_pt() * 0.85, size.to_pt() * 0.25)
                };

                let in_artifact = !out.active_artifacts.is_empty();
                let active_eq = out.active_formulas.last().copied();
                let mut last_span: Option<(Span, Option<MainSpanRange>)> = None;
                let mut x = 0.0;
                let mut y = 0.0;
                for glyph in &text.glyphs {
                    let adv_x = glyph.x_advance.at(size).to_pt();
                    let adv_y = -glyph.y_advance.at(size).to_pt();
                    let oy = -glyph.y_offset.at(size).to_pt();
                    let (gx0, gx1) = if adv_x >= 0.0 {
                        (x, x + adv_x)
                    } else {
                        (x + adv_x, x)
                    };
                    let local = [gx0, y + oy + base_y0, gx1, y + oy + base_y1];
                    let bounds = transformed(local, at);
                    x += adv_x;
                    y += adv_y;
                    if !is_finite_rect(bounds) {
                        continue;
                    }
                    if in_artifact {
                        out.artifacts.push(bounds);
                        continue;
                    }
                    let (span, sub) = glyph.span;
                    let resolved = match last_span {
                        Some((prev, cached)) if prev == span => cached,
                        _ => {
                            let r = resolve_main_span_range(main, span);
                            last_span = Some((span, r));
                            r
                        }
                    };
                    if let Some((start, end, apply_sub)) = resolved {
                        let offset =
                            apply_glyph_offset(main.text(), start, end, apply_sub, Some(sub));
                        out.items.push(MainItem {
                            kind: ItemKind::Text,
                            bounds,
                            local,
                            at,
                            depth,
                            offset,
                        });
                    } else if active_eq.is_some() || is_markdown_math_span(span) {
                        out.formulas.push(FormulaItem {
                            bounds,
                            local,
                            at,
                            equation: active_eq,
                        });
                    }
                }
            }
            FrameItem::Image(_, size, span) => {
                let local = [0.0, 0.0, size.x.to_pt(), size.y.to_pt()];
                let bounds = transformed(local, at);
                if !is_finite_rect(bounds) {
                    continue;
                }
                if !out.active_artifacts.is_empty() {
                    out.artifacts.push(bounds);
                    continue;
                }
                if let Some(offset) = resolve_main_span(main, *span, None) {
                    out.items.push(MainItem {
                        kind: ItemKind::Image,
                        bounds,
                        local,
                        at,
                        depth,
                        offset,
                    });
                }
            }
            FrameItem::Shape(shape, span) => {
                let b = shape.bbox(true);
                let local = [
                    b.min.x.to_pt(),
                    b.min.y.to_pt(),
                    b.max.x.to_pt(),
                    b.max.y.to_pt(),
                ];
                let bounds = transformed(local, at);
                if !is_finite_rect(bounds) {
                    continue;
                }
                if !out.active_artifacts.is_empty() {
                    out.artifacts.push(bounds);
                    continue;
                }
                let active_eq = out.active_formulas.last().copied();
                if let Some(offset) = resolve_main_span(main, *span, None) {
                    out.items.push(MainItem {
                        kind: ItemKind::Shape,
                        bounds,
                        local,
                        at,
                        depth,
                        offset,
                    });
                } else if active_eq.is_some() || is_markdown_math_span(*span) {
                    out.formulas.push(FormulaItem {
                        bounds,
                        local,
                        at,
                        equation: active_eq,
                    });
                }
            }
            FrameItem::Link(..) => {}
        }
    }
}

/// Whether `span` originates from the bundled Markdown math wrapper (`/xcpc/markdown.typ`
/// or `/xcpc/mitex/*`), rather than `/main.typ` or page chrome in `olymp-classic.typ`.
fn is_markdown_math_span(span: Span) -> bool {
    let Some(id) = span.id() else {
        return false;
    };
    let path = id.vpath().get_with_slash();
    path == "/xcpc/markdown.typ" || path.starts_with("/xcpc/mitex/")
}

type MainSpanRange = (usize, usize, bool);

/// Resolves a span belonging to `main` (`/main.typ`) to `(start, end, apply_sub)`.
fn resolve_main_span_range(main: &Source, span: Span) -> Option<MainSpanRange> {
    if span.id() != Some(main.id()) {
        return None;
    }
    let (range, apply_sub) = match span.get() {
        SpanKind::Detached => return None,
        SpanKind::Number { .. } => {
            let node = main.find(span)?;
            let apply_sub = matches!(node.kind(), SyntaxKind::Text);
            (node.range(), apply_sub)
        }
        SpanKind::Range { range, .. } => (range, false),
    };
    let text = main.text();
    let len = text.len();
    let start = text.floor_char_boundary(range.start.min(len));
    let end = text.floor_char_boundary(range.end.min(len));
    Some((start, end, apply_sub))
}

fn apply_glyph_offset(
    text: &str,
    start: usize,
    end: usize,
    apply_sub: bool,
    glyph_offset: Option<u16>,
) -> usize {
    if apply_sub
        && let Some(sub) = glyph_offset
        && start < end
    {
        let raw = start.saturating_add(usize::from(sub));
        let clamped = if raw < end {
            raw
        } else {
            text[start..end]
                .char_indices()
                .next_back()
                .map_or(start, |(i, _)| start + i)
        };
        return text.floor_char_boundary(clamped).max(start);
    }
    start
}

/// Resolves a span belonging to `main` (`/main.typ`) to a UTF-8 char-boundary byte offset.
fn resolve_main_span(main: &Source, span: Span, glyph_offset: Option<u16>) -> Option<usize> {
    let (start, end, apply_sub) = resolve_main_span_range(main, span)?;
    Some(apply_glyph_offset(
        main.text(),
        start,
        end,
        apply_sub,
        glyph_offset,
    ))
}

/// `[left, top, width, height]` in points from `<xcpc-text-area>`, if the template emitted it.
fn text_area(document: &PagedDocument) -> Option<[f64; 4]> {
    let label = Label::new(PicoStr::intern("xcpc-text-area"))?;
    let found = document.introspector().query(&Selector::Label(label));
    let Value::Dict(dict) = found.first()?.get_by_name("value").ok()? else {
        return None;
    };
    let get = |key: &str| match dict.get(key).ok()? {
        Value::Length(l) => Some(l.abs.to_pt()),
        _ => None,
    };
    Some([get("left")?, get("top")?, get("width")?, get("height")?])
}

fn contains_point(bounds: [f64; 4], local: [f64; 4], at: Transform, x_pt: f64, y_pt: f64) -> bool {
    if x_pt < bounds[0] || x_pt > bounds[2] || y_pt < bounds[1] || y_pt > bounds[3] {
        return false;
    }
    if at.is_only_translate() {
        return true;
    }
    if let Some(inv) = at.invert() {
        let p = Point::new(Abs::pt(x_pt), Abs::pt(y_pt)).transform_inf(inv);
        let (lx, ly) = (p.x.to_pt(), p.y.to_pt());
        let (x0, x1) = (local[0].min(local[2]), local[0].max(local[2]));
        let (y0, y1) = (local[1].min(local[3]), local[1].max(local[3]));
        return lx >= x0 && lx <= x1 && ly >= y0 && ly <= y1;
    }
    true
}

fn is_finite_rect(r: [f64; 4]) -> bool {
    r.iter().all(|v| v.is_finite()) && r[0] <= r[2] && r[1] <= r[3]
}

fn axis_distance(v: f64, min: f64, max: f64) -> f64 {
    if v < min {
        min - v
    } else if v > max {
        v - max
    } else {
        0.0
    }
}

fn box_distance(bounds: [f64; 4], x_pt: f64, y_pt: f64) -> f64 {
    let dx = axis_distance(x_pt, bounds[0], bounds[2]);
    let dy = axis_distance(y_pt, bounds[1], bounds[3]);
    dx.hypot(dy)
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
        let (px, py) = (p.x.to_pt(), p.y.to_pt());
        out = [
            out[0].min(px),
            out[1].min(py),
            out[2].max(px),
            out[3].max(py),
        ];
    }
    out
}
