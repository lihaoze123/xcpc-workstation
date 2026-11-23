use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

use tempfile::TempDir;
use xcpc_domain::{
    ContestItem, ContestMeta, ExpectVerdict, Language, Limits, PackageConfig, ProblemMeta,
    SolutionSpec, TestSpec,
};
use xcpc_judge::{BuildMode, BuildOptions, build_package};
use xcpc_packages::{
    ExportOjOptions, ExportTarget, OjFormat, PolygonImportKind, PolygonImportOptions,
    export_oj_package, import_polygon,
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
    let warn_codes: Vec<&str> = res.warnings.iter().map(|w| w.code.as_str()).collect();
    assert!(warn_codes.contains(&"subtasks_preserved"));
    assert!(warn_codes.contains(&"interactive_not_supported"));
    assert!(warn_codes.contains(&"non_cpp_program"));
    assert!(warn_codes.contains(&"fallback_static_test"));
    assert!(warn_codes.contains(&"latex_fallback_typst"));

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
        },
    );
    solutions.insert(
        "wa".to_owned(),
        SolutionSpec {
            main: None,
            expect: Some(ExpectVerdict::Single("WA".to_owned())),
            tags: None,
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
    assert!(h_md.contains("![fig](file://fig.png)"));
    assert!(h_md.contains("```input1\n1 2\n```"));
    assert!(h_md.contains("```output1\n3\n```"));
    assert!(h_entries.contains_key("additional_file/fig.png"));
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
