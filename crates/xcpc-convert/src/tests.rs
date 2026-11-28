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
    let start = doc.source.find("body: [\n").unwrap() + "body: [\n".len();
    let end = doc.source[start..].find("\n],\n").unwrap();
    doc.source[start..start + end].trim()
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
    assert!(doc.source.contains("time-limit: \"2 秒\""));
    assert!(doc.source.contains("memory-limit: \"256 MB\""));
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
    assert!(doc.diagnostics[0].error);
    assert_eq!(
        doc.diagnostics[0].code.as_deref(),
        Some(xcpc_lint::code::MISSING_IMAGE)
    );
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
fn source_map_unmarks_generated_wrappers_between_problems() {
    let mut a = problem("题 A 第一行。\n", &[]);
    a.id = "pa".into();
    a.dir = "problems/pa".into();
    a.statements[0].file.path = "problems/pa/statement.md".into();

    let mut b = problem("题 B 第一行。\n", &[]);
    b.id = "pb".into();
    b.dir = "problems/pb".into();
    b.meta.title = "UniqueTitleB".into();
    b.statements[0].file.path = "problems/pb/statement.md".into();

    let meta = ContestMeta {
        name: "C".into(),
        ..Default::default()
    };
    let entries = [
        Entry {
            item: ContestItem {
                id: "pa".into(),
                label: "A".into(),
                ..Default::default()
            },
            problem: &a,
        },
        Entry {
            item: ContestItem {
                id: "pb".into(),
                label: "B".into(),
                ..Default::default()
            },
            problem: &b,
        },
    ];
    let doc = statements(&Book::contest(&meta), &entries);
    let at_a = doc.source.find("题 A 第一行").unwrap();
    let at_b_header = doc.source.find("UniqueTitleB").unwrap();
    let at_b = doc.source.find("题 B 第一行").unwrap();
    assert_eq!(
        doc.map.locate(at_a),
        Some(("problems/pa/statement.md", 1, 1))
    );
    assert_eq!(doc.map.locate(at_b_header), None);
    assert_eq!(
        doc.map.locate(at_b),
        Some(("problems/pb/statement.md", 1, 1))
    );
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
    assert!(doc.source.contains("label: \"B\",\ntitle: \"新名\","));
    assert!(doc.source.contains("\"show-metadata\": false"));
    assert!(doc.source.contains("book: \"statements\","));
    let doc = editorials(&Book::contest(&meta), &entries);
    // Author, tags and sources, not the rating; `##` a subsection, `###` run in.
    assert!(
        doc.source.contains(
            "meta: (\"作者 某人\", \"greedy\", \"Codeforces 1A\",),\nbody: [\n\
             #subsection[做法]\n\n贪心。\n\n#runin[引理 1];成立。\n],"
        ),
        "{}",
        doc.source
    );
    assert!(doc.source.contains("book: \"editorial\","));
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
        doc.source.contains("#__xcpc_tpl.render((\nname: \"\","),
        "{}",
        doc.source
    );
    assert!(doc.source.contains("single: true,"));
    assert!(
        doc.source
            .contains("label: none,\ntitle: \"Two \\\"Sum\\\"\",\n")
            && doc.source.contains("meta: (),\nbody: [\n做法。\n],"),
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

#[test]
fn phase7a_plugin_template_render_eject_missing_and_frozen_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let ws_root = dir.path().join("ws");
    let ws = xcpc_storage::init_workspace(&ws_root).unwrap();
    ws.create_problem("a-plus-b", "A + B").unwrap();
    ws.write(
        "problems/a-plus-b/statement.md",
        b"Given $a$ and $b$, output $a+b$.\n\n## Input\nTwo integers.\n\n## Output\nOne integer.\n",
    )
    .unwrap();
    ws.write("problems/a-plus-b/samples/01.in", b"1 2\n")
        .unwrap();
    ws.write("problems/a-plus-b/samples/01.ans", b"3\n")
        .unwrap();

    // Create a mock template plugin directory with a helper module + SVG asset.
    let plugin_root = dir.path().join("plugin-pack");
    std::fs::create_dir_all(plugin_root.join("templates")).unwrap();
    std::fs::create_dir_all(plugin_root.join("assets")).unwrap();
    std::fs::write(
        plugin_root.join("assets/logo.svg"),
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16" fill="#0B6989"/></svg>"##,
    )
    .unwrap();
    std::fs::write(
        plugin_root.join("templates/helper.typ"),
        "#let badge() = image(\"../assets/logo.svg\", height: 10pt)\n",
    )
    .unwrap();
    std::fs::write(
        plugin_root.join("templates/ec.typ"),
        r##"
#import "/templates/defaults.typ": section, samples, subsection, runin
#import "/xcpc/fonts.typ": lm-roman-10, cjk-song
#import "./helper.typ": badge

#let schema = (
  kind: "statement",
  name: (zh: "EC 模板", en: "EC Template"),
  fields: (
    (key: "show-logo", label: "Logo", type: "bool", scope: "book", default: true),
  ),
)

#let render(doc) = {
  set page(paper: "a4")
  set text(font: (lm-roman-10, cjk-song), size: 10pt)
  show math.equation: set text(font: "Latin Modern Math")
  for p in doc.problems {
    [#metadata((label: p.label)) <xcpc-problem>]
    badge()
    [= #p.title]
    p.body
  }
}
"##,
    )
    .unwrap();

    let mount = xcpc_engine::PluginMount {
        plugin_id: "icpc-pack".into(),
        plugin_name: "ICPC Pack".into(),
        version: "1.0.0".into(),
        root: plugin_root.clone(),
        font_dirs: Vec::new(),
        templates: vec![xcpc_engine::PluginTemplateEntry {
            id: "ec".into(),
            entry: "templates/ec.typ".into(),
            kind: Some(xcpc_domain::TemplateKind::Statement),
            name: Some("ICPC EC".into()),
            description: Some("EC Layout".into()),
        }],
    };

    let mut engine = crate::publish::engine(&ws, &xcpc_engine::FontOptions::from_env());
    engine.set_plugin_mounts(vec![mount]);

    // 1. Discovery lists plugin template with Plugin source.
    let templates = crate::publish::list_templates(&ws, &engine);
    let found = templates
        .iter()
        .find(|t| t.id == "plugin:icpc-pack/ec")
        .expect("plugin template listed");
    assert_eq!(
        found.source,
        xcpc_domain::TemplateSource::Plugin {
            plugin_id: "icpc-pack".into(),
            plugin_name: "ICPC Pack".into(),
            template_id: "ec".into(),
        }
    );
    assert!(found.schema.fields.iter().any(|f| f.key == "show-logo"));

    // 2. Compose contest using plugin template & compile cleanly.
    ws.write(
        "contests/round1/contest.toml",
        b"name = \"Round 1\"\nlanguage = \"english\"\ntemplate = \"plugin:icpc-pack/ec\"\n\n[[problem]]\nid = \"a-plus-b\"\nlabel = \"A\"\n",
    )
    .unwrap();
    let (compiled, _doc, report) = crate::publish::check(
        &ws,
        &mut engine,
        &crate::publish::Target::Contest("round1".into()),
        crate::publish::Kind::Statements,
        None,
        None,
    )
    .unwrap();
    assert!(report.ok, "compile diagnostics: {:?}", report.diagnostics);
    assert!(compiled.is_some());

    // 3. Eject copies bundle + wrapper into workspace templates/, compiling without any plugin mount.
    let (ejected_id, ejected_rel) =
        crate::publish::eject_plugin_template(&ws, &engine, "icpc-pack", "ec", Some("ec-local"))
            .unwrap();
    assert_eq!(ejected_id, "ec-local");
    assert_eq!(ejected_rel, "templates/ec-local.typ");
    assert!(ws_root.join("templates/ec-local.typ").is_file());
    assert!(ws_root.join("templates/ec-local/assets/logo.svg").is_file());

    // 4. Freeze contest archives plugin template bundle into snapshot/template/, then unmounting
    //    the plugin still allows frozen contest to compile cleanly offline!
    let frozen = crate::publish::freeze_contest(&ws, &mut engine, "round1").unwrap();
    assert!(
        frozen.frozen,
        "freeze report: {:?}",
        frozen.report.diagnostics
    );
    assert!(
        ws_root
            .join("contests/round1/snapshot/template/templates/ec.typ")
            .is_file()
    );
    assert!(
        ws_root
            .join("contests/round1/snapshot/template/assets/logo.svg")
            .is_file()
    );

    // Unmount all plugins -> frozen contest STILL compiles!
    engine.set_plugin_mounts(Vec::new());
    let (frozen_compiled, _, frozen_report) = crate::publish::check(
        &ws,
        &mut engine,
        &crate::publish::Target::Contest("round1".into()),
        crate::publish::Kind::Statements,
        None,
        None,
    )
    .unwrap();
    assert!(
        frozen_report.ok,
        "frozen offline report: {:?}",
        frozen_report.diagnostics
    );
    assert!(frozen_compiled.is_some());

    // And ejected template compiles cleanly with zero plugins mounted!
    let (ejected_compiled, _, ejected_report) = crate::publish::check_with_template(
        &ws,
        &mut engine,
        &crate::publish::Target::Problem("a-plus-b".into()),
        crate::publish::Kind::Statements,
        None,
        None,
        Some("ec-local"),
    )
    .unwrap();
    assert!(
        ejected_report.ok,
        "ejected report: {:?}",
        ejected_report.diagnostics
    );
    assert!(ejected_compiled.is_some());

    // 5. Missing plugin template emits PLUGIN_TEMPLATE_NOT_FOUND.
    let (missing_compiled, _, missing_report) = crate::publish::check_with_template(
        &ws,
        &mut engine,
        &crate::publish::Target::Problem("a-plus-b".into()),
        crate::publish::Kind::Statements,
        None,
        None,
        Some("plugin:icpc-pack/ec"),
    )
    .unwrap();
    assert!(!missing_report.ok);
    assert!(missing_compiled.is_none());
    assert!(
        missing_report
            .diagnostics
            .iter()
            .any(|d| d.code.as_deref() == Some(crate::publish::PLUGIN_TEMPLATE_NOT_FOUND))
    );
}

