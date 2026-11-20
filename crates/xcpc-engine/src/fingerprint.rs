//! Page fingerprints that ignore source positions.
//!
//! A laid-out frame carries the spans of the source it came from, so any edit shifts
//! the spans of everything after it and `hash128(page)` changes on every page. The
//! fingerprint hashes only what a renderer draws: geometry, paints, glyphs, images,
//! transforms and clips. Links and introspection tags draw nothing and are skipped.

use std::hash::Hash;

use siphasher::sip128::{Hasher128, SipHasher13};
use typst::layout::{Frame, FrameItem};
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
