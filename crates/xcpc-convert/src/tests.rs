use xcpc_domain::{ProblemMeta, Sample, Statement, TextFile};

use super::*;

fn problem(statement: &str, samples: &[(&str, &str)]) -> Problem {
    Problem {
        id: "p".into(),
        dir: "problems/p".into(),
        meta: ProblemMeta {
            title: "Two \"Sum\"".into(),
            limits: Limits {
                time: Some("2s".into()),
                memory: Some("256MB".into()),
                ..Default::default()
            },
            ..Default::default()
        },
        statements: vec![Statement {
            language: None,
            file: TextFile {
                path: "problems/p/statement.md".into(),
                text: statement.into(),
            },
        }],
        samples: samples
            .iter()
            .enumerate()
            .map(|(i, (a, b))| Sample {
                name: format!("{:02}", i + 1),
                input: (*a).into(),
                output: (*b).into(),
            })
            .collect(),
        editorial: None,
    }
}

fn body(statement: &str, samples: &[(&str, &str)]) -> Document {
    let p = problem(statement, samples);
    statements(&Book::single(Language::Chinese), &[Entry::single(&p)])
}

/// The generated problem body, without the head.
fn inner(doc: &Document) -> &str {
    let start = doc.source.find(")[\n").unwrap() + 3;
    doc.source[start..]
        .trim_end()
        .strip_suffix(']')
        .unwrap()
        .trim()
}

#[test]
fn sections_and_samples_after_input_and_output() {
    let doc = body(
        "给定 $n$。\n\n## 输入\n\n一行。\n\n## 输出格式\n\n一个数。\n\n## 提示\n\n无。\n",
        &[("1 2\n", "3\n")],
    );
    assert_eq!(
        inner(&doc),
        "给定 #m(\"n\");。\n\n#section(\"input\")\n\n一行。\n\n#section(\"output\")\n\n一个数。\n\n\
         #samples((\"1 2\\n\", \"3\\n\"),)\n\n#section(\"note\")\n\n无。"
    );
    assert!(
        doc.source
            .contains("time-limit: \"2 秒\", memory-limit: \"256 MB\"")
    );
    assert!(doc.source.contains("title: \"Two \\\"Sum\\\"\""));
}

#[test]
fn an_explicit_samples_heading_places_them() {
    let doc = body("描述。\n\n## 样例\n\n样例之后。\n", &[("a", "b")]);
    assert_eq!(
        inner(&doc),
        "描述。\n\n#samples((\"a\", \"b\"),)\n\n样例之后。"
    );
}

#[test]
fn samples_go_last_without_sections() {
    let doc = body("描述。\n", &[("a", "b")]);
    assert_eq!(inner(&doc), "描述。\n\n#samples((\"a\", \"b\"),)");
}

#[test]
fn unknown_headings_are_plain_sections() {
    let doc = body("## 题目描述\n\n描述。\n\n## 其他 $x$\n\n文字。\n", &[]);
    assert_eq!(
        inner(&doc),
        "描述。\n\n#section(none, title: [其他 #m(\"x\");])\n\n文字。"
    );
}

#[test]
fn text_is_escaped() {
    let doc = body("a\\*b\\* #x $5 <y> [z] // c = d\n\n1\\. not a list\n", &[]);
    assert_eq!(
        inner(&doc),
        "a\\*b\\* \\#x \\$5 \\<y\\> \\[z\\] \\/\\/ c \\= d\n\n1\\. not a list"
    );
}

#[test]
fn inline_structure_is_calls() {
    let doc = body("**粗**体、*斜*和 `code`，[链接](https://x.y)。\n", &[]);
    assert_eq!(
        inner(&doc),
        "#strong[粗];体、#emph[斜];和 #raw(\"code\");，#link(\"https://x.y\")[链接];。"
    );
}

#[test]
fn soft_breaks_vanish_between_cjk() {
    let doc = body("第一行\n第二行\nand English\nwords\n", &[]);
    assert_eq!(inner(&doc), "第一行第二行 and English words");
}

#[test]
fn lists_and_display_math() {
    let doc = body(
        "- 若 $x<y$；\n- 否则。\n\n$$\nq \\leq \\left\\lceil \\log_2 n \\right\\rceil\n$$\n",
        &[],
    );
    let s = inner(&doc);
    assert!(
        s.starts_with("#list([若 #m(\"x < y\");；], [否则。],)"),
        "{s}"
    );
    assert!(s.contains("#dm(\"q  <=  lr(⌈  log _(2 ) n  ⌉ )\");"), "{s}");
}

#[test]
fn plain_delimiters_do_not_scale() {
    let doc = body("$(a_i,b_i)$ 与 $[1,n]$\n", &[]);
    assert!(
        inner(&doc).contains("\\\\(a _(i )\\\\,b _(i )\\\\)"),
        "{}",
        inner(&doc)
    );
}

