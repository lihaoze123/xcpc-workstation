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
