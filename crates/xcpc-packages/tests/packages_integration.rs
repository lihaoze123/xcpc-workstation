use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use tempfile::TempDir;
use xcpc_domain::{
    ContestItem, ContestMeta, ExpectVerdict, Language, Limits, PackageConfig, ProblemMeta,
    SolutionSpec, TestSpec,
};
use xcpc_judge::{BuildMode, BuildOptions, build_package};
use xcpc_packages::{
    BatchExportEvent, BatchExportItemStatus, ExportOjOptions, ExportTarget, OjFormat,
    PolygonImportKind, PolygonImportOptions, export_oj_package, export_oj_packages_batch,
    import_polygon,
};
use xcpc_storage::{Workspace, init_workspace};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

fn create_zip(path: &Path, entries: &[(&str, &[u8])]) {
    let file = File::create(path).expect("create test zip");
    let mut writer = ZipWriter::new(file);
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, data) in entries {
        writer.start_file(*name, opts).expect("start zip entry");
        writer.write_all(data).expect("write zip entry");
    }
    writer.finish().expect("finish test zip");
}

fn read_zip_entries(path: &Path) -> BTreeMap<String, (Vec<u8>, Option<u32>)> {
    let file = File::open(path).expect("open output zip");
    let mut archive = ZipArchive::new(file).expect("read output zip");
    let mut map = BTreeMap::new();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).expect("zip entry");
        let name = entry.name().to_owned();
        let mode = entry.unix_mode();
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf).expect("read zip bytes");
        map.insert(name, (buf, mode));
    }
    map
}

fn build_synthetic_polygon_problem_entries(short_name: &str) -> Vec<(String, Vec<u8>)> {
    let problem_xml = format!(
        r#"<?xml version="1.0" encoding="utf-8" standalone="no"?>
<problem revision="3" short-name="{short_name}" url="https://polygon.codeforces.com/p/demo/{short_name}">
  <names>
    <name language="chinese" value="A 加 B"/>
    <name language="english" value="A Plus B"/>
  </names>
  <statements>
    <statement charset="UTF-8" language="chinese" mathjax="true" path="statements/chinese/problem.tex" type="application/x-tex"/>
    <statement charset="UTF-8" language="english" mathjax="true" path="statements/english/problem.tex" type="application/x-tex"/>
  </statements>
  <judging cpu-name="Intel" input-file="" output-file="" run-count="1">
    <testset name="tests">
      <time-limit>1500</time-limit>
      <memory-limit>268435456</memory-limit>
      <test-count>4</test-count>
      <input-path-pattern>tests/%02d</input-path-pattern>
      <answer-path-pattern>tests/%02d.a</answer-path-pattern>
      <groups>
        <group feedback-policy="complete" name="1" points="100" points-policy="complete-group"/>
      </groups>
      <tests>
        <test method="manual" sample="true" group="1"/>
        <test method="generated" cmd="gen_rand 10 42" group="1"/>
        <test method="manual" group="1"/>
        <test method="generated" cmd="gen_multi 5" from-file="2" group="1"/>
      </tests>
    </testset>
  </judging>
  <files>
    <executables>
      <executable>
        <source path="files/gen_rand.cpp" type="cpp.g++17"/>
      </executable>
      <executable>
        <source path="files/gen_py.py" type="python.3"/>
      </executable>
    </executables>
  </files>
  <assets>
    <checker name="std::rcmp6.cpp" type="testlib">
      <source path="files/check.cpp" type="cpp.g++17"/>
    </checker>
    <validators>
      <validator>
        <source path="files/val.cpp" type="cpp.g++17"/>
        <testset>
          <input-path-pattern>files/tests/validator-tests/%02d</input-path-pattern>
          <tests>
            <test verdict="valid"/>
            <test verdict="invalid"/>
          </tests>
        </testset>
      </validator>
    </validators>
    <solutions>
      <solution tag="main">
        <source path="solutions/std.cpp" type="cpp.g++17"/>
      </solution>
      <solution tag="wrong-answer">
        <source path="solutions/wa.cpp" type="cpp.g++17"/>
      </solution>
      <solution tag="time-limit-exceeded-or-accepted">
        <source path="solutions/slow.cpp" type="cpp.g++17"/>
      </solution>
      <solution tag="accepted">
        <source path="solutions/sol.py" type="python.3"/>
      </solution>
    </solutions>
    <interactor>
      <source path="files/interactor.cpp" type="cpp.g++17"/>
    </interactor>
  </assets>
</problem>
"#
    );

    vec![
        ("problem.xml".into(), problem_xml.into_bytes()),
        (
            "statement-sections/chinese/name.tex".into(),
            "A 加 B".as_bytes().to_vec(),
        ),
        (
            "statement-sections/chinese/legend.tex".into(),
            r#"给定两个整数 $a, b$，请计算 \textbf{它们的和}。
\includegraphics[width=5cm]{diagram.png}
\begin{tabular}{|c|c|}
\hline
1 & 2 \\
\hline
\end{tabular}
"#
            .as_bytes()
            .to_vec(),
        ),
        (
            "statement-sections/chinese/input.tex".into(),
            r#"一行两个整数 $a, b$（$1 \le a, b \le 10^9$）。"#.as_bytes().to_vec(),
        ),
        (
            "statement-sections/chinese/output.tex".into(),
            r#"输出一个整数表示 $a + b$。"#.as_bytes().to_vec(),
        ),
        (
            "statement-sections/chinese/notes.tex".into(),
            r#"\begin{itemize}\item 样例中 $1 + 2 = 3$。\end{itemize}"#
                .as_bytes()
                .to_vec(),
        ),
        (
            "statement-sections/chinese/diagram.png".into(),
            b"\x89PNG\r\n\x1a\nfake".to_vec(),
        ),
        (
            "statement-sections/english/legend.tex".into(),
            r#"Given $a$ and $b$, compute $a + b$."#.as_bytes().to_vec(),
        ),
        (
            "statement-sections/english/input.tex".into(),
            r#"Two integers $a, b$."#.as_bytes().to_vec(),
        ),
        (
            "statement-sections/english/output.tex".into(),
            r#"One integer $a + b$."#.as_bytes().to_vec(),
        ),
        ("tests/01".into(), b"1 2\n".to_vec()),
        ("tests/01.a".into(), b"3\n".to_vec()),
        ("tests/02".into(), b"10 20\n".to_vec()),
        ("tests/02.a".into(), b"30\n".to_vec()),
        ("tests/03".into(), b"5 7\n".to_vec()),
        ("tests/03.a".into(), b"12\n".to_vec()),
        ("tests/04".into(), b"9 9\n".to_vec()),
        ("tests/04.a".into(), b"18\n".to_vec()),
        (
            "files/gen_rand.cpp".into(),
            b"#include \"testlib.h\"\nint main(){}\n".to_vec(),
        ),
        ("files/gen_py.py".into(), b"print(1, 2)\n".to_vec()),
        (
            "files/val.cpp".into(),
            b"#include \"testlib.h\"\nint main(){}\n".to_vec(),
        ),
        ("files/tests/validator-tests/01".into(), b"1 2\n".to_vec()),
        ("files/tests/validator-tests/02".into(), b"-1 -2\n".to_vec()),
        ("files/interactor.cpp".into(), b"// interactor\n".to_vec()),
        ("solutions/std.cpp".into(), b"int main(){}\n".to_vec()),
        ("solutions/wa.cpp".into(), b"int main(){}\n".to_vec()),
        ("solutions/slow.cpp".into(), b"int main(){}\n".to_vec()),
        (
            "solutions/sol.py".into(),
            b"print(sum(map(int, input().split())))\n".to_vec(),
        ),
    ]
}

