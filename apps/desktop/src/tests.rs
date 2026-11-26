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

#[test]
fn static_ui_design_token_guardrail() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = vec![manifest.join("App.tsx")];
    let mut stack = vec![manifest.join("src/ui")];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().and_then(|s| s.to_str()) == Some("tokens") {
                    continue;
                }
                stack.push(path);
                continue;
            }
            let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            if !name.ends_with(".tsx") || name.ends_with(".test.tsx") || name == "BrandIcon.tsx" {
                continue;
            }
            files.push(path);
        }
    }

    const STYLE_PROPS: &[&str] = &[
        "fontSize",
        "fontWeight",
        "gap",
        "columnGap",
        "rowGap",
        "padding",
        "paddingTop",
        "paddingBottom",
        "paddingLeft",
        "paddingRight",
        "paddingHorizontal",
        "paddingVertical",
        "margin",
        "marginTop",
        "marginBottom",
        "marginLeft",
        "marginRight",
        "marginHorizontal",
        "marginVertical",
        "borderRadius",
        "borderTopLeftRadius",
        "borderTopRightRadius",
        "borderBottomLeftRadius",
        "borderBottomRightRadius",
    ];

    fn is_ident_char(b: u8) -> bool {
        b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
    }

    fn exemption_status(line: &str, prev: Option<&str>) -> Option<bool> {
        for candidate in [Some(line), prev].into_iter().flatten() {
            if let Some(pos) = candidate.find("token-exempt:") {
                let reason = candidate[pos + "token-exempt:".len()..]
                    .trim()
                    .trim_end_matches("*/}")
                    .trim_end_matches("*/")
                    .trim();
                return Some(!reason.is_empty());
            }
        }
        None
    }

    fn parse_numeric_after_colon(s: &str) -> Option<&str> {
        let rest = s.trim_start();
        let rest = rest.strip_prefix(':')?.trim_start();
        let bytes = rest.as_bytes();
        let mut idx = 0;
        if idx < bytes.len() && bytes[idx] == b'-' {
            idx += 1;
        }
        let digit_start = idx;
        while idx < bytes.len() && bytes[idx].is_ascii_digit() {
            idx += 1;
        }
        if idx == digit_start {
            return None;
        }
        if idx < bytes.len() && bytes[idx] == b'.' {
            idx += 1;
            let frac_start = idx;
            while idx < bytes.len() && bytes[idx].is_ascii_digit() {
                idx += 1;
            }
            if idx == frac_start {
                return None;
            }
        }
        if idx < bytes.len() && is_ident_char(bytes[idx]) {
            return None;
        }
        Some(&rest[..idx])
    }

    let mut violations = Vec::new();
    for path in files {
        let rel = path.strip_prefix(manifest).unwrap_or(&path).display();
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        for (i, &line) in lines.iter().enumerate() {
            let prev = if i > 0 { Some(lines[i - 1]) } else { None };
            match exemption_status(line, prev) {
                Some(true) => continue,
                Some(false) => {
                    violations.push(format!("{rel}:{}: empty token-exempt reason", i + 1));
                    continue;
                }
                None => {}
            }

            let bytes = line.as_bytes();
            // 1. Raw style property numeric literals
            for &prop in STYLE_PROPS {
                let mut offset = 0;
                while let Some(pos) = line[offset..].find(prop) {
                    let abs = offset + pos;
                    offset = abs + prop.len();
                    if abs > 0 && is_ident_char(bytes[abs - 1]) {
                        continue;
                    }
                    if let Some(num) = parse_numeric_after_colon(&line[offset..]) {
                        violations.push(format!("{rel}:{}: raw {prop}: {num}", i + 1));
                    }
                }
            }

            // 2. Raw control heights (18..=40)
            let mut offset = 0;
            while let Some(pos) = line[offset..].find("height") {
                let abs = offset + pos;
                offset = abs + "height".len();
                if abs > 0 && is_ident_char(bytes[abs - 1]) {
                    continue;
                }
                if let Some(num) = parse_numeric_after_colon(&line[offset..])
                    && let Ok(v) = num.parse::<u32>()
                    && (18..=40).contains(&v)
                {
                    violations.push(format!("{rel}:{}: raw control height: {num}", i + 1));
                }
            }

            // 3. Raw <Icon size={number}> or strokeWidth=
            if let Some(icon_pos) = line.find("<Icon") {
                let tag = &line[icon_pos..];
                if let Some(sz_pos) = tag.find("size={") {
                    let inner = tag[sz_pos + "size={".len()..].trim_start();
                    if inner.bytes().next().is_some_and(|b| b.is_ascii_digit()) {
                        violations
                            .push(format!("{rel}:{}: raw numeric <Icon size={{...}}>", i + 1));
                    }
                }
                if tag.contains("strokeWidth=") {
                    violations.push(format!("{rel}:{}: caller <Icon strokeWidth=...>", i + 1));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Phase 6D §2.4 design token guardrail violations:\n{}",
        violations.join("\n")
    );
}

#[test]
fn static_ui_motion_guardrail() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = vec![manifest.join("App.tsx")];
    let mut stack = vec![manifest.join("src/ui")];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().and_then(|s| s.to_str()) == Some("tokens") {
                    continue;
                }
                stack.push(path);
                continue;
            }
            let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            if (!name.ends_with(".ts") && !name.ends_with(".tsx"))
                || name.ends_with(".test.ts")
                || name.ends_with(".test.tsx")
            {
                continue;
            }
            // Documented motion exceptions (design-system.md §8):
            // - Brand intro animation (EXPO curve): BrandIcon.tsx, NoWorkspaceView.tsx
            // - Preview canvas crossfade (90ms) & wheel zoom damping (~120ms): studio/PreviewCanvas.tsx
            if name == "BrandIcon.tsx"
                || name == "NoWorkspaceView.tsx"
                || name == "PreviewCanvas.tsx"
            {
                continue;
            }
            files.push(path);
        }
    }

    let mut violations = Vec::new();
    for path in files {
        let rel = path.strip_prefix(manifest).unwrap_or(&path).display();
        let text = std::fs::read_to_string(&path).unwrap();
        for (i, line) in text.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") || trimmed.starts_with("*") {
                continue;
            }
            if line.contains("transitionDuration") && line.contains("ms(") {
                violations.push(format!("{rel}:{}: raw transitionDuration: ms(...)", i + 1));
            }
            if line.contains("\"spring(") || line.contains("'spring(") || line.contains("`spring(")
            {
                violations.push(format!("{rel}:{}: inline spring(...) string", i + 1));
            }
            if line.contains("reduceMotion(") {
                violations.push(format!(
                    "{rel}:{}: direct reduceMotion() call (use tokens/motion.ts)",
                    i + 1
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Phase 6G §4 motion guardrail violations:\n{}",
        violations.join("\n")
    );
}