#[test]
fn editorial_template_kind_precedence_and_frozen_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let ws_root = dir.path().join("ws");
    let ws = xcpc_storage::init_workspace(&ws_root).unwrap();
    ws.create_problem("a-plus-b", "A + B Problem").unwrap();
    ws.write(
        "problems/a-plus-b/statement.md",
        b"Given $a, b$, output $a+b$.\n",
    )
    .unwrap();
    ws.write("problems/a-plus-b/samples/01.in", b"1 2\n")
        .unwrap();
    ws.write("problems/a-plus-b/samples/01.ans", b"3\n")
        .unwrap();
    ws.write(
        "problems/a-plus-b/editorial.md",
        b"## Statement\n\nAdd $a$ and $b$.\n\n## Complexity\n\n$O(1)$ time.\n",
    )
    .unwrap();

    // Create an editorial-only plugin template.
    let plugin_root = dir.path().join("plugins/editorial-slides");
    std::fs::create_dir_all(plugin_root.join("templates")).unwrap();
    std::fs::write(
        plugin_root.join("templates/slides.typ"),
        r##"#import "/xcpc/fonts.typ": cjk-hei, lm-sans-10

#let schema = (
  kind: "editorial",
  name: (zh: "题解演示", en: "Editorial Slides"),
  fields: (
    (key: "accent", label: "Accent", type: "color", scope: "book", default: "#0B6989"),
  ),
)

