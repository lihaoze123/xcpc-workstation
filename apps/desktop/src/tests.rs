use nsh_core::JsonValue;
use nsh_core::element::{JsValue, TsTypes};
use xcpc_domain::Patch;

use crate::types::{ContestMetaPatchJs, OverridePatchJs, ProblemMetaPatchJs};

#[test]
fn tristate_patch_types_declare_valid_ts_interfaces_and_decode_keep_clear_set() {
    let mut types = TsTypes::default();
    assert_eq!(
        ProblemMetaPatchJs::ts_type(&mut types),
        "ProblemMetaPatchJs"
    );
    assert_eq!(
        ContestMetaPatchJs::ts_type(&mut types),
        "ContestMetaPatchJs"
    );
    assert_eq!(OverridePatchJs::ts_type(&mut types), "OverridePatchJs");

    // Verify tri-state decoding via nsh_core's JsonHost: omitted -> Keep, null -> Clear, value -> Set.
    let ov_json: JsonValue = r#"{"title":"New Title","time":null,"showMetadata":false}"#
        .parse()
        .unwrap();
    let ov: OverridePatchJs = nsh_core::element::from_json(&ov_json).unwrap();
    assert_eq!(ov.title, Patch::Set("New Title".into()));
    assert_eq!(ov.time, Patch::Clear);
    assert_eq!(ov.memory, Patch::Keep);
    assert_eq!(ov.input, Patch::Keep);
    assert_eq!(ov.output, Patch::Keep);
    assert_eq!(ov.show_metadata, Patch::Set(false));

    let pm_json: JsonValue = r#"{"title":"T","rating":null,"author":"Bob","memory":"512MB"}"#
        .parse()
        .unwrap();
    let pm: ProblemMetaPatchJs = nsh_core::element::from_json(&pm_json).unwrap();
    assert_eq!(pm.title.as_deref(), Some("T"));
    assert_eq!(pm.rating, Patch::Clear);
    assert_eq!(pm.author, Patch::Set("Bob".into()));
    assert_eq!(pm.time, Patch::Keep);
    assert_eq!(pm.memory, Patch::Set("512MB".into()));

    let cm_json: JsonValue = r#"{"location":null,"cover":true,"blankPages":false}"#
        .parse()
        .unwrap();
    let cm: ContestMetaPatchJs = nsh_core::element::from_json(&cm_json).unwrap();
    assert_eq!(cm.name, None);
    assert_eq!(cm.location, Patch::Clear);
    assert_eq!(cm.date, Patch::Keep);
    assert_eq!(cm.cover, Patch::Set(true));
    assert_eq!(cm.blank_pages, Some(false));
}

#[test]
fn static_ui_ipc_ban_no_sync_heavy_calls() {
    let ui_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ui");
    let banned = ["preview.inspectPackage(", "preview.exportStatus("];
    let mut violations = Vec::new();
    let mut stack = vec![ui_dir];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            if (!name.ends_with(".ts") && !name.ends_with(".tsx"))
                || name.ends_with(".test.ts")
                || name.ends_with(".test.tsx")
                || name == "store.ts"
            {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            for needle in banned {
                if text.contains(needle) {
                    violations.push(format!("{}: {needle}", path.display()));
                }
            }
        }
    }
    assert!(
        violations.is_empty(),
        "banned synchronous heavy IPC calls found in UI code: {violations:#?}"
    );
}
