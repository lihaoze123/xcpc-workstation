//! Page fingerprints that ignore source positions.
//!
//! A laid-out frame carries the spans of the source it came from, so any edit shifts
//! the spans of everything after it and `hash128(page)` changes on every page. The
//! fingerprint hashes only what a renderer draws: geometry, paints, glyphs, images,
//! transforms and clips. Links and introspection tags draw nothing and are skipped.
//!
//! [`regions`] fingerprints parts of a page (a preview's tiles) by the items that may
//! draw there, so an edit changes only the fingerprints of the parts it touches.

use std::hash::Hash;

use siphasher::sip128::{Hasher128, SipHasher13};
use typst::layout::{Abs, Frame, FrameItem, Point, Transform};
use typst::text::TextItem;
use typst_layout::Page;

/// The fingerprint of `page`: equal fingerprints rasterize to the same pixels.
pub fn page(page: &Page) -> u128 {
    let mut h = SipHasher13::new();
    page.fill_or_white().hash(&mut h);
    page.bleed.hash(&mut h);
    frame(&page.frame, &mut h);
    h.finish128().as_u128()
}

fn frame(frame: &Frame, h: &mut SipHasher13) {
    frame.size().hash(h);
    for (pos, item) in frame.items() {
        match item {
            FrameItem::Group(group) => {
                0u8.hash(h);
                pos.hash(h);
                group.transform.hash(h);
                group.clip.hash(h);
                self::frame(&group.frame, h);
            }
            FrameItem::Text(text) => {
                1u8.hash(h);
                pos.hash(h);
                text.font.hash(h);
                text.size.hash(h);
                text.fill.hash(h);
                text.stroke.hash(h);
                text.glyphs.len().hash(h);
                for g in &text.glyphs {
                    (g.id, g.x_advance, g.x_offset, g.y_advance, g.y_offset).hash(h);
                }
            }
            FrameItem::Shape(shape, _) => {
                2u8.hash(h);
                pos.hash(h);
                shape.hash(h);
            }
            FrameItem::Image(image, size, _) => {
                3u8.hash(h);
                pos.hash(h);
                image.hash(h);
                size.hash(h);
            }
            FrameItem::Link(..) | FrameItem::Tag(_) => {}
        }
    }
}

/// What one drawn item (a text run, a shape, an image) covers on its page, in points
/// (`[x0, y0, x1, y1]`, generously), and a hash of how it draws there.
struct Leaf {
    bounds: [f64; 4],
    hash: u128,
}

/// The fingerprints of regions `[x, y, width, height]` (pixels at `pixel_per_pt`) of
/// `page`: equal fingerprints rasterize the region to the same pixels. They hash the
/// items whose bounds meet the region, so an edit changes those of the regions it
/// touches and keeps the others.
pub fn regions(page: &Page, pixel_per_pt: f32, regions: &[[u32; 4]]) -> Vec<u128> {
    let mut leaves = Vec::new();
    walk(&page.frame, Transform::identity(), &mut leaves);
    let mut base = SipHasher13::new();
    page.fill_or_white().hash(&mut base);
    page.bleed.hash(&mut base);
    let s = f64::from(pixel_per_pt);
    regions
        .iter()
        .map(|&[x, y, w, h]| {
            let mut h128 = base;
            (x, y, w, h, pixel_per_pt.to_bits()).hash(&mut h128);
            // A pixel at the edge is touched by antialiasing just outside it.
            let px = |v: u32, d: f64| (f64::from(v) + d) / s;
            let r = [px(x, -1.0), px(y, -1.0), px(x + w, 1.0), px(y + h, 1.0)];
            for leaf in &leaves {
                let b = leaf.bounds;
                if b[0] < r[2] && b[2] > r[0] && b[1] < r[3] && b[3] > r[1] {
                    leaf.hash.hash(&mut h128);
                }
            }
            h128.finish128().as_u128()
        })
        .collect()
}

