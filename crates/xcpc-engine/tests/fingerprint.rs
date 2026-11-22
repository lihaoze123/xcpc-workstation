//! Page fingerprints ignore source positions: an edit late in the source keeps the
//! fingerprints of the pages it does not change.

use xcpc_engine::{Engine, FontOptions};

const SAMPLE: &str = include_str!("contest-zh.typ");

fn fingerprints(engine: &mut Engine, source: &str) -> Vec<u128> {
    engine.set_main(source);
    let compiled = engine.compile().expect("sample compiles");
    (0..compiled.page_count())
        .map(|i| compiled.page_fingerprint(i).unwrap())
        .collect()
}

#[test]
fn late_edit_keeps_earlier_pages() {
    let mut engine = Engine::new(
        &FontOptions::from_env(),
        Some(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))),
    );
    let before = fingerprints(&mut engine, SAMPLE);
    assert!(before.len() >= 3, "the sample spans several pages");

    // Same text again: same fingerprints.
    assert_eq!(fingerprints(&mut engine, SAMPLE), before);

    // A word in the last problem's note.
    let edited = SAMPLE.replace("请使用 64 位整数", "请使用 64 位有符号整数");
    assert_ne!(edited, SAMPLE);
    let after = fingerprints(&mut engine, &edited);
    assert_eq!(after.len(), before.len());
    assert_eq!(after[0], before[0], "the first page did not change");
    assert_ne!(after.last(), before.last(), "the last page changed");

    // Shifting every span without changing what is drawn keeps all of them.
    let shifted = format!("// a comment\n{SAMPLE}");
    assert_eq!(fingerprints(&mut engine, &shifted), before);
}