#let render(doc) = {
  set page(width: 16cm, height: 9cm, margin: 1cm)
  set text(font: (lm-sans-10, cjk-hei), size: 11pt)
  show math.equation: set text(font: "Latin Modern Math")
  for p in doc.problems {
    [#metadata((label: p.label)) <xcpc-tutorial>]
    [= #p.title]
    p.body
  }
}
"##,
    )
    .unwrap();

    let mount = xcpc_engine::PluginMount {
        plugin_id: "editorial-slides".into(),
        plugin_name: "Editorial Slides".into(),
        version: "0.1.0".into(),
        root: plugin_root,
        font_dirs: Vec::new(),
        templates: vec![xcpc_engine::PluginTemplateEntry {
            id: "slides".into(),
            entry: "templates/slides.typ".into(),
            kind: Some(xcpc_domain::TemplateKind::Editorial),
            name: Some("Editorial Slides".into()),
            description: Some("Slide deck editorial layout".into()),
        }],
    };

    let mut engine = crate::publish::engine(&ws, &xcpc_engine::FontOptions::from_env());
    engine.set_plugin_mounts(vec![mount]);

    // 1. TemplateKind::Editorial stays isolated from Statement-only listing.
    let stmt_only =
        crate::publish::list_templates_by_kind(&ws, &engine, xcpc_domain::TemplateKind::Statement);
    assert!(
        !stmt_only
            .iter()
            .any(|t| t.id == "plugin:editorial-slides/slides")
    );
    let ed_only =
        crate::publish::list_templates_by_kind(&ws, &engine, xcpc_domain::TemplateKind::Editorial);
    assert!(
        ed_only
            .iter()
            .any(|t| t.id == "plugin:editorial-slides/slides")
    );
    let both = crate::publish::list_statement_and_editorial_templates(&ws, &engine);
    assert!(both.iter().any(|t| t.id == "olymp-classic"));
    assert!(
        both.iter()
            .any(|t| t.id == "plugin:editorial-slides/slides")
    );

    // 2. Contest with statement template = olymp-classic and editorial-template = plugin:...
    ws.write(
        "contests/round1/contest.toml",
        b"name = \"Round 1\"\nlanguage = \"english\"\ntemplate = \"olymp-classic\"\neditorial-template = \"plugin:editorial-slides/slides\"\n\n[[problem]]\nid = \"a-plus-b\"\nlabel = \"A\"\n",
    )
    .unwrap();

    let (stmt_compiled, stmt_doc, stmt_report) = crate::publish::check(
        &ws,
        &mut engine,
        &crate::publish::Target::Contest("round1".into()),
        crate::publish::Kind::Statements,
        None,
        None,
    )
    .unwrap();
    assert!(
        stmt_report.ok,
        "stmt diagnostics: {:?}",
        stmt_report.diagnostics
    );
    assert!(stmt_compiled.is_some());
    assert!(
        stmt_doc
            .expect("stmt doc")
            .source
            .contains("/templates/olymp-classic.typ")
    );

    let (ed_compiled, ed_doc, ed_report) = crate::publish::check(
        &ws,
        &mut engine,
        &crate::publish::Target::Contest("round1".into()),
        crate::publish::Kind::Editorial,
        None,
        None,
    )
    .unwrap();
    assert!(ed_report.ok, "ed diagnostics: {:?}", ed_report.diagnostics);
    assert!(ed_compiled.is_some());
    assert!(
        ed_doc
            .expect("ed doc")
            .source
            .contains("/plugins/editorial-slides/templates/slides.typ")
    );

    // 3. Freeze archives both statement template and distinct editorial plugin template!
    let frozen = crate::publish::freeze_contest(&ws, &mut engine, "round1").unwrap();
    assert!(
        frozen.frozen,
        "freeze report: {:?}",
        frozen.report.diagnostics
    );
    assert!(
        ws_root
            .join("contests/round1/snapshot/editorial-template.toml")
            .is_file()
    );
    assert!(
        ws_root
            .join("contests/round1/snapshot/editorial-template/templates/slides.typ")
            .is_file()
    );

    // Unmount all plugins -> frozen contest editorial STILL compiles offline!
    engine.set_plugin_mounts(Vec::new());
    let (frozen_ed, frozen_ed_doc, frozen_ed_report) = crate::publish::check(
        &ws,
        &mut engine,
        &crate::publish::Target::Contest("round1".into()),
        crate::publish::Kind::Editorial,
        None,
        None,
    )
    .unwrap();
    assert!(
        frozen_ed_report.ok,
        "frozen editorial report: {:?}",
        frozen_ed_report.diagnostics
    );
    assert!(frozen_ed.is_some());
    assert!(
        frozen_ed_doc
            .expect("frozen ed doc")
            .source
            .contains("/contests/round1/snapshot/editorial-template/templates/slides.typ")
    );
}
