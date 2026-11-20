//! Where a preview's time goes after an edit: compile, fingerprint, rasterize, encode.
//!
//!   cargo run --release -p xcpc-engine --example preview_bench

use std::time::Instant;

use xcpc_engine::{Engine, FontOptions};

const SAMPLE: &str = include_str!("../../../tests/fixtures/olymp-ref/contest-zh/main.typ");

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

fn main() {
    let mut engine = Engine::new(&FontOptions::from_env(), None);
    engine.set_main(SAMPLE);
    let t = Instant::now();
    let compiled = engine.compile().unwrap();
    println!(
        "first compile        {:7.1} ms, {} pages",
        ms(t),
        compiled.page_count()
    );
    let pages = compiled.page_count();

    // Typing: one more character each time, early (page 1) and late (last page).
    for (name, anchor) in [
        ("edit page 1", "这是一道签到题"),
        ("edit last page", "请使用 64 位整数"),
    ] {
        let (mut compile, mut fp, mut changed) = (vec![], vec![], 0);
        let mut text = SAMPLE.to_owned();
        let mut last: Vec<u128> = vec![];
        for i in 0..30 {
            let at = text.find(anchor).unwrap();
            text.insert(at, if i % 2 == 0 { '甲' } else { '乙' });
            engine.set_main(&text);
            let t = Instant::now();
            // `XCPC_BENCH_THREADS=1`: each compilation on a new thread, as a pool runs them.
            let c = if std::env::var_os("XCPC_BENCH_THREADS").is_some() {
                std::thread::scope(|s| s.spawn(|| engine.compile().unwrap()).join().unwrap())
            } else {
                engine.compile().unwrap()
            };
            compile.push(ms(t));
            let t = Instant::now();
            let f: Vec<u128> = (0..c.page_count())
                .map(|p| c.page_fingerprint(p).unwrap())
                .collect();
            fp.push(ms(t));
            if i > 0 {
                changed += f.iter().zip(&last).filter(|(a, b)| a != b).count()
                    + f.len().abs_diff(last.len());
            }
            last = f;
        }
        println!(
            "{name:<20} compile {:5.1} ms  fingerprint {:4.2} ms  pages changed {:.1}/{pages}",
            median(compile),
            median(fp),
            changed as f64 / 29.0
        );
    }

    let compiled = engine.compile().unwrap();
    for scale in [1.0f32, 2.0, 2.5, 4.0] {
        let (mut raster, mut png) = (vec![], vec![]);
        for _ in 0..3 {
            for p in 0..pages {
                let t = Instant::now();
                let pix = compiled.render(p, scale).unwrap();
                raster.push(ms(t));
                let t = Instant::now();
                let _ = pix.encode_png().unwrap();
                png.push(ms(t));
            }
        }
        println!(
            "scale {scale:3}: raster/page {:6.1} ms   png encode/page {:6.1} ms",
            median(raster),
            median(png)
        );
    }
    let mut svg = vec![];
    let mut bytes = 0;
    for p in 0..pages {
        let t = Instant::now();
        bytes += compiled.render_svg(p).unwrap().len();
        svg.push(ms(t));
    }
    println!(
        "svg/page {:6.1} ms, {} KiB total",
        median(svg),
        bytes / 1024
    );
}
