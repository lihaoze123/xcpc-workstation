//! Bundled Latin Modern and JetBrains Mono fonts work in release (and
//! `bundled-fonts` feature) builds without `XCPC_FONT_DIRS` or system fonts.

#[cfg(any(not(debug_assertions), feature = "bundled-fonts"))]
#[test]
fn bundled_fonts_compile_without_dirs_or_system() {
    use xcpc_engine::{Engine, FontOptions};

    let mut engine = Engine::new(
        &FontOptions {
            dirs: vec![],
            system: false,
        },
        None,
    );

    let families: Vec<String> = engine
        .font_families()
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    for expected in [
        "Latin Modern Roman",
        "Latin Modern Roman 12",
        "Latin Modern Roman 17",
        "Latin Modern Sans",
        "Latin Modern Sans 12",
        "Latin Modern Sans 17",
        "Latin Modern Mono",
        "Latin Modern Mono 12",
        "Latin Modern Math",
        "JetBrains Mono",
    ] {
        assert!(
            families.iter().any(|f| f == expected),
            "missing bundled font family `{expected}` in {families:?}"
        );
    }

    engine.set_main(
        r#"
#set text(font: "Latin Modern Roman 12")
#show math.equation: set text(font: "Latin Modern Math")
Roman *bold* _italic_ *_bold italic_*.

#text(font: "Latin Modern Roman", size: 10pt)[10pt]
#text(font: "Latin Modern Roman 17", size: 17pt)[17pt]
#text(font: "Latin Modern Sans")[Sans *bold* _oblique_]
#text(font: "Latin Modern Sans 12")[Sans 12 _oblique_]
#text(font: "Latin Modern Sans 17")[Sans 17 _oblique_]
#text(font: "Latin Modern Mono 12")[Mono 12]
#text(font: "Latin Modern Mono")[Mono 10 _italic_]

$ sum_(i=1)^n a_i <= 10^9 $

#text(font: "JetBrains Mono")[JetBrains *bold* _italic_ *_bold italic_*]
"#,
    );

    let compiled = engine.compile().expect("bundled-fonts sample compiles");
    assert!(compiled.page_count() >= 1);
    let font_warnings: Vec<_> = compiled
        .warnings
        .iter()
        .filter(|w| w.message.contains("unknown font family"))
        .collect();
    assert!(
        font_warnings.is_empty(),
        "unexpected font warnings: {font_warnings:#?}"
    );
}
