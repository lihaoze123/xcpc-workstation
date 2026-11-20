//! A region of a page renders the pixels the whole page has there.

use xcpc_engine::{Engine, FontOptions};

const SAMPLE: &str = include_str!("../../../tests/fixtures/olymp-ref/contest-zh/main.typ");

#[test]
fn region_matches_whole_page() {
    let mut engine = Engine::new(&FontOptions::from_env(), None);
    engine.set_main(SAMPLE);
    let compiled = engine.compile().unwrap();
    let scale = 2.5;
    let whole = compiled.render(0, scale).unwrap();
    // An inner tile and one cut by the page's right and bottom edges.
    for [x, y, w, h] in [[512, 512, 512, 512], [1024, 1536, 512, 512]] {
        let w = w.min(whole.width() - x);
        let h = h.min(whole.height() - y);
        let tile = compiled.render_region(0, scale, [x, y, w, h]).unwrap();
        assert_eq!((tile.width(), tile.height()), (w, h));
        let mut off = 0usize;
        for row in 0..h {
            for col in 0..w {
                let a = whole.pixel(x + col, y + row).unwrap();
                let b = tile.pixel(col, row).unwrap();
                let d = |p: u8, q: u8| p.abs_diff(q);
                if d(a.red(), b.red()) + d(a.green(), b.green()) + d(a.blue(), b.blue()) > 6 {
                    off += 1;
                }
            }
        }
        // Antialiasing may differ in a few edge pixels, not the content.
        assert!(
            off * 1000 < (w * h) as usize,
            "{off} of {} pixels differ",
            w * h
        );
    }
}

/// All tiles of `TILE` pixels of every page at `scale`.
fn all_tiles(compiled: &xcpc_engine::Compiled, scale: f32) -> Vec<(usize, [u32; 4])> {
    const TILE: u32 = 512;
    let mut out = Vec::new();
    for page in 0..compiled.page_count() {
        let (w, h) = compiled.page_size(page).unwrap();
        let (pw, ph) = (
            (w * f64::from(scale)).round() as u32,
            (h * f64::from(scale)).round() as u32,
        );
        for y in (0..ph).step_by(TILE as usize) {
            for x in (0..pw).step_by(TILE as usize) {
                out.push((page, [x, y, TILE.min(pw - x), TILE.min(ph - y)]));
            }
        }
    }
    out
}

#[test]
fn equal_region_fingerprints_render_equal_pixels() {
    let scale = 2.5;
    let mut engine = Engine::new(&FontOptions::from_env(), None);
    engine.set_main(SAMPLE);
    let before = engine.compile().unwrap();
    let tiles = all_tiles(&before, scale);
    let fingerprints = |c: &xcpc_engine::Compiled| -> Vec<u128> {
        tiles
            .iter()
            .map(|&(p, r)| c.region_fingerprints(p, scale, &[r]).unwrap()[0])
            .collect()
    };
    let old = fingerprints(&before);

    // A word in a paragraph, a formula, and a sample (a table).
    for (from, to) in [
        ("这是一道签到题", "这是一道很简单的签到题"),
        ("$-10^9 <= a,b <= 10^9$", "$-10^18 <= a,b <= 10^18$"),
        ("(\"1 2\", \"3\")", "(\"1 2\", \"4\")"),
    ] {
        let edited = SAMPLE.replace(from, to);
        assert_ne!(edited, SAMPLE, "{from}");
        engine.set_main(&edited);
        let after = engine.compile().unwrap();
        assert_eq!(after.page_count(), before.page_count());
        let new = fingerprints(&after);
        let mut same = 0;
        for (i, &(page, region)) in tiles.iter().enumerate() {
            if old[i] != new[i] {
                continue;
            }
            same += 1;
            let a = before.render_region(page, scale, region).unwrap();
            let b = after.render_region(page, scale, region).unwrap();
            assert!(
                a.data() == b.data(),
                "{from}: tile {region:?} of page {page} changed under an equal fingerprint"
            );
        }
        let changed = tiles.len() - same;
        assert!(changed > 0, "{from}: some tile changed");
        assert!(
            changed * 4 < tiles.len(),
            "{from}: {changed} of {} tiles changed",
            tiles.len()
        );
        eprintln!("{from}: {changed} of {} tiles changed", tiles.len());
    }
}
