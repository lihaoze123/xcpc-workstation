//! Where a preview's time goes after an edit: compile, fingerprint, rasterize, encode.
//!
//!   cargo run --release -p xcpc-engine --example preview_bench
//!   XCPC_BENCH_COPIES=12 cargo run --release -p xcpc-engine --example preview_bench
//!
//! `XCPC_BENCH_COPIES=n` repeats the sample's problems n times: a long contest.
//! `XCPC_BENCH_THREADS=1` compiles each time on a new thread, as a thread pool would.

use std::time::Instant;

use xcpc_engine::{Engine, FontOptions};

const SAMPLE: &str = include_str!("../tests/contest-zh.typ");

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

fn main() {
    let copies: usize = std::env::var("XCPC_BENCH_COPIES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);
    let threads = std::env::var_os("XCPC_BENCH_THREADS").is_some();
    let at = SAMPLE.find("#problem(").unwrap();
    let source = format!("{}{}", &SAMPLE[..at], SAMPLE[at..].repeat(copies));

    let mut engine = Engine::new(
        &FontOptions::from_env(),
        Some(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))),
    );
    engine.set_main(&source);
    let t = Instant::now();
    let compiled = engine.compile().unwrap();
    let pages = compiled.page_count();
    println!("first compile        {:7.1} ms, {pages} pages", ms(t));

    // Typing: one more character each time, in the first problem and in the last.
    for (name, anchor, last) in [
        ("edit early", "这是一道签到题", false),
        ("edit late", "请使用 64 位整数", true),
    ] {
        let (mut compile, mut prints, mut changed) = (vec![], vec![], 0);
        let mut text = source.clone();
        let mut before: Vec<u128> = vec![];
        for i in 0..30 {
            let at = if last {
                text.rfind(anchor)
            } else {
                text.find(anchor)
            }
            .unwrap();
            text.insert(at, if i % 2 == 0 { '甲' } else { '乙' });
            engine.set_main(&text);
            let t = Instant::now();
            let c = if threads {
                std::thread::scope(|s| s.spawn(|| engine.compile().unwrap()).join().unwrap())
            } else {
                engine.compile().unwrap()
            };
            compile.push(ms(t));
            // Every page's fingerprint, and a screenful of tiles of one page, as the
            // preview takes them.
            let t = Instant::now();
            let now: Vec<u128> = (0..c.page_count())
                .map(|p| c.page_fingerprint(p).unwrap())
                .collect();
            let tiles: Vec<[u32; 4]> = (0..12)
                .map(|k| [k % 3 * 512, k / 3 * 512, 512, 512])
                .collect();
            let _ = c.region_fingerprints(1, 2.5, &tiles);
            prints.push(ms(t));
            if i > 0 {
                changed += now.iter().zip(&before).filter(|(a, b)| a != b).count()
                    + now.len().abs_diff(before.len());
            }
            before = now;
        }
        println!(
            "{name:<20} compile {:5.1} ms  fingerprints {:4.2} ms  pages changed {:.1}/{pages}",
            median(compile),
            median(prints),
            changed as f64 / 29.0
        );
    }
    if copies > 1 {
        return;
    }

    let compiled = engine.compile().unwrap();
    for scale in [1.0f32, 2.0, 2.5, 4.0] {
        let (mut raster, mut tile, mut png) = (vec![], vec![], vec![]);
        for _ in 0..3 {
            for p in 0..pages {
                let t = Instant::now();
                let pix = compiled.render(p, scale).unwrap();
                raster.push(ms(t));
                let t = Instant::now();
                let _ = compiled.render_region(p, scale, [0, 0, 512, 512]).unwrap();
                tile.push(ms(t));
                let t = Instant::now();
                let _ = pix.encode_png().unwrap();
                png.push(ms(t));
            }
        }
        println!(
            "scale {scale:3}: raster/page {:6.1} ms   tile {:5.2} ms   png encode/page {:6.1} ms",
            median(raster),
            median(tile),
            median(png)
        );
    }
}