#[test]
fn pictures_map_into_the_problem() {
    let doc = body("![图](assets/a.png) ![坏](../q/b.png)\n", &[]);
    assert!(inner(&doc).starts_with("#box(image(\"/problems/p/assets/a.png\", alt: \"图\"));"));
    assert_eq!(doc.diagnostics.len(), 1);
    assert_eq!(
        (doc.diagnostics[0].line, doc.diagnostics[0].column),
        (1, 20)
    );
}

#[test]
fn bad_math_is_a_diagnostic_at_its_place() {
    let doc = body("第一行\n\n公式 $\\foo{x}$ 坏了\n", &[]);
    let d = &doc.diagnostics[0];
    assert!(d.error);
    assert_eq!(
        (d.file.as_str(), d.line, d.column),
        ("problems/p/statement.md", 3, 4)
    );
}

#[test]
fn the_source_map_points_back() {
    let doc = body("第一段。\n\n## 输入\n\n第二段 $n$。\n", &[]);
    let at = doc.source.find("第二段").unwrap();
    assert_eq!(doc.map.locate(at), Some(("problems/p/statement.md", 5, 1)));
    let at = doc.source.find("#m(\"n\")").unwrap();
    assert_eq!(doc.map.locate(at), Some(("problems/p/statement.md", 5, 5)));
    assert_eq!(doc.map.locate(0), None);
}

#[test]
fn contest_overrides_and_editorials() {
    let mut p = problem("描述。\n", &[]);
    p.editorial = Some(TextFile {
        path: "problems/p/editorial.md".into(),
        text: "## 做法\n\n贪心。\n\n### 引理 1\n\n成立。\n".into(),
    });
    p.meta.author = Some("某人".into());
    p.meta.rating = Some(1700);
    p.meta.tags = vec!["greedy".into()];
    p.meta.sources = vec![xcpc_domain::Source {
        platform: "codeforces".into(),
        id: "1A".into(),
        url: None,
    }];
    let q = problem("描述。\n", &[]);
    let meta = ContestMeta {
        name: "赛".into(),
        cover: Some(true),
        ..Default::default()
    };
    let item = |id: &str, label: &str| ContestItem {
        id: id.into(),
        label: label.into(),
        title: Some("新名".into()),
        show_metadata: Some(false),
        ..Default::default()
    };
    let entries = [
        Entry {
            item: item("p", "A"),
            problem: &p,
        },
        Entry {
            item: item("q", "B"),
            problem: &q,
        },
    ];
    let doc = statements(&Book::contest(&meta), &entries);
    assert!(
        doc.source
            .contains("#problem(label: \"B\", title: \"新名\"")
    );
    assert!(doc.source.contains("show-metadata: false)"));
    assert!(doc.source.contains("show-cover: true"));
    let doc = editorials(&Book::contest(&meta), &entries);
    // Author, tags and sources, not the rating; `##` a subsection, `###` run in.
    assert!(doc.source.contains(
        "#tutorial(label: \"A\", title: \"新名\", meta: (\"作者 某人\",\"greedy\",\"Codeforces 1A\",))[\n\
         #subsection[做法]\n\n贪心。\n\n#runin[引理 1];成立。\n]"
    ), "{}", doc.source);
    assert!(
        doc.source
            .contains("show-cover: true, blank-pages: false, book: \"editorial\"")
    );
    assert!(!doc.source.contains("label: \"B\""));
    assert_eq!(doc.diagnostics.len(), 1);
}

#[test]
fn a_single_problems_editorial_has_no_label_nor_name() {
    let mut p = problem("描述。\n", &[]);
    p.editorial = Some(TextFile {
        path: "problems/p/editorial.md".into(),
        text: "做法。\n".into(),
    });
    let doc = editorials(&Book::single(Language::Chinese), &[Entry::single(&p)]);
    assert!(
        doc.source.contains("contest.with(name: \"\""),
        "{}",
        doc.source
    );
    assert!(doc.source.contains("show-cover: false"));
    assert!(
        doc.source
            .contains("#tutorial(label: none, title: \"Two \\\"Sum\\\"\", meta: ())[\n做法。\n]"),
        "{}",
        doc.source
    );
}

#[test]
fn limits_read_as_the_template_writes_them() {
    let l = |t: &str, m: &str| Limits {
        time: Some(t.into()),
        memory: Some(m.into()),
        ..Default::default()
    };
    let s = |v: (Option<String>, Option<String>)| (v.0.unwrap(), v.1.unwrap());
    assert_eq!(
        s(limits(&l("1s", "256MB"), Language::Chinese)),
        ("1 秒".into(), "256 MB".into())
    );
    assert_eq!(
        s(limits(&l("1 s", "1024 MB"), Language::English)),
        ("1 second".into(), "1024 megabytes".into())
    );
    assert_eq!(
        s(limits(&l("2.5s", "512MiB"), Language::English)),
        ("2.5 seconds".into(), "512 mebibytes".into())
    );
    assert_eq!(
        s(limits(&l("500ms", "weird"), Language::Chinese)),
        ("500 毫秒".into(), "weird".into())
    );
}