/// Collects the leaves of `frame` drawn with `ts`, as `typst-render` places them.
fn walk(frame: &Frame, ts: Transform, out: &mut Vec<Leaf>) {
    for (pos, item) in frame.items() {
        let at = ts.pre_concat(Transform::translate(pos.x, pos.y));
        let (local, hash) = match item {
            FrameItem::Group(group) => {
                // A clip only takes away: the group's items count where they are.
                walk(&group.frame, at.pre_concat(group.transform), out);
                continue;
            }
            FrameItem::Text(text) => {
                let mut h = SipHasher13::new();
                1u8.hash(&mut h);
                text.font.hash(&mut h);
                text.size.hash(&mut h);
                text.fill.hash(&mut h);
                text.stroke.hash(&mut h);
                for g in &text.glyphs {
                    (g.id, g.x_advance, g.x_offset, g.y_advance, g.y_offset).hash(&mut h);
                }
                (text_bounds(text), h)
            }
            FrameItem::Shape(shape, _) => {
                let mut h = SipHasher13::new();
                2u8.hash(&mut h);
                shape.hash(&mut h);
                let b = shape.bbox(true);
                // Joins and caps may reach past the stroke's half width.
                let m = shape
                    .stroke
                    .as_ref()
                    .map_or(0.0, |s| s.thickness.to_pt() * 2.0)
                    + 1.0;
                let r = [
                    b.min.x.to_pt() - m,
                    b.min.y.to_pt() - m,
                    b.max.x.to_pt() + m,
                    b.max.y.to_pt() + m,
                ];
                (r, h)
            }
            FrameItem::Image(image, size, _) => {
                let mut h = SipHasher13::new();
                3u8.hash(&mut h);
                image.hash(&mut h);
                size.hash(&mut h);
                ([-1.0, -1.0, size.x.to_pt() + 1.0, size.y.to_pt() + 1.0], h)
            }
            FrameItem::Link(..) | FrameItem::Tag(_) => continue,
        };
        let mut h = hash;
        at.hash(&mut h);
        out.push(Leaf {
            bounds: transformed(local, at),
            hash: h.finish128().as_u128(),
        });
    }
}

/// Where a text run's glyphs may draw, relative to its baseline origin: their outlines'
/// boxes, or two ems around a glyph without one (a bitmap or SVG glyph), and a point more.
fn text_bounds(text: &TextItem) -> [f64; 4] {
    let size = text.size;
    let em = size.to_pt();
    let unit = em / text.font.units_per_em();
    let face = text.font.ttf();
    let stroke = text.stroke.as_ref().map_or(0.0, |s| s.thickness.to_pt());
    let mut r = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    let mut x = 0.0;
    for g in &text.glyphs {
        let ox = x + g.x_offset.at(size).to_pt();
        let oy = -g.y_offset.at(size).to_pt();
        let advance = g.x_advance.at(size).to_pt();
        let b = match face.glyph_bounding_box(ttf_parser::GlyphId(g.id)) {
            Some(b) => [
                ox + f64::from(b.x_min) * unit,
                oy - f64::from(b.y_max) * unit,
                ox + f64::from(b.x_max) * unit,
                oy - f64::from(b.y_min) * unit,
            ],
            None => [ox - em, oy - 2.0 * em, ox + advance + em, oy + em],
        };
        r = [
            r[0].min(b[0]),
            r[1].min(b[1]),
            r[2].max(b[2]),
            r[3].max(b[3]),
        ];
        x += advance;
    }
    if r[0] > r[2] {
        return [0.0; 4];
    }
    let m = stroke + 1.0;
    [r[0] - m, r[1] - m, r[2] + m, r[3] + m]
}

/// The box around `r` under `ts`.
fn transformed(r: [f64; 4], ts: Transform) -> [f64; 4] {
    let corners = [(r[0], r[1]), (r[2], r[1]), (r[0], r[3]), (r[2], r[3])];
    let mut out = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for (x, y) in corners {
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
