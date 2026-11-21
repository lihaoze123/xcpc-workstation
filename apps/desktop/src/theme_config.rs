//! User-global (`<dataDir>/theme.toml`) and workspace-local (`.xcpc/theme.toml`)
//! theme override loading and starter file generation.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use nsh_core::prelude::*;

/// Starter template written when the user clicks "打开 theme.toml" and no file exists yet.
pub(crate) const STARTER_THEME_TOML: &str = r##"# XCPC Workstation 主题定制（所有字段均可选，保存后自动生效）
# 优先级：工作区 .xcpc/theme.toml 覆盖用户全局 theme.toml

# [colors.light]
# primary = "#0B6989"
# background = "#FAFAFA"
# card = "#FFFFFF"
# sidebar = "#E4E4E7"

# [colors.dark]
# primary = "#0E7FA6"
# background = "#09090B"
# card = "#111113"
# sidebar = "#141417"

# [editor]
# font_family = "JetBrains Mono"
# font_size = 14
# line_height = 22

# [radius]
# base = 8
"##;

/// Allowed CSS token keys in `[colors.light]` and `[colors.dark]`.
const ALLOWED_COLOR_KEYS: &[&str] = &[
    "background",
    "foreground",
    "card",
    "card-foreground",
    "popover",
    "popover-foreground",
    "muted",
    "muted-foreground",
    "border",
    "input",
    "primary",
    "primary-foreground",
    "accent",
    "accent-foreground",
    "secondary",
    "secondary-foreground",
    "sidebar",
    "sidebar-primary",
    "chart-4",
    "chart-5",
    "ring",
    "destructive",
];

#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct ThemeColorEntryJs {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct ThemeOverridesJs {
    pub light_colors: Vec<ThemeColorEntryJs>,
    pub dark_colors: Vec<ThemeColorEntryJs>,
    pub radius: Option<f64>,
    pub editor_font_family: Option<String>,
    pub editor_font_size: Option<f64>,
    pub editor_line_height: Option<f64>,
    /// Absolute path of the active `theme.toml` (workspace override if present, else global).
    pub active_path: Option<String>,
}

/// Loads `<dataDir>/theme.toml` and overlays `<workspace>/.xcpc/theme.toml` if present.
pub(crate) fn load_theme_overrides(
    data_dir: Option<&Path>,
    workspace_root: Option<&Path>,
) -> ThemeOverridesJs {
    let mut light: HashMap<String, String> = HashMap::new();
    let mut dark: HashMap<String, String> = HashMap::new();
    let mut radius: Option<f64> = None;
    let mut editor_font_family: Option<String> = None;
    let mut editor_font_size: Option<f64> = None;
    let mut editor_line_height: Option<f64> = None;
    let mut active_path: Option<PathBuf> = None;

    let candidates = [
        data_dir.map(|d| d.join("theme.toml")),
        workspace_root.map(|w| w.join(".xcpc/theme.toml")),
    ];

    for path in candidates.into_iter().flatten() {
        let Ok(raw) = std::fs::read_to_string(&path) else {
            continue;
        };
        active_path = Some(path);
        let Ok(table) = toml::from_str::<toml::Table>(&raw) else {
            continue;
        };
        if let Some(colors) = table.get("colors").and_then(toml::Value::as_table) {
            if let Some(tbl) = colors.get("light").and_then(toml::Value::as_table) {
                merge_colors(&mut light, tbl);
            }
            if let Some(tbl) = colors.get("dark").and_then(toml::Value::as_table) {
                merge_colors(&mut dark, tbl);
            }
        }
        if let Some(rad) = table.get("radius").and_then(toml::Value::as_table) {
            let v = rad
                .get("base")
                .or_else(|| rad.get("md"))
                .and_then(as_f64);
            if let Some(r) = v
                && r.is_finite()
                && (0.0..=20.0).contains(&r)
            {
                radius = Some(r);
            }
        }
        if let Some(ed) = table.get("editor").and_then(toml::Value::as_table) {
            if let Some(fam) = ed
                .get("font_family")
                .or_else(|| ed.get("font-family"))
                .and_then(toml::Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                editor_font_family = Some(fam.to_owned());
            }
            if let Some(sz) = ed
                .get("font_size")
                .or_else(|| ed.get("font-size"))
                .and_then(as_f64)
                && sz.is_finite()
                && (11.0..=22.0).contains(&sz)
            {
                editor_font_size = Some(sz);
            }
            if let Some(lh) = ed
                .get("line_height")
                .or_else(|| ed.get("line-height"))
                .and_then(as_f64)
                && lh.is_finite()
                && (16.0..=36.0).contains(&lh)
            {
                editor_line_height = Some(lh);
            }
        }
    }

    let to_sorted = |map: HashMap<String, String>| {
        let mut out: Vec<ThemeColorEntryJs> = map
            .into_iter()
            .map(|(key, value)| ThemeColorEntryJs { key, value })
            .collect();
        out.sort_by(|a, b| a.key.cmp(&b.key));
        out
    };

    ThemeOverridesJs {
        light_colors: to_sorted(light),
        dark_colors: to_sorted(dark),
        radius,
        editor_font_family,
        editor_font_size,
        editor_line_height,
        active_path: active_path.map(|p| p.display().to_string()),
    }
}

/// Ensures a `theme.toml` file exists (preferring workspace `.xcpc/theme.toml` if already
/// created, else `<dataDir>/theme.toml`) and returns its absolute path.
pub(crate) fn ensure_theme_file(
    data_dir: Option<&Path>,
    workspace_root: Option<&Path>,
) -> Result<PathBuf, String> {
    if let Some(ws) = workspace_root {
        let ws_theme = ws.join(".xcpc/theme.toml");
        if ws_theme.is_file() {
            return Ok(ws_theme);
        }
    }
    let dir = data_dir.ok_or_else(|| "当前环境未配置应用数据目录，无法创建 theme.toml".to_owned())?;
    std::fs::create_dir_all(dir).map_err(|e| format!("无法创建配置目录：{e}"))?;
    let path = dir.join("theme.toml");
    if !path.is_file() {
        xcpc_storage::write_atomic(&path, STARTER_THEME_TOML.as_bytes())
            .map_err(|e| format!("无法写入 theme.toml：{e}"))?;
    }
    Ok(path)
}

fn merge_colors(dest: &mut HashMap<String, String>, tbl: &toml::Table) {
    for (k, v) in tbl {
        let key = k.trim();
        if !ALLOWED_COLOR_KEYS.contains(&key) {
            continue;
        }
        if let Some(val) = v.as_str().map(str::trim)
            && is_valid_hex_color(val)
        {
            dest.insert(key.to_owned(), val.to_owned());
        }
    }
}

fn is_valid_hex_color(s: &str) -> bool {
    let Some(hex) = s.strip_prefix('#') else {
        return false;
    };
    matches!(hex.len(), 3 | 6 | 8) && hex.bytes().all(|b| b.is_ascii_hexdigit())
}

fn as_f64(v: &toml::Value) -> Option<f64> {
    v.as_float().or_else(|| v.as_integer().map(|i| i as f64))
}
