//! Bundled preset template seeds (`olymp-classic`, `cnoi-classic`, `minimal`,
//! `trd-compact`, `trd-book`).
//!
//! Presets have no runtime privilege or implicit fallback; they are only materialized
//! into a workspace's `templates/` directory during initialization or explicit install.

use crate::{LocalizedText, PresetInfo, TemplateKind};

/// One bundled template seed (`olymp-classic`, `cnoi-classic`, `minimal`, `trd-compact`,
/// `trd-book`).
///
/// Presets have no runtime privilege or implicit fallback; they are only materialized
/// into a workspace's `templates/` directory during initialization or explicit install.
#[derive(Debug, Clone, Copy)]
pub struct PresetSeed {
    pub id: &'static str,
    pub kind: TemplateKind,
    pub name_zh: &'static str,
    pub name_en: &'static str,
    pub description_zh: &'static str,
    pub description_en: &'static str,
    pub source: &'static str,
}

/// All bundled template seeds shipped with XCPC Workstation.
pub const PRESETS: &[PresetSeed] = &[
    PresetSeed {
        id: "olymp-classic",
        kind: TemplateKind::Statement,
        name_zh: "XCPC 经典",
        name_en: "XCPC Classic",
        description_zh: "对齐 olymp.sty 的经典 ICPC / XCPC 版式",
        description_en: "Classic ICPC / XCPC layout aligned with olymp.sty",
        source: include_str!("../../xcpc-engine/templates/olymp-classic.typ"),
    },
    PresetSeed {
        id: "cnoi-classic",
        kind: TemplateKind::Statement,
        name_zh: "OI 风格",
        name_en: "OI Classic",
        description_zh: "CCF CSP-J/S 与 NOI 官方风格题面与题解",
        description_en: "Official CCF CSP-J/S and NOI style statements and editorials",
        source: include_str!("../../xcpc-engine/templates/cnoi-classic.typ"),
    },
    PresetSeed {
        id: "minimal",
        kind: TemplateKind::Statement,
        name_zh: "极简空白",
        name_en: "Minimal Blank",
        description_zh: "极简空白起点模板，方便从零编写自定义比赛模板",
        description_en: "Minimal blank starter template for building custom contest layouts from scratch",
        source: include_str!("../../xcpc-engine/templates/minimal.typ"),
    },
    PresetSeed {
        id: "trd-compact",
        kind: TemplateKind::Trd,
        name_zh: "赛场紧凑多栏",
        name_en: "TRD Compact",
        description_zh: "横版 A4 紧凑多栏赛场参考手册，附累计分块校验哈希边栏与考场核对命令",
        description_en: "Landscape A4 compact multi-column contest reference with KACTL++ cumulative hash gutter",
        source: include_str!("../../xcpc-engine/templates/trd-compact.typ"),
    },
    PresetSeed {
        id: "trd-book",
        kind: TemplateKind::Trd,
        name_zh: "中文讲义与复习册",
        name_en: "TRD Study Book",
        description_zh: "竖版 A4 讲义与复习册，延续 olymp-classic 字体体系，适合数学推导、公式卡片与代码混排",
        description_en: "Portrait A4 study guide with olymp-classic typography for derivations, formula cards, and code",
        source: include_str!("../../xcpc-engine/templates/trd-book.typ"),
    },
];

/// Returns metadata for all bundled preset template seeds.
pub fn list_presets() -> Vec<PresetInfo> {
    PRESETS
        .iter()
        .map(|p| PresetInfo {
            id: p.id.to_owned(),
            kind: p.kind,
            name: LocalizedText::bilingual(p.name_zh, p.name_en),
            description: LocalizedText::bilingual(p.description_zh, p.description_en),
        })
        .collect()
}

/// Returns metadata for bundled preset template seeds of a specific [`TemplateKind`].
pub fn list_presets_by_kind(kind: TemplateKind) -> Vec<PresetInfo> {
    list_presets()
        .into_iter()
        .filter(|p| p.kind == kind)
        .collect()
}

/// Returns the bundled Typst seed source for a preset ID (`"olymp-classic"`,
/// `"cnoi-classic"`, `"minimal"`, `"trd-compact"`, or `"trd-book"`).
pub fn preset_source(id: &str) -> Option<&'static str> {
    PRESETS.iter().find(|p| p.id == id).map(|p| p.source)
}