#[test]
fn polygon_single_problem_import_full() {
    let tmp = TempDir::new().unwrap();
    let ws = init_workspace(tmp.path().join("ws")).unwrap();
    let zip_path = tmp.path().join("a-plus-b.zip");

    let owned = build_synthetic_polygon_problem_entries("a-plus-b");
    let refs: Vec<(&str, &[u8])> = owned
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_slice()))
        .collect();
    create_zip(&zip_path, &refs);

    let res = import_polygon(&ws, &zip_path, &PolygonImportOptions::default()).unwrap();
    assert_eq!(res.kind, PolygonImportKind::Problem);
    assert_eq!(res.contest_id, None);
    assert_eq!(res.problems.len(), 1);

    let p = &res.problems[0];
    assert_eq!(p.id, "a-plus-b");
    assert_eq!(p.title, "A 加 B");
    assert_eq!(p.samples_count, 1);
    assert_eq!(p.tests_count, 3); // gen_rand + manual 03 + from-file static 04
    assert_eq!(p.checker, "rcmp6");
    assert!(p.has_validator);
    assert_eq!(p.assets_count, 1);

    // Check structured warnings emitted for unsupported/fallback features.
    // Phase 3C supports <groups> and <interactor>, so those Phase 3B placeholder warnings are gone.
    let warn_codes: Vec<&str> = res.warnings.iter().map(|w| w.code.as_str()).collect();
    assert!(!warn_codes.contains(&"subtasks_preserved"));
    assert!(!warn_codes.contains(&"interactive_not_supported"));
    assert!(warn_codes.contains(&"non_cpp_program"));
    assert!(warn_codes.contains(&"fallback_static_test"));
    assert!(warn_codes.contains(&"latex_fallback_typst"));
    assert!(p.has_interactor);
    assert_eq!(p.subtasks_count, 1);

    // Verify loaded problem metadata & files in workspace.
    let loaded = ws.load_problem("a-plus-b").unwrap();
    assert_eq!(loaded.meta.limits.time.as_deref(), Some("1.5s"));
    assert_eq!(loaded.meta.limits.memory.as_deref(), Some("256MB"));
    assert_eq!(
        loaded
            .meta
            .package
            .as_ref()
            .and_then(|p| p.checker.as_deref()),
        Some("rcmp6")
    );
    assert_eq!(loaded.samples.len(), 1);
    assert_eq!(loaded.samples[0].input, "1 2\n");
    assert_eq!(loaded.samples[0].output, "3\n");

    // Check declarative generator test + static fallbacks.
    assert_eq!(loaded.meta.test.len(), 3);
    assert_eq!(loaded.meta.test[0].generator.as_deref(), Some("gen_rand"));
    assert_eq!(loaded.meta.test[0].args.as_deref(), Some("10 42"));
    assert_eq!(loaded.meta.test[1].file.as_deref(), Some("tests/03.in"));
    assert_eq!(loaded.meta.test[2].file.as_deref(), Some("tests/04.in"));

    // Check solutions mapping (non-C++ sol.py preserved on disk, excluded from [solutions.*]).
    assert_eq!(loaded.meta.solutions["std"].main, Some(true));
    assert_eq!(
        loaded.meta.solutions["wa"].expect,
        Some(ExpectVerdict::Single("WA".into()))
    );
    assert_eq!(
        loaded.meta.solutions["slow"].expect,
        Some(ExpectVerdict::Multiple(vec!["TLE".into(), "AC".into()]))
    );
    assert!(!loaded.meta.solutions.contains_key("sol"));
    assert!(
        ws.resolve("problems/a-plus-b/solutions/sol.py")
            .unwrap()
            .is_file()
    );
    assert!(
        ws.resolve("problems/a-plus-b/interactor.cpp")
            .unwrap()
            .is_file()
    );
    assert!(
        ws.resolve("problems/a-plus-b/assets/diagram.png")
            .unwrap()
            .is_file()
    );
    assert!(
        ws.resolve("problems/a-plus-b/tests/validator/valid/01.in")
            .unwrap()
            .is_file()
    );
    assert!(
        ws.resolve("problems/a-plus-b/tests/validator/invalid/01.in")
            .unwrap()
            .is_file()
    );

    // Check Chinese statement Markdown conversion (math preserved, image rewritten, tabular -> ```typst).
    let zh_stmt = ws.read_text("problems/a-plus-b/statement.zh.md").unwrap();
    assert!(zh_stmt.contains("$a, b$"));
    assert!(zh_stmt.contains("**它们的和**"));
    assert!(zh_stmt.contains("![](assets/diagram.png)"));
    assert!(zh_stmt.contains("```typst"));
    assert!(zh_stmt.contains("## 输入格式"));
    assert!(zh_stmt.contains("## 输出格式"));
    assert!(zh_stmt.contains("## 提示"));

    // Slug conflict refusal & explicit target_id override.
    let err = import_polygon(&ws, &zip_path, &PolygonImportOptions::default()).unwrap_err();
    assert_eq!(err.code(), "target_exists");

    let res2 = import_polygon(
        &ws,
        &zip_path,
        &PolygonImportOptions {
            target_id: Some("a-plus-b-copy".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(res2.problems[0].id, "a-plus-b-copy");
}

#[test]
fn polygon_contest_import_expands_problems_and_contest_toml() {
    let tmp = TempDir::new().unwrap();
    let ws = init_workspace(tmp.path().join("ws")).unwrap();
    let zip_path = tmp.path().join("round-1.zip");

    let contest_xml = r#"<?xml version="1.0" encoding="utf-8"?>
<contest url="https://polygon.codeforces.com/c/1">
  <names>
    <name language="chinese" value="校赛第一场"/>
    <name language="english" value="Campus Round 1"/>
  </names>
  <problems>
    <problem index="A" path="problems/prob-a" short-name="prob-a"/>
    <problem index="B" path="problems/prob-b" short-name="prob-b"/>
  </problems>
</contest>
"#;

    let mut entries: Vec<(String, Vec<u8>)> = vec![("contest.xml".into(), contest_xml.into())];
    for (sub, sn) in [("problems/prob-a", "prob-a"), ("problems/prob-b", "prob-b")] {
        for (k, v) in build_synthetic_polygon_problem_entries(sn) {
            entries.push((format!("{sub}/{k}"), v));
        }
    }

    let refs: Vec<(&str, &[u8])> = entries
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_slice()))
        .collect();
    create_zip(&zip_path, &refs);

    let res = import_polygon(
        &ws,
        &zip_path,
        &PolygonImportOptions {
            target_id: Some("campus-r1".into()),
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(res.kind, PolygonImportKind::Contest);
    assert_eq!(res.contest_id.as_deref(), Some("campus-r1"));
    assert_eq!(res.problems.len(), 2);
    assert_eq!(res.problems[0].label.as_deref(), Some("A"));
    assert_eq!(res.problems[1].label.as_deref(), Some("B"));

    let contest = ws.load_contest("campus-r1").unwrap();
    assert_eq!(contest.meta.name, "校赛第一场");
    assert_eq!(contest.meta.problems.len(), 2);
    assert_eq!(contest.meta.problems[0].id, "prob-a");
    assert_eq!(contest.meta.problems[0].label, "A");
    assert_eq!(contest.meta.problems[1].id, "prob-b");
    assert_eq!(contest.meta.problems[1].label, "B");
}

#[test]
fn zip_slip_and_size_limit_are_rejected() {
    let tmp = TempDir::new().unwrap();
    let ws = init_workspace(tmp.path().join("ws")).unwrap();

    // 1. Zip Slip `../` traversal
    let evil_zip = tmp.path().join("evil.zip");
    create_zip(
        &evil_zip,
        &[
            ("problem.xml", b"<problem short-name=\"ok\"/>"),
            ("../../escaped.txt", b"pwned"),
        ],
    );
    let err = import_polygon(&ws, &evil_zip, &PolygonImportOptions::default()).unwrap_err();
    assert_eq!(err.code(), "zip_slip");
    assert!(!ws.resolve("problems/ok").unwrap().exists());

    // 2. Decompression budget exceeded
    let big_zip = tmp.path().join("big.zip");
    let big_blob = vec![b'A'; 4096];
    create_zip(
        &big_zip,
        &[
            ("problem.xml", b"<problem short-name=\"big\"/>"),
            ("tests/01", &big_blob),
        ],
    );
    let err = import_polygon(
        &ws,
        &big_zip,
        &PolygonImportOptions {
            max_uncompressed_bytes: Some(512),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert_eq!(err.code(), "archive_too_large");
}

/// Helper: creates a real compilable & strictly verifiable A+B problem in `ws`.
fn seed_verifiable_problem(ws: &Workspace, slug: &str, custom_checker: bool) {
    let checker_name = if custom_checker {
        "checker.cpp"
    } else {
        "rcmp6"
    };
    let mut solutions = BTreeMap::new();
    solutions.insert(
        "std".to_owned(),
        SolutionSpec {
            main: Some(true),
            expect: Some(ExpectVerdict::Single("AC".to_owned())),
            tags: None,
            score: None,
            subtasks: BTreeMap::new(),
        },
    );
    solutions.insert(
        "wa".to_owned(),
        SolutionSpec {
            main: None,
            expect: Some(ExpectVerdict::Single("WA".to_owned())),
            tags: None,
            score: None,
            subtasks: BTreeMap::new(),
        },
    );

    let meta = ProblemMeta {
        title: format!("Sum Problem {slug}"),
        tags: vec!["math".into(), "easy".into()],
        limits: Limits {
            time: Some("1s".into()),
            memory: Some("256MB".into()),
            ..Default::default()
        },
        package: Some(PackageConfig {
            checker: Some(checker_name.to_owned()),
            runs: None,
        }),
        test: vec![TestSpec {
            file: Some("tests/01.in".into()),
            ..Default::default()
        }],
        solutions,
        ..Default::default()
    };

    ws.save_problem_meta(slug, &meta).unwrap();
    ws.write(
        &format!("problems/{slug}/statement.zh.md"),
        b"Compute $a + b$.\n\n![fig](assets/fig.png)\n\n## \xE8\xBE\x93\xE5\x85\xA5\xE6\xA0\xBC\xE5\xBC\x8F\n\nTwo integers $a, b$.\n\n## \xE8\xBE\x93\xE5\x87\xBA\xE6\xA0\xBC\xE5\xBC\x8F\n\nOne integer $a + b$.\n",
    )
    .unwrap();
    ws.write(
        &format!("problems/{slug}/editorial.md"),
        b"Just print `a + b`.\n",
    )
    .unwrap();
    // 1x1 transparent PNG so Typst PDF compilation succeeds cleanly with image embedded.
    let png_1x1: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    ws.write(&format!("problems/{slug}/assets/fig.png"), png_1x1)
        .unwrap();

    ws.write(&format!("problems/{slug}/samples/01.in"), b"1 2\n")
        .unwrap();
    ws.write(&format!("problems/{slug}/samples/01.ans"), b"3\n")
        .unwrap();
    ws.write(&format!("problems/{slug}/tests/01.in"), b"10 20\n")
        .unwrap();

    let validator_cpp = br#"#include "testlib.h"
int main(int argc, char* argv[]) {
    registerValidation(argc, argv);
    inf.readInt(1, 1000, "a");
    inf.readSpace();
    inf.readInt(1, 1000, "b");
    inf.readEoln();
    inf.readEof();
}
"#;
    ws.write(&format!("problems/{slug}/validator.cpp"), validator_cpp)
        .unwrap();
    ws.write(
        &format!("problems/{slug}/tests/validator/valid/01.in"),
        b"3 4\n",
    )
    .unwrap();
    ws.write(
        &format!("problems/{slug}/tests/validator/invalid/01.in"),
        b"-5 4\n",
    )
    .unwrap();

    let std_cpp = br#"#include <iostream>
int main() {
    long long a, b;
    if (std::cin >> a >> b) std::cout << (a + b) << "\n";
}
"#;
    let wa_cpp = br#"#include <iostream>
int main() {
    long long a, b;
    if (std::cin >> a >> b) std::cout << (a + b + 1) << "\n";
}
"#;
    ws.write(&format!("problems/{slug}/solutions/std.cpp"), std_cpp)
        .unwrap();
    ws.write(&format!("problems/{slug}/solutions/wa.cpp"), wa_cpp)
        .unwrap();

    if custom_checker {
        let checker_cpp = br#"#include "testlib.h"
int main(int argc, char* argv[]) {
    registerTestlibCmd(argc, argv);
    long long ja = ans.readLong();
    long long pa = ouf.readLong();
    if (ja != pa) quitf(_wa, "expected %lld, found %lld", ja, pa);
    quitf(_ok, "ok");
}
"#;
        ws.write(&format!("problems/{slug}/checker.cpp"), checker_cpp)
            .unwrap();
        ws.write(&format!("problems/{slug}/tests/checker/ok-01/in"), b"1 2\n")
            .unwrap();
        ws.write(&format!("problems/{slug}/tests/checker/ok-01/out"), b"3\n")
            .unwrap();
        ws.write(&format!("problems/{slug}/tests/checker/ok-01/ans"), b"3\n")
            .unwrap();
        ws.write(&format!("problems/{slug}/tests/checker/wa-01/in"), b"1 2\n")
            .unwrap();
        ws.write(&format!("problems/{slug}/tests/checker/wa-01/out"), b"4\n")
            .unwrap();
        ws.write(&format!("problems/{slug}/tests/checker/wa-01/ans"), b"3\n")
            .unwrap();
    }
}

#[test]
fn export_strict_gate_and_domjudge_hydro_formats() {
    let tmp = TempDir::new().unwrap();
    let ws = init_workspace(tmp.path().join("ws")).unwrap();
    let cache_dir = tmp.path().join("judge-cache");

    seed_verifiable_problem(&ws, "sum-std", false);
    seed_verifiable_problem(&ws, "sum-custom", true);

    // 1. Strict gate rejects unverified problem.
    let dj_out = tmp.path().join("out/sum-std-dj.zip");
    let err = export_oj_package(
        &ws,
        &ExportTarget::Problem("sum-std".into()),
        &ExportOjOptions {
            format: OjFormat::Domjudge,
            output_path: dj_out.clone(),
            force: false,
            language: None,
        },
    )
    .unwrap_err();
    assert_eq!(err.code(), "not_verified");

    // 2. Even with `force: true`, missing built testdata is rejected.
    let err = export_oj_package(
        &ws,
        &ExportTarget::Problem("sum-std".into()),
        &ExportOjOptions {
            format: OjFormat::Hydro,
            output_path: tmp.path().join("out/sum-std-hydro.zip"),
            force: true,
            language: None,
        },
    )
    .unwrap_err();
    assert_eq!(err.code(), "missing_testdata");

    // 3. Strictly build & verify both problems via `xcpc-judge`.
    let build_opts = BuildOptions {
        mode: BuildMode::Verify,
        cache_dir: Some(cache_dir),
        ..Default::default()
    };
    let r1 = build_package(&ws, "sum-std", &build_opts, None, None).unwrap();
    assert!(r1.verified, "sum-std verify failed: {:?}", r1.gates);
    let r2 = build_package(&ws, "sum-custom", &build_opts, None, None).unwrap();
    assert!(r2.verified, "sum-custom verify failed: {:?}", r2.gates);

    // 4. DOMjudge export with standard checker (`rcmp6`).
    let res_dj_std = export_oj_package(
        &ws,
        &ExportTarget::Problem("sum-std".into()),
        &ExportOjOptions {
            format: OjFormat::Domjudge,
            output_path: dj_out.clone(),
            force: false,
            language: None,
        },
    )
    .unwrap();
    assert_eq!(res_dj_std.artifacts.len(), 1);
    assert!(res_dj_std.artifacts[0].verified);
    assert_eq!(res_dj_std.artifacts[0].sample_count, 1);
    assert_eq!(res_dj_std.artifacts[0].secret_count, 1);

    let dj_entries = read_zip_entries(&dj_out);
    let ini = String::from_utf8(dj_entries["domjudge-problem.ini"].0.clone()).unwrap();
    assert!(ini.contains("short-name = sum-std\n"));
    assert!(ini.contains("externalid = sum-std\n"));
    assert!(ini.contains("timelimit = 1.0\n"));
    assert!(!ini.contains("color"));

    let pyaml = String::from_utf8(dj_entries["problem.yaml"].0.clone()).unwrap();
    assert!(pyaml.contains("name: Sum Problem sum-std\n"));
    assert!(pyaml.contains("memory: 256\n"));
    assert!(pyaml.contains("validation: default\n"));
    assert!(pyaml.contains("validator_flags: float_tolerance 1e-6\n"));

    assert!(dj_entries["problem.pdf"].0.starts_with(b"%PDF-"));
    assert_eq!(dj_entries["data/sample/01.in"].0, b"1 2\n");
    assert_eq!(dj_entries["data/sample/01.ans"].0, b"3\n");
    assert_eq!(dj_entries["data/secret/01.in"].0, b"10 20\n");
    assert_eq!(dj_entries["data/secret/01.ans"].0, b"30\n");

    // Minimal-set leak check for DOMjudge.
    for key in dj_entries.keys() {
        assert!(!key.contains("validator.cpp"), "leaked {key}");
        assert!(!key.contains("solutions/"), "leaked {key}");
        assert!(!key.contains("generators/"), "leaked {key}");
        assert!(!key.contains("editorial"), "leaked {key}");
        assert!(!key.starts_with("tests/"), "leaked {key}");
        assert!(!key.starts_with("output_validators/"), "unexpected {key}");
    }

    // 5. DOMjudge export with custom `checker.cpp` (Kattis 42/43 adapter + embedded `testlib.h`).
    let dj_custom_out = tmp.path().join("out/sum-custom-dj.zip");
    export_oj_package(
        &ws,
        &ExportTarget::Problem("sum-custom".into()),
        &ExportOjOptions {
            format: OjFormat::Domjudge,
            output_path: dj_custom_out.clone(),
            force: false,
            language: None,
        },
    )
    .unwrap();
    let dj_c_entries = read_zip_entries(&dj_custom_out);
    let pyaml_c = String::from_utf8(dj_c_entries["problem.yaml"].0.clone()).unwrap();
    assert!(pyaml_c.contains("validation: custom\n"));
    assert!(!pyaml_c.contains("validator_flags"));
    assert!(dj_c_entries.contains_key("output_validators/checker/checker.cpp"));
    assert_eq!(
        dj_c_entries["output_validators/checker/testlib.h"].0,
        xcpc_judge::TESTLIB_H.as_bytes()
    );
    let run_sh =
        String::from_utf8(dj_c_entries["output_validators/checker/run"].0.clone()).unwrap();
    assert!(run_sh.contains("exit 42"));
    assert!(run_sh.contains("exit 43"));
    assert!(run_sh.contains("judgemessage.txt"));
    assert_eq!(
        dj_c_entries["output_validators/checker/build"]
            .1
            .map(|m| m & 0o777),
        Some(0o755)
    );
    assert_eq!(
        dj_c_entries["output_validators/checker/run"]
            .1
            .map(|m| m & 0o777),
        Some(0o755)
    );

    // 6. Hydro export (`rcmp6` -> `checker_type: testlib` + `checker.cpp`, NO `testlib.h`).
    let hydro_out = tmp.path().join("out/sum-std-hydro.zip");
    export_oj_package(
        &ws,
        &ExportTarget::Problem("sum-std".into()),
        &ExportOjOptions {
            format: OjFormat::Hydro,
            output_path: hydro_out.clone(),
            force: false,
            language: None,
        },
    )
    .unwrap();
    let h_entries = read_zip_entries(&hydro_out);
    let h_pyaml = String::from_utf8(h_entries["problem.yaml"].0.clone()).unwrap();
    assert!(h_pyaml.contains("title: Sum Problem sum-std\n"));
    assert!(h_pyaml.contains("  - math\n"));
    let h_md = String::from_utf8(h_entries["problem_zh.md"].0.clone()).unwrap();
    assert_eq!(h_md, "@[pdf](file://problem.pdf?noDisposition=1)\n");
    assert!(
        h_entries
            .get("additional_file/problem.pdf")
            .is_some_and(|(b, _)| b.starts_with(b"%PDF-")),
        "Hydro ZIP must contain compiled additional_file/problem.pdf"
    );
    let h_cfg = String::from_utf8(h_entries["testdata/config.yaml"].0.clone()).unwrap();
    assert!(h_cfg.contains("type: default\n"));
    assert!(h_cfg.contains("time: 1000ms\n"));
    assert!(h_cfg.contains("memory: 256m\n"));
    assert!(h_cfg.contains("checker_type: testlib\n"));
    assert!(h_cfg.contains("checker: checker.cpp\n"));
    assert!(h_entries.contains_key("testdata/checker.cpp"));
    assert_eq!(h_entries["testdata/01.in"].0, b"1 2\n");
    assert_eq!(h_entries["testdata/01.out"].0, b"3\n");
    assert_eq!(h_entries["testdata/02.in"].0, b"10 20\n");
    assert_eq!(h_entries["testdata/02.out"].0, b"30\n");

    // Minimal-set leak check for Hydro (including NO testlib.h!).
    for key in h_entries.keys() {
        assert!(
            !key.contains("testlib.h"),
            "Hydro must not bundle testlib.h: {key}"
        );
        assert!(!key.contains("validator.cpp"), "leaked {key}");
        assert!(!key.contains("solutions/"), "leaked {key}");
        assert!(!key.contains("generators/"), "leaked {key}");
        assert!(!key.contains("editorial"), "leaked {key}");
        assert!(!key.starts_with("tests/"), "leaked {key}");
    }

    // 7. Contest batch export (DOMjudge): `A-sum-std.zip` and `B-sum-custom.zip` with overrides.
    let contest_meta = ContestMeta {
        name: "Test Contest".into(),
        language: Language::Chinese,
        show_metadata: true,
        problems: vec![
            ContestItem {
                id: "sum-std".into(),
                label: "A".into(),
                title: Some("Overridden Title A".into()),
                ..Default::default()
            },
            ContestItem {
                id: "sum-custom".into(),
                label: "B".into(),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    ws.save_contest_meta("test-contest", &contest_meta).unwrap();

    let contest_out_dir = tmp.path().join("out/contest-dj");
    let res_contest = export_oj_package(
        &ws,
        &ExportTarget::Contest("test-contest".into()),
        &ExportOjOptions {
            format: OjFormat::Domjudge,
            output_path: contest_out_dir.clone(),
            force: false,
            language: None,
        },
    )
    .unwrap();
    assert_eq!(res_contest.artifacts.len(), 2);
    assert!(contest_out_dir.join("A-sum-std.zip").is_file());
    assert!(contest_out_dir.join("B-sum-custom.zip").is_file());

    let a_entries = read_zip_entries(&contest_out_dir.join("A-sum-std.zip"));
    let a_ini = String::from_utf8(a_entries["domjudge-problem.ini"].0.clone()).unwrap();
    assert!(a_ini.contains("short-name = A\n"));
    let a_yaml = String::from_utf8(a_entries["problem.yaml"].0.clone()).unwrap();
    assert!(a_yaml.contains("name: Overridden Title A\n"));
}

#[test]
fn polygon_fallback_problem_tex_and_custom_checker_self_tests() {
    let tmp = TempDir::new().unwrap();
    let ws = init_workspace(tmp.path().join("ws")).unwrap();
    let zip_path = tmp.path().join("fallback.zip");

    let problem_xml = r#"<?xml version="1.0" encoding="utf-8"?>
<problem short-name="fallback-prob">
  <names>
    <name language="english" value="Fallback Problem"/>
  </names>
  <statements>
    <statement language="english" path="statements/english/problem.tex" type="application/x-tex"/>
  </statements>
  <judging>
    <testset name="tests">
      <time-limit>2000</time-limit>
      <memory-limit>536870912</memory-limit>
      <input-path-pattern>tests/%02d</input-path-pattern>
      <answer-path-pattern>tests/%02d.a</answer-path-pattern>
      <tests>
        <test method="manual" sample="true"/>
      </tests>
    </testset>
  </judging>
  <assets>
    <checker type="testlib">
      <source path="files/my_checker.cpp" type="cpp.g++20"/>
      <testset>
        <input-path-pattern>files/tests/checker-tests/%02d</input-path-pattern>
        <output-path-pattern>files/tests/checker-tests/%02d.o</output-path-pattern>
        <answer-path-pattern>files/tests/checker-tests/%02d.a</answer-path-pattern>
        <tests>
          <test verdict="ok"/>
          <test verdict="wrong-answer"/>
          <test verdict="presentation-error"/>
        </tests>
      </testset>
    </checker>
  </assets>
</problem>
"#;
    let problem_tex = r#"\begin{problem}{Fallback Title}{standard input}{standard output}{2 seconds}{512 megabytes}
Find the sum of $x$ and $y$.
\InputFile
Two integers $x, y$.
\OutputFile
Print $x + y$.
\Examples
\Note
Easy!
\end{problem}
"#;

    create_zip(
        &zip_path,
        &[
            ("topdir/problem.xml", problem_xml.as_bytes()),
            (
                "topdir/statements/english/problem.tex",
                problem_tex.as_bytes(),
            ),
            ("topdir/tests/01", b"2 3\n"),
            ("topdir/tests/01.a", b"5\n"),
            ("topdir/files/my_checker.cpp", b"// custom checker\n"),
            ("topdir/files/tests/checker-tests/01", b"2 3\n"),
            ("topdir/files/tests/checker-tests/01.o", b"5\n"),
            ("topdir/files/tests/checker-tests/01.a", b"5\n"),
            ("topdir/files/tests/checker-tests/02", b"2 3\n"),
            ("topdir/files/tests/checker-tests/02.o", b"6\n"),
            ("topdir/files/tests/checker-tests/02.a", b"5\n"),
            ("topdir/files/tests/checker-tests/03", b"2 3\n"),
            ("topdir/files/tests/checker-tests/03.o", b"abc\n"),
            ("topdir/files/tests/checker-tests/03.a", b"5\n"),
        ],
    );

    let res = import_polygon(&ws, &zip_path, &PolygonImportOptions::default()).unwrap();
    assert_eq!(res.problems[0].id, "fallback-prob");
    assert_eq!(res.problems[0].checker, "checker.cpp");
    assert!(
        ws.resolve("problems/fallback-prob/checker.cpp")
            .unwrap()
            .is_file()
    );
    assert!(
        ws.resolve("problems/fallback-prob/tests/checker/ok-01/out")
            .unwrap()
            .is_file()
    );
    assert!(
        ws.resolve("problems/fallback-prob/tests/checker/wa-02/out")
            .unwrap()
            .is_file()
    );
    assert!(
        ws.resolve("problems/fallback-prob/tests/checker/pe-03/out")
            .unwrap()
            .is_file()
    );

    let en_stmt = ws
        .read_text("problems/fallback-prob/statement.en.md")
        .unwrap();
    assert!(en_stmt.contains("Find the sum of $x$ and $y$."));
    assert!(en_stmt.contains("## Input"));
    assert!(en_stmt.contains("## Output"));
    assert!(en_stmt.contains("## Note"));
}

#[test]
fn polygon_std_checker_without_cpp_suffix_maps_to_builtin() {
    let tmp = TempDir::new().unwrap();
    let ws = init_workspace(tmp.path().join("ws")).unwrap();
    let zip_path = tmp.path().join("std-no-ext.zip");

    let problem_xml = r#"<?xml version="1.0" encoding="utf-8"?>
<problem short-name="std-wcmp-no-ext">
  <names>
    <name language="english" value="Std Checker Test"/>
  </names>
  <judging>
    <testset name="tests">
      <time-limit>1000</time-limit>
      <memory-limit>268435456</memory-limit>
      <input-path-pattern>tests/%02d</input-path-pattern>
      <answer-path-pattern>tests/%02d.a</answer-path-pattern>
      <tests>
        <test method="manual" sample="true"/>
      </tests>
    </testset>
  </judging>
  <assets>
    <checker name="std::wcmp" type="testlib">
      <source path="files/check.cpp" type="cpp.g++17"/>
    </checker>
  </assets>
</problem>
"#;
    create_zip(
        &zip_path,
        &[
            ("problem.xml", problem_xml.as_bytes()),
            ("tests/01", b"1\n"),
            ("tests/01.a", b"1\n"),
            ("files/check.cpp", b"// std wcmp\n"),
        ],
    );

    let res = import_polygon(&ws, &zip_path, &PolygonImportOptions::default()).unwrap();
    assert_eq!(res.problems[0].checker, "wcmp");
    assert!(
        !ws.resolve("problems/std-wcmp-no-ext/checker.cpp")
            .unwrap()
            .exists(),
        "standard checker without .cpp suffix should map to builtin, not copy checker.cpp"
    );
}

#[test]
fn polygon_contest_xml_resolves_problems_by_url_slug() {
    let tmp = TempDir::new().unwrap();
    let ws = init_workspace(tmp.path().join("ws")).unwrap();
    let zip_path = tmp.path().join("contest-57383.zip");

    let contest_xml = r#"<?xml version="1.0" encoding="utf-8"?>
<contest url="https://polygon.codeforces.com/c/57383">
  <names>
    <name language="chinese" value="2026 选拔赛"/>
  </names>
  <problems>
    <problem index="a" url="https://polygon.codeforces.com/p90Lvc2/user/spiral-matrix"/>
  </problems>
</contest>
"#;
    let prob_xml = r#"<?xml version="1.0" encoding="utf-8"?>
<problem short-name="spiral-matrix" url="https://polygon.codeforces.com/p90Lvc2/user/spiral-matrix">
  <names><name language="chinese" value="回形矩阵"/></names>
  <judging><testset name="tests"><time-limit>1000</time-limit><memory-limit>268435456</memory-limit><tests/></testset></judging>
</problem>
"#;

    create_zip(
        &zip_path,
        &[
            ("contest.xml", contest_xml.as_bytes()),
            ("problems/spiral-matrix/problem.xml", prob_xml.as_bytes()),
            (
                "problems/spiral-matrix/statement-sections/chinese/legend.tex",
                "输出回形矩阵。".as_bytes(),
            ),
        ],
    );

    let res = import_polygon(&ws, &zip_path, &PolygonImportOptions::default()).unwrap();
    assert_eq!(res.kind, PolygonImportKind::Contest);
    assert_eq!(res.contest_id.as_deref(), Some("contest-57383"));
    assert_eq!(res.group.as_deref(), Some("2026 选拔赛"));
    assert_eq!(res.problems.len(), 1);
    assert_eq!(res.problems[0].id, "spiral-matrix");
    assert_eq!(res.problems[0].label.as_deref(), Some("A"));
}

#[test]
fn polygon_contest_import_auto_grouping_modes() {
    let tmp = TempDir::new().unwrap();
    let ws = init_workspace(tmp.path().join("ws")).unwrap();

    // Seed workspace.toml with an existing group and a comment to verify preservation.
    ws.write(
        "workspace.toml",
        b"# keep this comment\ngroups = [\"existing-group\"]\n",
    )
    .unwrap();

    let make_contest_zip = |name: &str, contest_title: &str, prob_slug: &str| {
        let zip_path = tmp.path().join(name);
        let contest_xml = format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<contest>
  <names>
    <name language="chinese" value="{contest_title}"/>
  </names>
  <problems>
    <problem index="A" path="problems/{prob_slug}"/>
  </problems>
</contest>
"#
        );
        let prob_xml = format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<problem short-name="{prob_slug}">
  <names><name language="chinese" value="题 {prob_slug}"/></names>
  <judging><testset name="tests"><time-limit>1000</time-limit><memory-limit>268435456</memory-limit><tests/></testset></judging>
</problem>
"#
        );
        create_zip(
            &zip_path,
            &[
                ("contest.xml", contest_xml.as_bytes()),
                (
                    &format!("problems/{prob_slug}/problem.xml"),
                    prob_xml.as_bytes(),
                ),
            ],
        );
        zip_path
    };

    // 1. Default auto-grouping collapses whitespace and registers in workspace.toml.
    let zip1 = make_contest_zip("c1.zip", "  2026   春季  热身赛  ", "p-auto");
    let res1 = import_polygon(
        &ws,
        &zip1,
        &PolygonImportOptions {
            target_id: Some("c-auto".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(res1.group.as_deref(), Some("2026 春季 热身赛"));
    assert_eq!(
        ws.load_problem("p-auto").unwrap().meta.group.as_deref(),
        Some("2026 春季 热身赛")
    );
    assert_eq!(
        ws.workspace_groups(),
        vec!["existing-group".to_owned(), "2026 春季 热身赛".to_owned()]
    );
    let ws_toml = std::fs::read_to_string(ws.resolve("workspace.toml").unwrap()).unwrap();
    assert!(
        ws_toml.contains("# keep this comment"),
        "workspace.toml comment should be preserved: {ws_toml}"
    );

    // 2. `no_auto_group: true` leaves problems ungrouped.
    let zip2 = make_contest_zip("c2.zip", "不自动分组比赛", "p-noauto");
    let res2 = import_polygon(
        &ws,
        &zip2,
        &PolygonImportOptions {
            target_id: Some("c-noauto".into()),
            no_auto_group: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(res2.group, None);
    assert_eq!(ws.load_problem("p-noauto").unwrap().meta.group, None);

    // 3. Explicit `group` overrides contest name (even if `no_auto_group` is set).
    let zip3 = make_contest_zip("c3.zip", "原比赛名", "p-explicit");
    let res3 = import_polygon(
        &ws,
        &zip3,
        &PolygonImportOptions {
            target_id: Some("c-explicit".into()),
            group: Some("  省赛候选  ".into()),
            no_auto_group: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(res3.group.as_deref(), Some("省赛候选"));
    assert_eq!(
        ws.load_problem("p-explicit").unwrap().meta.group.as_deref(),
        Some("省赛候选")
    );
    assert!(ws.workspace_groups().contains(&"省赛候选".to_owned()));

    // 4. Single-problem import supports explicit `group` and rejects invalid group names.
    let single_zip = tmp.path().join("single.zip");
    let single_entries = build_synthetic_polygon_problem_entries("single-grouped");
    let single_refs: Vec<(&str, &[u8])> = single_entries
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_slice()))
        .collect();
    create_zip(&single_zip, &single_refs);

    let bad_err = import_polygon(
        &ws,
        &single_zip,
        &PolygonImportOptions {
            group: Some("bad\nname".into()),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert_eq!(bad_err.code(), "invalid_group_name");
    assert!(ws.load_problem("single-grouped").is_err());

    let ok_single = import_polygon(
        &ws,
        &single_zip,
        &PolygonImportOptions {
            group: Some("单题分组".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(ok_single.group.as_deref(), Some("单题分组"));
    assert_eq!(
        ws.load_problem("single-grouped")
            .unwrap()
            .meta
            .group
            .as_deref(),
        Some("单题分组")
    );
    assert!(ws.workspace_groups().contains(&"单题分组".to_owned()));
}

#[test]
fn batch_export_oj_mixed_gates_cancellation_and_readonly() {
    let tmp = TempDir::new().unwrap();
    let ws = init_workspace(tmp.path().join("ws")).unwrap();

    seed_verifiable_problem(&ws, "pkg-verified", false);
    seed_verifiable_problem(&ws, "pkg-unverified", false);

    // Pure publishing problem (no package files).
    ws.create_problem("pure-stmt", "纯题面").unwrap();
    ws.write(
        "problems/pure-stmt/statement.md",
        b"Just a statement.\n\n## Input\n\nx\n\n## Output\n\ny\n",
    )
    .unwrap();

    // Strictly verify only `pkg-verified`.
    let build_opts = BuildOptions {
        mode: BuildMode::Verify,
        cache_dir: Some(tmp.path().join("cache")),
        ..Default::default()
    };
    let build_res = build_package(&ws, "pkg-verified", &build_opts, None, None).unwrap();
    assert!(
        build_res.verified,
        "pkg-verified failed: {:?}",
        build_res.gates
    );

    // 1. Mixed batch: verified + duplicate + unverified + pure publishing + missing.
    let out_dir = tmp.path().join("batch-out");
    let events = Arc::new(Mutex::new(Vec::<BatchExportEvent>::new()));
    let events_cb = Arc::clone(&events);
    let slugs = vec![
        "pkg-verified".to_owned(),
        "pkg-verified".to_owned(), // duplicate should be deduplicated
        "pkg-unverified".to_owned(),
        "pure-stmt".to_owned(),
        "missing-prob".to_owned(),
    ];
    let opts = ExportOjOptions {
        format: OjFormat::Domjudge,
        output_path: out_dir.clone(),
        force: false,
        language: None,
    };
    let report = export_oj_packages_batch(
        &ws,
        &slugs,
        &opts,
        None,
        Some(&move |ev| {
            events_cb.lock().unwrap().push(ev);
        }),
    )
    .unwrap();

    assert!(!report.ok, "missing-prob failed so ok should be false");
    assert!(!report.cancelled);
    assert_eq!(report.total, 4);
    assert_eq!(report.exported, 1);
    assert_eq!(report.skipped, 2);
    assert_eq!(report.failed, 1);
    assert_eq!(report.cancelled_count, 0);

    assert_eq!(report.items[0].problem_id, "pkg-verified");
    assert_eq!(report.items[0].status, BatchExportItemStatus::Exported);
    assert!(out_dir.join("pkg-verified.zip").is_file());
    let zip_entries = read_zip_entries(&out_dir.join("pkg-verified.zip"));
    assert!(zip_entries.contains_key("domjudge-problem.ini"));
    assert!(zip_entries.contains_key("problem.pdf"));

    assert_eq!(report.items[1].problem_id, "pkg-unverified");
    assert_eq!(report.items[1].status, BatchExportItemStatus::Skipped);
    assert_eq!(report.items[1].code.as_deref(), Some("not_verified"));

    assert_eq!(report.items[2].problem_id, "pure-stmt");
    assert_eq!(report.items[2].status, BatchExportItemStatus::Skipped);
    assert_eq!(report.items[2].code.as_deref(), Some("not_a_package"));

    assert_eq!(report.items[3].problem_id, "missing-prob");
    assert_eq!(report.items[3].status, BatchExportItemStatus::Failed);
    assert_eq!(report.items[3].code.as_deref(), Some("not_found"));

    // Verify event ordering & serde `kind` tag.
    let evs = events.lock().unwrap();
    assert!(matches!(
        evs.first(),
        Some(BatchExportEvent::ProblemStarted { index: 0, .. })
    ));
    assert!(matches!(
        evs.last(),
        Some(BatchExportEvent::AllFinished {
            exported: 1,
            skipped: 2,
            failed: 1,
            cancelled: 0,
            total: 4,
            ..
        })
    ));
    let ev_json = serde_json::to_string(evs.last().unwrap()).unwrap();
    assert!(ev_json.contains(r#""kind":"all_finished""#));

    // 2. Cancellation between problems via AtomicBool.
    let cancel_flag = Arc::new(AtomicBool::new(false));
    let cancel_trigger = Arc::clone(&cancel_flag);
    let cancel_out = tmp.path().join("batch-cancel");
    let cancel_report = export_oj_packages_batch(
        &ws,
        &[
            "pure-stmt".to_owned(),
            "pkg-verified".to_owned(),
            "pkg-unverified".to_owned(),
        ],
        &ExportOjOptions {
            format: OjFormat::Hydro,
            output_path: cancel_out,
            force: false,
            language: None,
        },
        Some(cancel_flag),
        Some(&move |ev| {
            if matches!(ev, BatchExportEvent::ProblemFinished { index: 0, .. }) {
                cancel_trigger.store(true, Ordering::Relaxed);
            }
        }),
    )
    .unwrap();
    assert!(cancel_report.cancelled);
    assert!(!cancel_report.ok);
    assert_eq!(cancel_report.skipped, 1);
    assert_eq!(cancel_report.cancelled_count, 2);
    assert_eq!(
        cancel_report.items[1].status,
        BatchExportItemStatus::Cancelled
    );
    assert_eq!(
        cancel_report.items[2].status,
        BatchExportItemStatus::Cancelled
    );

    // 3. Read-only workspace is rejected upfront.
    let ro_ws = Workspace::open(ws.root(), true).unwrap();
    let ro_err = export_oj_packages_batch(&ro_ws, &["pkg-verified".to_owned()], &opts, None, None)
        .unwrap_err();
    assert_eq!(ro_err.code(), "read_only");
}

#[test]
fn polygon_import_interactive_multirun_and_subtasks() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = init_workspace(tmp.path().join("ws").as_path()).unwrap();
    let zip_path = tmp.path().join("poly-3c.zip");

    let problem_xml = r#"<?xml version="1.0" encoding="utf-8"?>
<problem short-name="guess-num" url="https://polygon.codeforces.com/p/guess-num">
  <names>
    <name language="chinese" value="猜数交互"/>
  </names>
  <judging input-file="" output-file="" run-count="2">
    <testset name="tests">
      <time-limit>1000</time-limit>
      <memory-limit>268435456</memory-limit>
      <test-count>4</test-count>
      <input-path-pattern>tests/%02d</input-path-pattern>
      <answer-path-pattern>tests/%02d.a</answer-path-pattern>
      <tests>
        <test method="manual" sample="true" group="0"/>
        <test method="generated" cmd="gen_small 1" group="1"/>
        <test method="generated" cmd="gen_full 1" group="2"/>
        <test method="manual" group="2"/>
      </tests>
      <groups>
        <group name="0" feedback-policy="complete" points-policy="complete-group" points="0"/>
        <group name="1" feedback-policy="icpc" points-policy="complete-group" points="30"/>
        <group name="2" feedback-policy="points" points-policy="each-test" points="70">
          <dependencies>
            <dependency group="0"/>
            <dependency group="1"/>
          </dependencies>
        </group>
      </groups>
    </testset>
  </judging>
  <files>
    <executables>
      <executable><source path="files/gen_small.cpp" type="cpp.g++20"/></executable>
      <executable><source path="files/gen_full.cpp" type="cpp.g++20"/></executable>
    </executables>
  </files>
  <assets>
    <interactor>
      <source path="files/my_interactor.cpp" type="cpp.g++20"/>
    </interactor>
    <solutions>
      <solution tag="main"><source path="solutions/std.cpp" type="cpp.g++20"/></solution>
    </solutions>
  </assets>
</problem>"#;

    let files: Vec<(&str, &[u8])> = vec![
        ("problem.xml", problem_xml.as_bytes()),
        (
            "statement-sections/chinese/legend.tex",
            b"Guess the hidden number.",
        ),
        (
            "statement-sections/chinese/interaction.tex",
            b"Print your guess and flush.",
        ),
        ("tests/01", b"5\n"),
        ("tests/01.a", b"5\n"),
        ("tests/04", b"42\n"),
        ("files/gen_small.cpp", b"int main(){}\n"),
        ("files/gen_full.cpp", b"int main(){}\n"),
        ("files/my_interactor.cpp", b"// cpp interactor\n"),
        ("solutions/std.cpp", b"int main(){}\n"),
    ];
    create_zip(&zip_path, &files);

    let res = import_polygon(&ws, &zip_path, &PolygonImportOptions::default()).unwrap();
    assert_eq!(res.problems.len(), 1);
    let p = &res.problems[0];
    assert_eq!(p.id, "guess-num");
    assert!(p.has_interactor);
    assert_eq!(p.runs, Some(2));
    assert_eq!(p.subtasks_count, 2);

    let warn_codes: Vec<&str> = res.warnings.iter().map(|w| w.code.as_str()).collect();
    assert!(!warn_codes.contains(&"subtasks_preserved"));
    assert!(!warn_codes.contains(&"interactive_not_supported"));

    // Interactor is normalized to `interactor.cpp`.
    assert!(
        ws.resolve("problems/guess-num/interactor.cpp")
            .unwrap()
            .is_file()
    );

    let loaded = ws.load_problem("guess-num").unwrap();
    assert_eq!(loaded.meta.run_count(), 2);
    assert_eq!(loaded.meta.package.as_ref().unwrap().checker, None);
    assert_eq!(loaded.meta.subtask.len(), 2);
    assert_eq!(loaded.meta.subtask[0].id, "1");
    assert_eq!(loaded.meta.subtask[0].score, 30);
    assert_eq!(
        loaded.meta.subtask[0].scoring,
        xcpc_domain::SubtaskScoring::Min
    );
    assert!(loaded.meta.subtask[0].depends.is_empty());

    assert_eq!(loaded.meta.subtask[1].id, "2");
    assert_eq!(loaded.meta.subtask[1].score, 70);
    assert_eq!(
        loaded.meta.subtask[1].scoring,
        xcpc_domain::SubtaskScoring::Sum
    );
    // Dropped sample-only group "0" should be pruned from depends; "1" remains.
    assert_eq!(loaded.meta.subtask[1].depends, vec!["1".to_owned()]);

    assert_eq!(loaded.meta.test.len(), 3);
    assert_eq!(loaded.meta.test[0].subtask.as_deref(), Some("1"));
    assert_eq!(loaded.meta.test[1].subtask.as_deref(), Some("2"));
    assert_eq!(loaded.meta.test[2].subtask.as_deref(), Some("2"));
}

#[test]
fn export_hydro_and_domjudge_interactive_subtasks_and_runs_warnings() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = init_workspace(tmp.path().join("ws").as_path()).unwrap();
    let slug = "inter-oi";

    // Build an interactive + multi-run (runs=2) + OI subtasks problem.
    let mut solutions = BTreeMap::new();
    solutions.insert(
        "std".to_owned(),
        SolutionSpec {
            main: Some(true),
            expect: Some(ExpectVerdict::Single("AC".to_owned())),
            tags: None,
            score: None,
            subtasks: BTreeMap::new(),
        },
    );
    let meta = ProblemMeta {
        title: "Interactive Subtasks".to_owned(),
        limits: Limits {
            time: Some("1s".into()),
            memory: Some("256MB".into()),
            ..Default::default()
        },
        package: Some(PackageConfig {
            checker: None,
            runs: Some(2),
        }),
        subtask: vec![
            xcpc_domain::SubtaskSpec {
                id: "1".to_owned(),
                score: 30,
                depends: vec![],
                scoring: xcpc_domain::SubtaskScoring::Min,
            },
            xcpc_domain::SubtaskSpec {
                id: "2".to_owned(),
                score: 70,
                depends: vec!["1".to_owned()],
                scoring: xcpc_domain::SubtaskScoring::Sum,
            },
        ],
        test: vec![
            TestSpec {
                file: Some("tests/01.in".into()),
                subtask: Some("1".into()),
                ..Default::default()
            },
            TestSpec {
                file: Some("tests/02.in".into()),
                subtask: Some("2".into()),
                ..Default::default()
            },
        ],
        solutions,
        ..Default::default()
    };
    ws.save_problem_meta(slug, &meta).unwrap();
    ws.write(
        &format!("problems/{slug}/statement.zh.md"),
        "交互题测试。\n\n## 交互方式\n\n读入一个数并输出它。\n\n## 子任务\n\n子任务 1 30 分，子任务 2 70 分。\n"
            .as_bytes(),
    )
    .unwrap();
    ws.write(&format!("problems/{slug}/tests/01.in"), b"7\n")
        .unwrap();
    ws.write(&format!("problems/{slug}/tests/02.in"), b"42\n")
        .unwrap();
    ws.write(
        &format!("problems/{slug}/validator.cpp"),
        br#"#include "testlib.h"
int main(int argc, char* argv[]) {
    registerValidation(argc, argv);
    inf.readInt(1, 100, "x");
    inf.readEoln();
    inf.readEof();
    return 0;
}
"#,
    )
    .unwrap();
    ws.write(
        &format!("problems/{slug}/tests/validator/valid/01.in"),
        b"5\n",
    )
    .unwrap();
    ws.write(
        &format!("problems/{slug}/tests/validator/invalid/01.in"),
        b"999\n",
    )
    .unwrap();
    ws.write(
        &format!("problems/{slug}/interactor.cpp"),
        br#"#include "testlib.h"
#include <iostream>
int main(int argc, char* argv[]) {
    registerInteraction(argc, argv);
    int x = inf.readInt();
    std::cout << x << std::endl;
    int y = ouf.readInt();
    if (y != x) quitf(_wa, "expected %d, got %d", x, y);
    tout << x << "\n";
    quitf(_ok, "ok");
}
"#,
    )
    .unwrap();
    ws.write(
        &format!("problems/{slug}/solutions/std.cpp"),
        br#"#include <iostream>
int main() {
    int x;
    if (std::cin >> x) std::cout << x << std::endl;
    return 0;
}
"#,
    )
    .unwrap();

    let build_res = xcpc_judge::build_package(
        &ws,
        slug,
        &BuildOptions {
            mode: BuildMode::Verify,
            ..Default::default()
        },
        None,
        None,
    )
    .unwrap();
    assert!(build_res.verified, "gates={:?}", build_res.gates);

    // 1. Hydro export: interactive + subtasks + runs warning (no checker warning).
    let hydro_zip = tmp.path().join("inter-oi-hydro.zip");
    let hydro_res = export_oj_package(
        &ws,
        &ExportTarget::Problem(slug.to_owned()),
        &ExportOjOptions {
            format: OjFormat::Hydro,
            output_path: hydro_zip.clone(),
            force: false,
            language: None,
        },
    )
    .unwrap();
    let hydro_warns: Vec<&str> = hydro_res.warnings.iter().map(|w| w.code.as_str()).collect();
    assert!(
        hydro_warns.contains(&"hydro_multi_run_unsupported"),
        "{hydro_warns:?}"
    );
    assert!(!hydro_warns.contains(&"hydro_interactive_checker_ignored"));

    let hydro_entries = read_zip_entries(&hydro_zip);
    assert!(hydro_entries.contains_key("testdata/interactor.cpp"));
    assert!(hydro_entries.contains_key("testdata/testlib.h"));
    let cfg = String::from_utf8(hydro_entries["testdata/config.yaml"].0.clone()).unwrap();
    assert!(cfg.contains("type: interactive\n"), "{cfg}");
    assert!(cfg.contains("interactor: interactor.cpp\n"), "{cfg}");
    assert!(cfg.contains("subtasks:\n"), "{cfg}");
    assert!(cfg.contains("  - score: 30\n    type: min\n"), "{cfg}");
    assert!(
        cfg.contains("  - score: 70\n    type: sum\n    if:\n      - 1\n"),
        "{cfg}"
    );
    assert!(
        cfg.contains("input: 01.in\n        output: 01.out\n"),
        "{cfg}"
    );
    assert!(
        cfg.contains("input: 02.in\n        output: 02.out\n"),
        "{cfg}"
    );

    // 2. DOMjudge export: also add a standalone checker.cpp to verify explicit checker warning.
    ws.write(
        &format!("problems/{slug}/checker.cpp"),
        b"// dummy standalone checker\n",
    )
    .unwrap();
    let dj_zip = tmp.path().join("inter-oi-dj.zip");
    let dj_res = export_oj_package(
        &ws,
        &ExportTarget::Problem(slug.to_owned()),
        &ExportOjOptions {
            format: OjFormat::Domjudge,
            output_path: dj_zip.clone(),
            force: true,
            language: None,
        },
    )
    .unwrap();
    let dj_warns: Vec<&str> = dj_res.warnings.iter().map(|w| w.code.as_str()).collect();
    assert!(
        dj_warns.contains(&"domjudge_interactive_checker_ignored"),
        "{dj_warns:?}"
    );
    assert!(
        dj_warns.contains(&"domjudge_multi_run_unsupported"),
        "{dj_warns:?}"
    );
    assert!(
        dj_warns.contains(&"domjudge_subtasks_flattened"),
        "{dj_warns:?}"
    );

    let dj_entries = read_zip_entries(&dj_zip);
    let prob_yaml = String::from_utf8(dj_entries["problem.yaml"].0.clone()).unwrap();
    assert!(
        prob_yaml.contains("validation: custom interactive\n"),
        "{prob_yaml}"
    );
    assert!(dj_entries.contains_key("output_validators/interactor/interactor.cpp"));
    assert!(dj_entries.contains_key("output_validators/interactor/testlib.h"));
    assert!(dj_entries.contains_key("output_validators/interactor/build"));
    let run_sh =
        String::from_utf8(dj_entries["output_validators/interactor/run"].0.clone()).unwrap();
    assert!(run_sh.contains("exit 42"), "{run_sh}");
    assert!(run_sh.contains("exit 43"), "{run_sh}");
    assert!(run_sh.contains("-eq 4"), "{run_sh}");
    assert!(run_sh.contains("-eq 8"), "{run_sh}");
    assert!(dj_entries.contains_key("data/secret/01.in"));
    assert!(dj_entries.contains_key("data/secret/02.in"));
}
