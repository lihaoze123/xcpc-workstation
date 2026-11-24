//! Where a preview's time goes after an edit: compile, fingerprint, rasterize, encode.
//!
//!   cargo run --profile bench-fast -p xcpc-engine --example preview_bench
//!   XCPC_BENCH_COPIES=5 cargo run --profile bench-fast -p xcpc-engine --example preview_bench -- --json
//!
//! `XCPC_BENCH_COPIES=n` repeats the sample's problems n times: a long contest.
//! `XCPC_BENCH_THREADS=1` compiles each time on a new thread, as a thread pool would.
//! `XCPC_TRACE=1 XCPC_TRACE_OUT=trace.json` also dumps a Chrome Trace / Perfetto flamegraph.

use std::time::Instant;

use serde_json::json;
use xcpc_engine::{Engine, FontOptions};

const SAMPLE: &str = include_str!("../tests/contest-zh.typ");

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

fn percentile(mut v: Vec<f64>, p: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f64::total_cmp);
    let idx = (((v.len() - 1) as f64) * p).round() as usize;
    v[idx.min(v.len() - 1)]
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn main() {
    let emit_json = std::env::args().any(|a| a == "--json");
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
    let first_compile_ms = ms(t);
    let pages = compiled.page_count();
    if !emit_json {
        println!("first compile        {first_compile_ms:7.1} ms, {pages} pages");
    }

    let mut edits_json = serde_json::Map::new();

    // Typing: one more character each time, in the first problem and in the last.
    for (name, key, anchor, last) in [
        ("edit early", "editEarly", "这是一道签到题", false),
        ("edit late", "editLate", "请使用 64 位整数", true),
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
        let p50_compile = percentile(compile.clone(), 0.50);
        let p95_compile = percentile(compile, 0.95);
        let p50_prints = percentile(prints, 0.50);
        let avg_changed = changed as f64 / 29.0;
        if !emit_json {
            println!(
                "{name:<20} compile p50 {p50_compile:5.1} ms  p95 {p95_compile:5.1} ms  fingerprints {p50_prints:4.2} ms  pages changed {avg_changed:.1}/{pages}"
            );
        }
        edits_json.insert(
            key.to_owned(),
            json!({
                "compileP50Ms": round2(p50_compile),
                "compileP95Ms": round2(p95_compile),
                "fingerprintsP50Ms": round2(p50_prints),
                "pagesChangedAvg": round2(avg_changed),
            }),
        );
    }

    let mut scales_json = serde_json::Map::new();
    if copies == 1 {
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
            let r_p50 = percentile(raster.clone(), 0.50);
            let r_p95 = percentile(raster, 0.95);
            let t_p50 = percentile(tile, 0.50);
            let p_p50 = percentile(png, 0.50);
            if !emit_json {
                println!(
                    "scale {scale:3}: raster/page p50 {r_p50:5.1} ms (p95 {r_p95:5.1} ms)   tile {t_p50:5.2} ms   png encode/page {p_p50:6.1} ms"
                );
            }
            scales_json.insert(
                format!("{scale:.1}"),
                json!({
                    "rasterP50Ms": round2(r_p50),
                    "rasterP95Ms": round2(r_p95),
                    "tileP50Ms": round2(t_p50),
                    "pngP50Ms": round2(p_p50),
                }),
            );
        }
    }

    xcpc_trace::flush_if_env_configured();

    if emit_json {
        let report = json!({
            "copies": copies,
            "pages": pages,
            "firstCompileMs": round2(first_compile_ms),
            "edits": edits_json,
            "scales": scales_json,
        });
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
    }
}
