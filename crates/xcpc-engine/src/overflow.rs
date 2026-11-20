//! Content past the text area: what the template declares as `<xcpc-text-area>`
//! (left, top, width, height) against where each page's text, shapes and pictures
//! draw. Running heads and feet sit above and below the area and are not checked
//! downwards; nothing may stick out sideways or run off the bottom.

use typst::foundations::{Label, Selector, Value};
use typst::introspection::Introspector;
use typst::layout::{Abs, Frame, FrameItem, Point, Transform};
use typst::syntax::Span;
use typst::utils::PicoStr;
use typst_layout::PagedDocument;

/// How far past an edge counts (points): rules meet the edge exactly.
const SLACK: f64 = 0.5;

/// Typst hangs punctuation and hyphens into the margin (`text.overhang`), by part of
/// an em: text counts as past an edge beyond half an em.
const OVERHANG: f64 = 0.5;

pub struct Overflow {
    /// The page, 1-based.
    pub page: usize,
    pub message: String,
    /// What overflows first on it.
    pub span: Span,
}

pub fn check(document: &PagedDocument) -> Vec<Overflow> {
    let Some(area) = text_area(document) else {
        return Vec::new();
    };
    let [left, top, width, height] = area;
    let (right, bottom) = (left + width, top + height);
    let mut out = Vec::new();
    for (i, page) in document.pages().iter().enumerate() {
        let mut leaves = Vec::new();
        walk(&page.frame, Transform::identity(), &mut leaves);
        let mut worst: [(f64, Span); 3] = [(0.0, Span::detached()); 3];
        for (b, span, em) in leaves {
            let over = [
                left - b[0],
                b[2] - right,
                if b[1] < bottom { b[3] - bottom } else { 0.0 },
            ];
            let slack = SLACK.max(OVERHANG * em);
            for (w, o) in worst.iter_mut().zip(over) {
                if o > slack && o > w.0 {
                    *w = (o, span);
                }
            }
        }
        for (side, (by, span)) in ["left", "right", "bottom"].iter().zip(worst) {
            if by > 0.0 {
                out.push(Overflow {
                    page: i + 1,
                    message: format!(
                        "content runs {by:.1}pt past the {side} edge of the text on page {}",
                        i + 1
                    ),
                    span,
                });
            }
        }
    }
    out
}

/// `[left, top, width, height]` in points, from the template's metadata.
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

/// The boxes (`[x0, y0, x1, y1]` in points) of what `frame` draws, with their spans
/// and, for text, its size (else zero).
fn walk(frame: &Frame, ts: Transform, out: &mut Vec<([f64; 4], Span, f64)>) {
    for (pos, item) in frame.items() {
        let at = ts.pre_concat(Transform::translate(pos.x, pos.y));
        let mut em = 0.0;
        let (local, span) = match item {
            FrameItem::Group(group) => {
                walk(&group.frame, at.pre_concat(group.transform), out);
                continue;
            }
            FrameItem::Text(text) => {
                let size = text.size;
                let width: Abs = text.glyphs.iter().map(|g| g.x_advance.at(size)).sum();
                let (ascent, descent) =
                    (text.font.metrics().ascender, text.font.metrics().descender);
                let r = [
                    0.0,
                    -ascent.at(size).to_pt(),
                    width.to_pt(),
                    -descent.at(size).to_pt(),
                ];
                let span = text.glyphs.first().map_or(Span::detached(), |g| g.span.0);
                em = size.to_pt();
                (r, span)
            }
            FrameItem::Shape(shape, span) => {
                let b = shape.bbox(false);
                (
                    [
                        b.min.x.to_pt(),
                        b.min.y.to_pt(),
                        b.max.x.to_pt(),
                        b.max.y.to_pt(),
                    ],
                    *span,
                )
            }
            FrameItem::Image(_, size, span) => ([0.0, 0.0, size.x.to_pt(), size.y.to_pt()], *span),
            FrameItem::Link(..) | FrameItem::Tag(_) => continue,
        };
        out.push((transformed(local, at), span, em));
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
