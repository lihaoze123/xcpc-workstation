//! The workspace directory: the single source of truth (docs/architecture.md §5).
//!
//! Every file access goes through a [`Workspace`], which enforces the safety
//! boundary for the GUI, the CLI and MCP alike (§7.3):
//!
//! - paths are relative to the workspace and may not leave it, through `..` or a
//!   symbolic link;
//! - writes are atomic: a temporary file in the same directory, then a rename;
//! - a frozen contest's directory, its snapshot included, is read-only;
//! - a workspace opened read-only refuses every write.
//!
//! Errors carry a stable [`Error::code`] for `--json` and MCP.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
pub use xcpc_domain::EditorialSummary;
use xcpc_domain::{
    ContestItem, ContestMeta, Language, MAX_CONTEST_PROBLEMS, MAX_GROUP_NAME_CHARS, Problem,
    ProblemMeta, ProblemSource, Sample, SampleInventory, Statement, SyncedProblemStatus,
    TemplateOptionValue, TemplateOptionsMap, TextFile, TrainingLogEntry, TrainingStatus, check_id,
    derive_training_status, normalize_group_name, resolve_manual_status,
};

pub mod assets;
pub mod bundler;
pub mod code_hash;
pub mod edit;
pub mod example;
mod example_en;
mod example_packages;
mod example_snippets;
pub mod index;
pub mod snippets;
pub mod watcher;

pub use assets::{
    ASSET_MARKDOWN_DOCS, AssetEntry, AssetRef, CleanUnusedFailure, CleanUnusedResult,
    RenameAssetResult, ScannedAssetRef, is_existing_asset_name, is_safe_asset_name,
    is_windows_reserved_name, read_image_dimensions,
};
pub use bundler::{
    ParsedInclude, SnippetDag, bundle_local_file, bundle_local_source, bundle_snippet,
    bundle_workspace_code, parse_local_includes, parse_snippet_deps_from_code,
};
pub use code_hash::{
    compute_cpp_hash_6, extract_snippet_slug_from_include, md5_digest, md5_hex_6,
    preprocess_printed_code, strip_cpp_comments,
};
pub use edit::CreateProblemOptions;
pub use example::{
    init_example_workspace, init_example_workspace_with_options,
    init_example_workspace_with_template,
};
pub use index::{IndexedFrozenItem, WorkspaceIndex};
pub use snippets::DEFAULT_HASH_INTERVAL;
pub use watcher::{SelfWriteTracker, WatchEvent, WorkspaceWatcher};

/// The file that marks a workspace's root.
pub const WORKSPACE_FILE: &str = "workspace.toml";

/// Workspace-relative path of the append-only training status log (`training/logs.jsonl`).
pub const TRAINING_LOGS_REL_PATH: &str = "training/logs.jsonl";

/// Static workspace-level `AGENTS.md` written by [`init_workspace`] when absent.
/// Once created, it belongs to the user and is never overwritten.
pub const WORKSPACE_AGENTS_MD: &str = r#"# XCPC Workspace Guide

This directory is an **XCPC Workstation** workspace (`workspace.toml` marks the root).

## Directory Layout

```text
workspace.toml           # Workspace root marker & settings
problems/<slug>/         # One directory per problem (slug = immutable ID)
  problem.toml           # Metadata, [limits], [[source]], [package], [[test]], [solutions.*]
  statement.md           # Or statement.zh.md / statement.en.md (Markdown + $LaTeX$)
  editorial.md           # Optional A4 editorial
  samples/               # 01.in, 01.ans, ... (single source of truth; auto-included as leading tests)
  assets/                # Figures referenced by relative path: ![](assets/fig.png)
  validator.cpp          # Input validator (required for strict verify)
  checker.cpp            # Optional custom checker (default: wcmp, or builtin ncmp/fcmp/lcmp/rcmp4/rcmp6/rcmp9/yesno/nyesno)
  generators/<name>.cpp  # Deterministic testlib.h generators
  solutions/<name>.cpp   # std (main = true), brute/alt solutions, targeted wrong solutions
  tests/
    *.in                 # Hand-written tests & stress counterexamples (stress-<n>.in)
    validator/valid/*.in # Validator self-tests that must pass
    validator/invalid/*.in # Validator self-tests that must fail
    checker/{ok,wa,pe}-*/{in,out,ans} # Custom checker self-tests
contests/<id>/
  contest.toml           # Contest metadata, ordered [[problem]] live refs + metadata overrides
  snapshot/              # Created when frozen (read-only archive, committed to Git)
templates/               # Workspace Typst templates (committed to Git)
  preamble.typ           # Optional workspace-level Typst preamble injected after template show rule
build/                   # Generated PDFs & package build reports (gitignored)
.xcpc/                   # Strictly local caches & derived data (gitignored, never committed)
.agents/skills/          # Optional workspace skills (installed via `xcpc init --with-skills`)
```

## Authoring Rules

- **Problem IDs**: lowercase ASCII letters, digits, `-`, `_`, starting with a letter or digit (`<= 64` chars). Never rename a problem directory after creation.
- **Statements**: use level-2 Markdown headings (`## 输入格式` / `## Input`, `## 输出格式` / `## Output`, `## 交互` / `## Interaction`, `## 提示` / `## Note`). Samples are rendered automatically from `samples/`.
- **Contests**: live references with metadata-only overrides (`title`, `[problem.limits]`, `show-metadata`). Labels `A`–`Z` (max 26 problems) follow the order in `contest.toml`.
- **Frozen contests**: strictly read-only (including `snapshot/`). Unfreezing requires human confirmation via the GUI or `xcpc contest unfreeze` and is not available over MCP.
- **Static/dynamic separation**: `xcpc check` and `xcpc contest freeze` never compile or execute C++ silently. Problems with packages must pass `xcpc package build <slug> --strict` (producing a fresh verified report) before checking or freezing succeeds.

## Local Polygon Workflow & Anti-Cheating Discipline

Follow the built-in **`xcpc-problem-creator`** skill (install into `.agents/skills/xcpc-problem-creator/` with `xcpc init --with-skills`) for end-to-end problem authoring: statement & samples → `validator.cpp` + self-tests → `std.cpp` + independent `brute.cpp` + `stress_package` → adversarial generators & wrong solutions → `build_package(mode: "verify")` → editorial & PDF visual self-check.

**Non-negotiable discipline** (when verification fails, fix the root cause — never bypass the gate):
1. **Never relax `[limits]`** (`time` / `memory`) without explicit user approval just to make `std` pass the `<= 50%` time-limit margin or avoid TLE/MLE.
2. **Never delete or skip failing test cases** from `[[test]]` to make the build green.
3. **Never loosen `expect`** (e.g. changing `expect = "WA"` to `["AC", "WA"]`, or allowing a reference AC solution to fail).
4. **Never write an empty `validator.cpp` or fake `checker.cpp`**: both must have genuine passing and failing self-tests.
5. **Never silently overwrite manual `samples/NN.ans`**; manual sample answers are validated alongside `std` output.
6. **Never treat `mode: "debug"` as acceptance**: only a full `build_package(mode: "verify")` / `xcpc package build <id> --strict` issues `verified = true`.

## CLI & MCP

- Publishing & Contests:
  - `xcpc init [dir] [--example] [--with-skills]`
  - `xcpc check <problem|contest> <id> [--editorial] [--json]`
  - `xcpc build <problem|contest> <id> [--editorial]`
  - `xcpc render <problem|contest> <id> [--page N]`
  - `xcpc search` / `xcpc show` / `xcpc contest compose|override|freeze|unfreeze`
- Local Polygon Packages:
  - `xcpc package init <id> [--json]`
  - `xcpc package build <id> [--strict] [--tests <list>] [--solutions <list>] [--jobs N] [--json]`
  - `xcpc package stress <id> --gen <name> --args "<args>" [--solutions <list>] [--max-iter N] [--timeout S] [--json]`
- MCP Server (`xcpc mcp [--read-only]`):
  - Package tools: `inspect_package`, `inspect_test_case` (read-only); `init_package` (write); `build_package`, `stress_package` (exec, blocked under `--read-only`).
  - Publishing tools: `compile_and_diagnose`, `render_page_png`, `export_pdf`, `search_problems`, `get_problem`, `get_contest`, `write_problem`, `duplicate_problem`, `compose_contest`, `set_override`, `freeze_contest`.
"#;

/// Built-in `xcpc-problem-creator` skill files embedded into the binary for `xcpc init --with-skills`.
pub const BUILTIN_PROBLEM_CREATOR_SKILLS: &[(&str, &str)] = &[
    (
        ".agents/skills/xcpc-problem-creator/SKILL.md",
        include_str!("../../../skills/xcpc-problem-creator/SKILL.md"),
    ),
    (
        ".agents/skills/xcpc-problem-creator/mistake-taxonomy.md",
        include_str!("../../../skills/xcpc-problem-creator/mistake-taxonomy.md"),
    ),
    (
        ".agents/skills/xcpc-problem-creator/judge-validator/SKILL.md",
        include_str!("../../../skills/xcpc-problem-creator/judge-validator/SKILL.md"),
    ),
    (
        ".agents/skills/xcpc-problem-creator/solutions-tutorial/SKILL.md",
        include_str!("../../../skills/xcpc-problem-creator/solutions-tutorial/SKILL.md"),
    ),
    (
        ".agents/skills/xcpc-problem-creator/wrong-gen/SKILL.md",
        include_str!("../../../skills/xcpc-problem-creator/wrong-gen/SKILL.md"),
    ),
];

/// Copies the built-in `xcpc-problem-creator` skill files into
/// `<root>/.agents/skills/xcpc-problem-creator/`, creating parent directories and writing
/// atomically. Returns the workspace-relative paths written.
pub fn install_workspace_skills(root: impl AsRef<Path>) -> Result<Vec<String>> {
    let root = root.as_ref();
    let mut written = Vec::with_capacity(BUILTIN_PROBLEM_CREATOR_SKILLS.len());
    for (rel, content) in BUILTIN_PROBLEM_CREATOR_SKILLS {
        let dest = root.join(rel);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| io(parent.display(), e))?;
        }
        write_atomic(&dest, content.as_bytes()).map_err(|e| io(dest.display(), e))?;
        written.push((*rel).to_owned());
    }
    Ok(written)
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0} not found")]
    NotFound(String),
    #[error(transparent)]
    InvalidId(#[from] xcpc_domain::InvalidId),
    #[error("path {0:?} is outside the workspace")]
    PathEscape(String),
    #[error("contest {0:?} is frozen: it and its snapshot are read-only")]
    Frozen(String),
    #[error("contest {0:?} is archived: it is read-only until unarchived")]
    Archived(String),
    #[error("the workspace is open read-only")]
    ReadOnly,
    #[error("{path}: {message}")]
    Invalid { path: String, message: String },
    #[error("{path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{path}: modified on disk (expected {expected}, actual {actual})")]
    Conflict {
        path: String,
        expected: String,
        actual: String,
    },
    #[error("contest has {0} problems (maximum is {MAX_CONTEST_PROBLEMS})")]
    TooManyProblems(usize),
    #[error("problem {id:?} is referenced by unfrozen contest(s): {}", contests.join(", "))]
    Referenced { id: String, contests: Vec<String> },
    #[error("contest {0:?} is already frozen")]
    AlreadyFrozen(String),
    #[error("contest {0:?} is not frozen")]
    NotFrozen(String),
    #[error("path {0:?} is not allowed")]
    DisallowedPath(String),
    #[error("invalid group name {0:?}")]
    InvalidGroupName(String),
    #[error("group {0:?} already exists")]
    GroupAlreadyExists(String),
    #[error("snippet {0:?} already exists")]
    SnippetAlreadyExists(String),
    #[error("TRD {0:?} is frozen: it and its snapshot are read-only")]
    TrdFrozen(String),
    #[error("TRD {0:?} is archived: it is read-only until unarchived")]
    TrdArchived(String),
    #[error("TRD {0:?} is already frozen")]
    TrdAlreadyFrozen(String),
    #[error("TRD {0:?} is not frozen")]
    TrdNotFrozen(String),
    #[error("dependency cycle at {file}:{line}: {}", cycle.join(" -> "))]
    DependencyCycle {
        file: String,
        line: usize,
        cycle: Vec<String>,
    },
    #[error("missing local include {include:?} at {file}:{line}")]
    MissingInclude {
        file: String,
        line: usize,
        include: String,
    },
    #[error("cannot freeze TRD {id:?}: {}", issues.join("; "))]
    TrdFreezeBlocked { id: String, issues: Vec<String> },
}

impl From<xcpc_domain::InvalidGroupName> for Error {
    fn from(err: xcpc_domain::InvalidGroupName) -> Self {
        Error::InvalidGroupName(err.0)
    }
}

impl Error {
    /// A stable name for machines (`--json`, MCP).
    pub fn code(&self) -> &'static str {
        match self {
            Error::NotFound(_) => "not_found",
            Error::InvalidId(_) => "invalid_id",
            Error::PathEscape(_) => "path_escape",
            Error::Frozen(_) => "contest_frozen",
            Error::Archived(_) => "contest_archived",
            Error::ReadOnly => "read_only",
            Error::Invalid { .. } => "invalid_file",
            Error::Io { .. } => "io",
            Error::Conflict { .. } => "conflict",
            Error::TooManyProblems(_) => "too_many_problems",
            Error::Referenced { .. } => "problem_referenced",
            Error::AlreadyFrozen(_) => "already_frozen",
            Error::NotFrozen(_) => "not_frozen",
            Error::DisallowedPath(_) => "disallowed_path",
            Error::InvalidGroupName(_) => "invalid_group_name",
            Error::GroupAlreadyExists(_) => "group_already_exists",
            Error::SnippetAlreadyExists(_) => "snippet_already_exists",
            Error::TrdFrozen(_) => "trd_frozen",
            Error::TrdArchived(_) => "trd_archived",
            Error::TrdAlreadyFrozen(_) => "trd_already_frozen",
            Error::TrdNotFrozen(_) => "trd_not_frozen",
            Error::DependencyCycle { .. } => "dependency_cycle",
            Error::MissingInclude { .. } => "missing_include",
            Error::TrdFreezeBlocked { .. } => "trd_freeze_blocked",
        }
    }

    /// A centralized concise Chinese message for humans (phenomenon + next step),
    /// shared by the CLI and the desktop bridge.
    pub fn human_message(&self) -> String {
        match self {
            Error::NotFound(what) => format_not_found_message(what),
            Error::InvalidId(err) => format!(
                "标识「{}」格式无效。请以小写英文字母或数字开头，仅使用小写字母、数字、连字符（-）或下划线（_），长度不超过 64 个字符。",
                err.0
            ),
            Error::PathEscape(path) => {
                format!("路径「{path}」越出了工作区范围。请改用工作区内的相对路径。")
            }
            Error::Frozen(id) => {
                format!("比赛「{id}」已定稿，当前只读。如需修改，请先「重新编辑」并确认。")
            }
            Error::Archived(id) => {
                format!("比赛「{id}」已归档，当前只读。如需修改，请先「取消归档」。")
            }
            Error::ReadOnly => "工作区以只读模式打开，无法写入。请关闭只读模式后重试。".to_owned(),
            Error::Invalid { path, message } => {
                format!("文件「{path}」格式有误：{message}。请修正后重试。")
            }
            Error::Io { path, source } => {
                format!("读写「{path}」失败：{source}。请检查权限或文件占用状态后重试。")
            }
            Error::Conflict { path, .. } => {
                format!("文件「{path}」已被外部修改，保存已取消。请先重新加载再保存。")
            }
            Error::TooManyProblems(n) => format!(
                "比赛题目数超出上限（当前 {n} 题，最多 {MAX_CONTEST_PROBLEMS} 题）。请移除多余题目后重试。"
            ),
            Error::Referenced { id, contests } => format!(
                "题目「{id}」仍被未定稿比赛引用（{}），无法删除。请先从对应比赛中移除该题。",
                contests.join("、")
            ),
            Error::AlreadyFrozen(id) => {
                format!("比赛「{id}」已定稿。如需重新生成快照，请先「重新编辑」。")
            }
            Error::NotFrozen(id) => {
                format!("比赛「{id}」尚未定稿，无法重新编辑。请确认比赛状态后重试。")
            }
            Error::DisallowedPath(path) => {
                format!("路径「{path}」不允许写入。请仅修改题目或比赛目录下的受管文件。")
            }
            Error::InvalidGroupName(name) => format!(
                "分组名称「{name}」无效。名称不能为空、不能包含控制字符，且长度不超过 {MAX_GROUP_NAME_CHARS} 个字符。"
            ),
            Error::GroupAlreadyExists(name) => {
                format!("分组「{name}」已存在。请使用其他名称。")
            }
            Error::SnippetAlreadyExists(slug) => {
                format!("标识名「{slug}」已存在，请换一个标识名。")
            }
            Error::TrdFrozen(id) => {
                format!("模板册「{id}」已定稿，当前只读。如需修改，请先「重新编辑」并确认。")
            }
            Error::TrdArchived(id) => {
                format!("模板册「{id}」已归档，当前只读。如需修改，请先「取消归档」。")
            }
            Error::TrdAlreadyFrozen(id) => {
                format!("模板册「{id}」已定稿。如需重新生成快照，请先「重新编辑」。")
            }
            Error::TrdNotFrozen(id) => {
                format!("模板册「{id}」尚未定稿，无法重新编辑。请确认模板册状态后重试。")
            }
            Error::DependencyCycle { file, line, cycle } => {
                format!("在 {file}:{line} 检测到循环依赖：{}。", cycle.join(" → "))
            }
            Error::MissingInclude {
                file,
                line,
                include,
            } => {
                format!(
                    "在 {file}:{line} 找不到本地头文件「{include}」。请检查路径或 -I 搜索目录。"
                )
            }
            Error::TrdFreezeBlocked { id, issues } => {
                format!("模板册「{id}」无法定稿：{}。", issues.join("；"))
            }
        }
    }
}

fn format_not_found_message(what: &str) -> String {
    if let Some(id) = parse_quoted_after(what, "problem ") {
        return format!("找不到题目「{id}」。请检查题目 ID 是否存在。");
    }
    if let Some(id) = parse_quoted_after(what, "contest ") {
        return format!("找不到比赛「{id}」。请检查比赛 ID 是否存在。");
    }
    if let Some(name) = parse_quoted_after(what, "group ") {
        return format!("找不到分组「{name}」。请检查分组名称是否存在。");
    }
    if let Some(id) = parse_quoted_after(what, "preset ") {
        return format!(
            "找不到预设模板「{id}」。可用预设：olymp-classic、cnoi-classic、minimal、trd-compact、trd-book。"
        );
    }
    if let Some(id) = parse_quoted_after(what, "template ") {
        return format!(
            "找不到模板「{id}」。可运行 xcpc template list 查看已有模板，或用 xcpc template add 安装预设。"
        );
    }
    if let Some(rest) = what.strip_prefix("label ")
        && let Some((label_part, contest_part)) = rest.split_once(" in contest ")
        && let (Some(label), Some(contest_id)) = (
            strip_debug_quotes(label_part),
            strip_debug_quotes(contest_part),
        )
    {
        return format!("比赛「{contest_id}」中找不到题号「{label}」。请检查题号是否正确。");
    }
    if let Some(dir) = what.strip_prefix("workspace.toml at or above ") {
        return format!(
            "未找到工作区（{dir} 及其上级目录缺少 workspace.toml）。请重新选择工作区目录。"
        );
    }
    format!("找不到目标路径「{what}」。请确认文件是否存在于当前工作区。")
}

fn parse_quoted_after<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    strip_debug_quotes(text.strip_prefix(prefix)?)
}

fn strip_debug_quotes(s: &str) -> Option<&str> {
    let inner = s.strip_prefix('"')?.strip_suffix('"')?;
    (!inner.is_empty()).then_some(inner)
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Computes the lowercase hexadecimal SHA-256 digest of `bytes`.
pub fn content_hash(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for b in digest {
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

/// Initializes an XCPC workspace at `dir` (creating `dir` if needed).
///
/// Creates `workspace.toml` (only if absent), `problems/`, `contests/`, `.gitignore`
/// (ensuring `build/` and `.xcpc/` are present), and an embedded static `AGENTS.md`
/// (**never** overwriting an existing `AGENTS.md`).
pub fn init_workspace(dir: impl AsRef<Path>) -> Result<Workspace> {
    init_workspace_with_template(dir, None)
}

pub fn init_workspace_with_template(
    dir: impl AsRef<Path>,
    template: Option<&str>,
) -> Result<Workspace> {
    let dir = dir.as_ref();
    fs::create_dir_all(dir).map_err(|e| io(dir.display(), e))?;
    for sub in ["problems", "contests", "templates"] {
        let p = dir.join(sub);
        fs::create_dir_all(&p).map_err(|e| io(p.display(), e))?;
    }

    let selected_tpl = template.map(str::trim).filter(|s| !s.is_empty());
    let ws_file = dir.join(WORKSPACE_FILE);
    if fs::symlink_metadata(&ws_file).is_err() {
        let content = match selected_tpl {
            Some(name) => format!("template = \"{name}\"\n"),
            None => String::new(),
        };
        write_atomic(&ws_file, content.as_bytes()).map_err(|e| io(ws_file.display(), e))?;
    }

    let seed_id = selected_tpl.unwrap_or("olymp-classic");
    for id in [seed_id, "trd-compact", "trd-book"] {
        if let Some(src) = xcpc_domain::preset_source(id) {
            let tpl_file = dir.join("templates").join(format!("{id}.typ"));
            if fs::symlink_metadata(&tpl_file).is_err() {
                write_atomic(&tpl_file, src.as_bytes()).map_err(|e| io(tpl_file.display(), e))?;
            }
        }
    }

    let gitignore = dir.join(".gitignore");
    match fs::read_to_string(&gitignore) {
        Ok(existing) => {
            let mut lines: Vec<&str> = existing.lines().map(str::trim).collect();
            let mut appended = String::new();
            for entry in ["build/", ".xcpc/"] {
                let bare = entry.trim_end_matches('/');
                if !lines.iter().any(|l| *l == entry || *l == bare) {
                    appended.push_str(entry);
                    appended.push('\n');
                    lines.push(entry);
                }
            }
            if !appended.is_empty() {
                let mut updated = existing;
                if !updated.is_empty() && !updated.ends_with('\n') {
                    updated.push('\n');
                }
                updated.push_str(&appended);
                write_atomic(&gitignore, updated.as_bytes())
                    .map_err(|e| io(gitignore.display(), e))?;
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            write_atomic(&gitignore, b"build/\n.xcpc/\n")
                .map_err(|e| io(gitignore.display(), e))?;
        }
        Err(e) => return Err(io(gitignore.display(), e)),
    }

    let agents = dir.join("AGENTS.md");
    if fs::symlink_metadata(&agents).is_err() {
        write_atomic(&agents, WORKSPACE_AGENTS_MD.as_bytes())
            .map_err(|e| io(agents.display(), e))?;
    }

    Workspace::open(dir, false)
}

/// Archived template configuration stored inside `contests/<id>/snapshot/template.toml`
/// when a contest is frozen, guaranteeing reproducible re-rendering even if the workspace's
/// default template, custom templates, plugin templates, or `templates/preamble.typ` later change.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SnapshotTemplateMeta {
    pub template: String,
    #[serde(default)]
    pub has_custom_template: bool,
    #[serde(default)]
    pub has_preamble: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub font_dirs: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub options: BTreeMap<String, TemplateOptionValue>,
}

/// Complete template archive payload passed to [`Workspace::freeze_commit_with_template`].
#[derive(Debug, Clone, Default)]
pub struct SnapshotTemplateArchive {
    pub meta: SnapshotTemplateMeta,
    pub custom_template_bytes: Option<Vec<u8>>,
    pub preamble_bytes: Option<Vec<u8>>,
    /// Archived template `image` option assets copied into `snapshot/assets/<filename>`.
    pub assets: BTreeMap<String, Vec<u8>>,
    /// Archived plugin template bundle files copied into `snapshot/template/<rel_path>`.
    pub plugin_bundle_files: BTreeMap<String, Vec<u8>>,
}

/// Reserved stem under `templates/` used by the workspace preamble (`templates/preamble.typ`).
pub const RESERVED_PREAMBLE_NAME: &str = "preamble";

/// Workspace-relative path of the shared Typst preamble.
pub const PREAMBLE_REL_PATH: &str = "templates/preamble.typ";

/// Starter content written when creating `templates/preamble.typ` for the first time.
const DEFAULT_PREAMBLE_STARTER: &str = "\
// XCPC Workstation 工作区级 Typst 样式注入（templates/preamble.typ）\n\
// 此文件在模板 #show: contest.with(...) 之后自动注入，可在此定义自定义宏、导入包或微调样式。\n\
// 保存后桌面端预览会自动热刷新。\n\n";

/// A contest as loaded from `contests/<id>/contest.toml`.
#[derive(Debug, Clone, PartialEq)]
pub struct Contest {
    pub id: String,
    /// `contests/<id>`.
    pub dir: String,
    pub meta: ContestMeta,
}

/// Strips Windows extended-length verbatim prefixes (`\\?\C:\...` -> `C:\...`,
/// `\\?\UNC\server\share\...` -> `\\server\share\...`) returned by `fs::canonicalize()`.
/// External tools such as MinGW/MSYS2 `g++` (`cc1plus.exe`) and testlib cannot open `\\?\` paths.
pub fn strip_verbatim_prefix(path: &Path) -> PathBuf {
    let Some(s) = path.to_str() else {
        return path.to_path_buf();
    };
    if let Some(unc) = s.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{unc}"));
    }
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        let bytes = rest.as_bytes();
        if bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && (bytes[2] == b'\\' || bytes[2] == b'/')
        {
            return PathBuf::from(rest);
        }
    }
    path.to_path_buf()
}

/// Canonicalizes `path` and strips any Windows verbatim `\\?\` prefix so prefix checks against
/// [`Workspace::root`] and invocations of external tools behave identically across platforms.
pub fn canonicalize_clean(path: &Path) -> std::io::Result<PathBuf> {
    path.canonicalize().map(|p| strip_verbatim_prefix(&p))
}

/// An open workspace.
#[derive(Debug, Clone)]
pub struct Workspace {
    root: PathBuf,
    read_only: bool,
    self_writes: Option<SelfWriteTracker>,
}

impl Workspace {
    /// Opens the workspace at `root` (which must exist).
    pub fn open(root: impl AsRef<Path>, read_only: bool) -> Result<Self> {
        let root = root.as_ref();
        let root = strip_verbatim_prefix(&root.canonicalize().map_err(|e| io(root.display(), e))?);
        if !root.is_dir() {
            return Err(Error::NotFound(root.display().to_string()));
        }
        Ok(Self {
            root,
            read_only,
            self_writes: None,
        })
    }

    /// The workspace `start` is in: the nearest directory at or above it with a
    /// `workspace.toml`.
    pub fn discover(start: impl AsRef<Path>, read_only: bool) -> Result<Self> {
        let start = start.as_ref();
        let start =
            strip_verbatim_prefix(&start.canonicalize().map_err(|e| io(start.display(), e))?);
        start
            .ancestors()
            .find(|d| d.join(WORKSPACE_FILE).is_file())
            .ok_or_else(|| {
                Error::NotFound(format!("{WORKSPACE_FILE} at or above {}", start.display()))
            })
            .and_then(|d| Self::open(d, read_only))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// Reads the optional `training = <bool>` setting from `workspace.toml`, defaulting
    /// to `true` when absent.
    pub fn training_enabled(&self) -> bool {
        let Ok(raw) = fs::read_to_string(self.root.join(WORKSPACE_FILE)) else {
            return true;
        };
        let Ok(table) = toml::from_str::<toml::Table>(&raw) else {
            return true;
        };
        table
            .get("training")
            .and_then(toml::Value::as_bool)
            .unwrap_or(true)
    }

    /// Reads all manual status events from `training/logs.jsonl`.
    ///
    /// Returns an empty list if `training/logs.jsonl` does not exist yet; skips blank lines;
    /// returns [`Error::Invalid`] on malformed JSON and [`Error::PathEscape`] if the path
    /// escapes the workspace (e.g. via a symlink).
    pub fn read_training_logs(&self) -> Result<Vec<TrainingLogEntry>> {
        let raw = match self.read_text(TRAINING_LOGS_REL_PATH) {
            Ok(text) => text,
            Err(Error::NotFound(_)) => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let mut entries = Vec::new();
        for (idx, line) in raw.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let entry =
                TrainingLogEntry::from_json_line(trimmed).map_err(|msg| Error::Invalid {
                    path: TRAINING_LOGS_REL_PATH.to_owned(),
                    message: format!("line {}: {msg}", idx + 1),
                })?;
            entries.push(entry);
        }
        Ok(entries)
    }

    /// Atomically appends one status event line to `training/logs.jsonl`.
    ///
    /// Enforces read-only mode and workspace path boundaries, validates `problem_id`
    /// (when present) and ensures at least one of `problem_id`, `source`, or `title`
    /// is non-empty, creates `training/` if needed, and writes via read-modify-[`write_atomic`].
    pub fn append_training_log(&self, entry: &TrainingLogEntry) -> Result<()> {
        self.check_writable(TRAINING_LOGS_REL_PATH)?;
        if let Some(ref id) = entry.problem_id {
            check_id(id)?;
        }
        entry.validate().map_err(|msg| Error::Invalid {
            path: TRAINING_LOGS_REL_PATH.to_owned(),
            message: msg,
        })?;
        let line = entry.to_json_line().map_err(|msg| Error::Invalid {
            path: TRAINING_LOGS_REL_PATH.to_owned(),
            message: msg,
        })?;
        let mut updated = match self.read_text(TRAINING_LOGS_REL_PATH) {
            Ok(existing) => existing,
            Err(Error::NotFound(_)) => String::new(),
            Err(e) => return Err(e),
        };
        if !updated.is_empty() && !updated.ends_with('\n') {
            updated.push('\n');
        }
        updated.push_str(&line);
        updated.push('\n');
        self.write(TRAINING_LOGS_REL_PATH, updated.as_bytes())
    }

    /// Scans workspace problems and returns summaries for every problem whose
    /// `editorial.md` exists and contains non-whitespace content, sorted by
    /// `updated_at` descending (tie-break by `problem_id` ascending).
    pub fn list_editorials(&self) -> Result<Vec<EditorialSummary>> {
        let mut items = Vec::new();
        for id in self.list_problems()? {
            let editorial_rel = format!("problems/{id}/editorial.md");
            let editorial_path = self.resolve(&editorial_rel)?;
            if !editorial_path.is_file() {
                continue;
            }
            let text = self.read_text(&editorial_rel)?;
            if text.trim().is_empty() {
                continue;
            }
            let meta_rel = format!("problems/{id}/problem.toml");
            let Ok(meta_raw) = self.read_text(&meta_rel) else {
                continue;
            };
            let Ok(meta) = parse_toml::<ProblemMeta>(&meta_rel, &meta_raw) else {
                continue;
            };
            let updated_at = fs::metadata(&editorial_path)
                .and_then(|m| m.modified())
                .map(system_time_to_rfc3339_utc)
                .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned());
            items.push(EditorialSummary {
                problem_id: id,
                title: meta.title,
                rating: meta.rating.and_then(|r| i32::try_from(r).ok()),
                tags: meta.tags,
                sources: meta.sources,
                excerpt: extract_editorial_excerpt(&text),
                updated_at,
            });
        }
        items.sort_by(|a, b| {
            b.updated_at
                .cmp(&a.updated_at)
                .then_with(|| a.problem_id.cmp(&b.problem_id))
        });
        Ok(items)
    }

    /// Derives the effective [`TrainingStatus`] for one problem by loading `training/logs.jsonl`
    /// and merging it with `synced`.
    pub fn derive_problem_training_status(
        &self,
        problem_id: &str,
        sources: &[ProblemSource],
        synced: SyncedProblemStatus,
    ) -> Result<TrainingStatus> {
        let logs = self.read_training_logs()?;
        Ok(Self::derive_problem_training_status_from_logs(
            &logs, problem_id, sources, synced,
        ))
    }

    /// Derives the effective [`TrainingStatus`] for one problem using pre-loaded `entries`.
    pub fn derive_problem_training_status_from_logs(
        entries: &[TrainingLogEntry],
        problem_id: &str,
        sources: &[ProblemSource],
        synced: SyncedProblemStatus,
    ) -> TrainingStatus {
        let manual = resolve_manual_status(entries, problem_id, sources);
        derive_training_status(synced, manual)
    }

    /// Batch-derives effective [`TrainingStatus`] values for multiple problems using pre-loaded
    /// `entries`.
    pub fn derive_problem_training_statuses<'a>(
        entries: &[TrainingLogEntry],
        problems: impl IntoIterator<Item = (&'a str, &'a [ProblemSource], SyncedProblemStatus)>,
    ) -> BTreeMap<String, TrainingStatus> {
        let mut by_problem_id: HashMap<&str, (usize, Option<TrainingStatus>)> = HashMap::new();
        let mut by_source: HashMap<(String, String), (usize, Option<TrainingStatus>)> =
            HashMap::new();

        for (idx, entry) in entries.iter().enumerate() {
            let status = entry.status.to_training_status();
            if let Some(pid) = entry.problem_id.as_deref()
                && !pid.is_empty()
            {
                by_problem_id.insert(pid, (idx, status));
            }
            if let Some(src) = entry.source.as_ref() {
                let platform = src.platform.trim().to_ascii_lowercase();
                let source_id = src.id.trim().to_ascii_lowercase();
                if !platform.is_empty() && !source_id.is_empty() {
                    by_source.insert((platform, source_id), (idx, status));
                }
            }
        }

        problems
            .into_iter()
            .map(|(problem_id, sources, synced)| {
                let mut latest: Option<(usize, Option<TrainingStatus>)> = None;
                if !problem_id.is_empty()
                    && let Some(&hit) = by_problem_id.get(problem_id)
                {
                    latest = Some(hit);
                }
                for src in sources {
                    let platform = src.platform.trim().to_ascii_lowercase();
                    let source_id = src.id.trim().to_ascii_lowercase();
                    if platform.is_empty() || source_id.is_empty() {
                        continue;
                    }
                    if let Some(&hit) = by_source.get(&(platform, source_id))
                        && latest.is_none_or(|(best_idx, _)| hit.0 >= best_idx)
                    {
                        latest = Some(hit);
                    }
                }
                let manual = latest.and_then(|(_, s)| s);
                let status = derive_training_status(synced, manual);
                (problem_id.to_owned(), status)
            })
            .collect()
    }

    /// Finds the first problem in the workspace whose `[[source]]` matches `(platform, source_id)`
    /// (case-insensitive), returning its slug ID if found.
    pub fn find_problem_by_source(
        &self,
        platform: &str,
        source_id: &str,
    ) -> Result<Option<String>> {
        let platform = platform.trim();
        let source_id = source_id.trim();
        if platform.is_empty() || source_id.is_empty() {
            return Ok(None);
        }
        for id in self.list_problems()? {
            let rel = format!("problems/{id}/problem.toml");
            let Ok(raw) = self.read_text(&rel) else {
                continue;
            };
            let Ok(meta) = parse_toml::<ProblemMeta>(&rel, &raw) else {
                continue;
            };
            if meta.sources.iter().any(|s| s.matches(platform, source_id)) {
                return Ok(Some(id));
            }
        }
        Ok(None)
    }

    /// Reads the optional `template = "<name>"` setting from `workspace.toml`.
    pub fn default_template(&self) -> Option<String> {
        let raw = fs::read_to_string(self.root.join(WORKSPACE_FILE)).ok()?;
        let table: toml::Table = toml::from_str(&raw).ok()?;
        table
            .get("template")
            .and_then(toml::Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    }

    /// Reads the optional `editorial-template = "<name>"` setting from `workspace.toml`.
    pub fn default_editorial_template(&self) -> Option<String> {
        let raw = fs::read_to_string(self.root.join(WORKSPACE_FILE)).ok()?;
        let table: toml::Table = toml::from_str(&raw).ok()?;
        table
            .get("editorial-template")
            .and_then(toml::Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    }

    /// Reads `[template-options.<template>]` defaults from `workspace.toml`.
    pub fn workspace_template_options(&self) -> TemplateOptionsMap {
        #[derive(serde::Deserialize, Default)]
        #[serde(rename_all = "kebab-case")]
        struct WsOpts {
            #[serde(default)]
            template_options: TemplateOptionsMap,
        }
        let Ok(raw) = fs::read_to_string(self.root.join(WORKSPACE_FILE)) else {
            return BTreeMap::new();
        };
        toml::from_str::<WsOpts>(&raw)
            .map(|w| w.template_options)
            .unwrap_or_default()
    }

    /// Reads the optional top-level `groups = [...]` declaration from `workspace.toml`,
    /// normalizing each entry, ignoring empty/invalid items, and deduplicating in order.
    pub fn workspace_groups(&self) -> Vec<String> {
        let Ok(raw) = fs::read_to_string(self.root.join(WORKSPACE_FILE)) else {
            return Vec::new();
        };
        let Ok(table) = toml::from_str::<toml::Table>(&raw) else {
            return Vec::new();
        };
        let Some(arr) = table.get("groups").and_then(toml::Value::as_array) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        for val in arr {
            if let Some(s) = val.as_str()
                && let Ok(norm) = normalize_group_name(s)
                && seen.insert(norm.clone())
            {
                out.push(norm);
            }
        }
        out
    }

    /// Reads `templates/preamble.typ` from the workspace if present and non-empty.
    pub fn load_preamble(&self) -> Option<String> {
        let text = fs::read_to_string(self.root.join(PREAMBLE_REL_PATH)).ok()?;
        (!text.trim().is_empty()).then_some(text)
    }

    /// Ensures `templates/preamble.typ` exists (creating it with a starter comment if absent)
    /// and returns its resolved path on disk.
    pub fn ensure_preamble(&self) -> Result<PathBuf> {
        let rel = PREAMBLE_REL_PATH;
        let path = self.resolve(rel)?;
        if !path.is_file() {
            self.write(rel, DEFAULT_PREAMBLE_STARTER.as_bytes())?;
        }
        self.resolve(rel)
    }

    /// Returns `true` when `templates/<name>.typ` exists inside the workspace.
    /// The reserved name `preamble` is never treated as a custom template.
    pub fn has_custom_template(&self, name: &str) -> bool {
        name != RESERVED_PREAMBLE_NAME
            && check_id(name).is_ok()
            && self
                .root
                .join("templates")
                .join(format!("{name}.typ"))
                .is_file()
    }

    /// Returns the resolved disk path of `templates/<name>.typ` if it exists.
    /// The reserved name `preamble` is never treated as a custom template.
    pub fn template_file_path(&self, name: &str) -> Result<Option<PathBuf>> {
        check_id(name)?;
        if name == RESERVED_PREAMBLE_NAME {
            return Ok(None);
        }
        let rel = format!("templates/{name}.typ");
        let path = self.resolve(&rel)?;
        Ok(path.is_file().then_some(path))
    }

    /// Creates a new custom template `templates/<new_name>.typ` from `source_bytes`.
    /// Refuses the reserved name `preamble` and refuses to overwrite an existing template file.
    pub fn fork_template(&self, new_name: &str, source_bytes: &[u8]) -> Result<String> {
        check_id(new_name)?;
        if new_name == RESERVED_PREAMBLE_NAME {
            return Err(Error::Invalid {
                path: PREAMBLE_REL_PATH.to_owned(),
                message: format!(
                    "template name `{RESERVED_PREAMBLE_NAME}` is reserved for the workspace preamble ({PREAMBLE_REL_PATH})"
                ),
            });
        }
        let rel = format!("templates/{new_name}.typ");
        self.check_writable(&rel)?;
        let path = self.resolve(&rel)?;
        if path.exists() {
            let actual = fs::read(&path)
                .map(|b| content_hash(&b))
                .unwrap_or_default();
            return Err(Error::Conflict {
                path: rel,
                expected: String::new(),
                actual,
            });
        }
        self.write(&rel, source_bytes)?;
        Ok(rel)
    }

    /// Writes an ejected plugin template into `templates/<target_name>.typ` and, when
    /// `bundle_files` is non-empty, copies all companion files into `templates/<target_name>/`.
    /// Refuses the reserved name `preamble` and refuses to overwrite an existing template.
    pub fn eject_template_bundle(
        &self,
        target_name: &str,
        entry_typ_bytes: &[u8],
        bundle_files: &BTreeMap<String, Vec<u8>>,
    ) -> Result<String> {
        check_id(target_name)?;
        if target_name == RESERVED_PREAMBLE_NAME {
            return Err(Error::Invalid {
                path: PREAMBLE_REL_PATH.to_owned(),
                message: format!(
                    "template name `{RESERVED_PREAMBLE_NAME}` is reserved for the workspace preamble ({PREAMBLE_REL_PATH})"
                ),
            });
        }
        let rel = format!("templates/{target_name}.typ");
        self.check_writable(&rel)?;
        let path = self.resolve(&rel)?;
        if path.exists() {
            let actual = fs::read(&path)
                .map(|b| content_hash(&b))
                .unwrap_or_default();
            return Err(Error::Conflict {
                path: rel,
                expected: String::new(),
                actual,
            });
        }
        if !bundle_files.is_empty() {
            let dir_rel = format!("templates/{target_name}");
            let dir_path = self.resolve(&dir_rel)?;
            if dir_path.exists() {
                return Err(Error::Conflict {
                    path: dir_rel,
                    expected: String::new(),
                    actual: "exists".into(),
                });
            }
            for (sub_rel, bytes) in bundle_files {
                let target_rel = format!("{dir_rel}/{sub_rel}");
                self.write(&target_rel, bytes)?;
            }
        }
        self.write(&rel, entry_typ_bytes)?;
        Ok(rel)
    }

    /// Installs a bundled preset seed (`olymp-classic`, `cnoi-classic`, `minimal`) into
    /// `templates/<target>.typ` (defaulting `target` to `preset_id`).
    /// Refuses to overwrite an existing template file.
    pub fn install_preset_template(
        &self,
        preset_id: &str,
        target_name: Option<&str>,
    ) -> Result<String> {
        let Some(src) = xcpc_domain::preset_source(preset_id) else {
            return Err(Error::NotFound(format!("preset {preset_id:?}")));
        };
        let target = target_name
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(preset_id);
        self.fork_template(target, src.as_bytes())
    }

    /// Returns `true` when `contests/<contest_id>/template.typ` exists inside the workspace.
    pub fn has_contest_template(&self, contest_id: &str) -> bool {
        check_id(contest_id).is_ok()
            && self
                .root
                .join("contests")
                .join(contest_id)
                .join("template.typ")
                .is_file()
    }

    /// Returns the resolved disk path of `contests/<contest_id>/template.typ` if it exists.
    pub fn contest_template_path(&self, contest_id: &str) -> Result<Option<PathBuf>> {
        check_id(contest_id)?;
        let rel = format!("contests/{contest_id}/template.typ");
        let path = self.resolve(&rel)?;
        Ok(path.is_file().then_some(path))
    }

    /// Copies the contest's currently effective workspace template (`templates/<name>.typ`)
    /// into `contests/<contest_id>/template.typ` so this contest can be customized independently.
    /// Refuses to overwrite an existing contest-local template.
    pub fn detach_contest_template(&self, contest_id: &str) -> Result<String> {
        check_id(contest_id)?;
        let rel = format!("contests/{contest_id}/template.typ");
        self.check_writable(&rel)?;
        let contest = self.load_contest(contest_id)?;
        let dest = self.resolve(&rel)?;
        if dest.exists() {
            let actual = fs::read(&dest)
                .map(|b| content_hash(&b))
                .unwrap_or_default();
            return Err(Error::Conflict {
                path: rel,
                expected: String::new(),
                actual,
            });
        }
        let tpl_name = contest
            .meta
            .template
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .or_else(|| self.default_template())
            .unwrap_or_else(|| "olymp-classic".into());
        check_id(&tpl_name)?;
        let src_rel = format!("templates/{tpl_name}.typ");
        let bytes = self.read_bytes(&src_rel).map_err(|e| match e {
            Error::NotFound(_) => Error::NotFound(format!("template {tpl_name:?}")),
            other => other,
        })?;
        self.write(&rel, &bytes)?;
        self.copy_contest_options_to_local(contest_id, &tpl_name)?;
        Ok(rel)
    }

    /// Removes `contests/<contest_id>/template.typ`, restoring the contest to use workspace templates.
    pub fn remove_contest_template(&self, contest_id: &str) -> Result<()> {
        check_id(contest_id)?;
        let rel = format!("contests/{contest_id}/template.typ");
        self.check_writable(&rel)?;
        let abs = self.resolve(&rel)?;
        let meta = fs::symlink_metadata(&abs).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => Error::NotFound(rel.clone()),
            _ => io(&rel, e),
        })?;
        if meta.file_type().is_symlink() {
            return Err(Error::PathEscape(rel));
        }
        if !meta.is_file() {
            return Err(Error::NotFound(rel));
        }
        if let Some(tracker) = &self.self_writes {
            tracker.record(&rel, watcher::SELF_DELETE_SENTINEL);
        }
        if let Err(e) = fs::remove_file(&abs) {
            if let Some(tracker) = &self.self_writes {
                tracker.unrecord(&rel);
            }
            return Err(io(&rel, e));
        }
        Ok(())
    }

    /// Atomically imports an image file from `src_path` into `contests/<id>/assets/` (when
    /// `contest_id` is `Some`) or workspace `assets/` (when `None`), avoiding filename
    /// collisions by appending `-2`, `-3`, etc. Returns the workspace-relative path.
    pub fn import_template_image(
        &self,
        contest_id: Option<&str>,
        src_path: &Path,
    ) -> Result<String> {
        let dir_rel = match contest_id {
            Some(cid) => {
                check_id(cid)?;
                format!("contests/{cid}/assets")
            }
            None => "assets".to_owned(),
        };
        self.check_writable(&format!("{dir_rel}/_probe"))?;
        let bytes = fs::read(src_path).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => Error::NotFound(src_path.display().to_string()),
            _ => io(src_path.display(), e),
        })?;

        let raw_name = src_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("image.png");
        let (stem, ext) = sanitize_template_image_name(raw_name);
        let mut candidate = format!("{stem}{ext}");
        let mut rel = format!("{dir_rel}/{candidate}");
        let mut suffix = 2u32;
        while self.resolve(&rel)?.exists() {
            candidate = format!("{stem}-{suffix}{ext}");
            rel = format!("{dir_rel}/{candidate}");
            suffix += 1;
        }
        self.write(&rel, &bytes)?;
        Ok(rel)
    }

    /// Reads archived template metadata from `contests/<id>/snapshot/template.toml` if present.
    pub fn load_snapshot_template(&self, contest_id: &str) -> Option<SnapshotTemplateMeta> {
        check_id(contest_id).ok()?;
        let rel = format!("contests/{contest_id}/snapshot/template.toml");
        let raw = self.read_text(&rel).ok()?;
        toml::from_str(&raw).ok()
    }

    /// Reads archived editorial template metadata from
    /// `contests/<id>/snapshot/editorial-template.toml` if present.
    pub fn load_snapshot_editorial_template(
        &self,
        contest_id: &str,
    ) -> Option<SnapshotTemplateMeta> {
        check_id(contest_id).ok()?;
        let rel = format!("contests/{contest_id}/snapshot/editorial-template.toml");
        let raw = self.read_text(&rel).ok()?;
        toml::from_str(&raw).ok()
    }

    /// Reads archived preamble from `contests/<id>/snapshot/preamble.typ` if present and non-empty.
    pub fn load_snapshot_preamble(&self, contest_id: &str) -> Option<String> {
        check_id(contest_id).ok()?;
        let rel = format!("contests/{contest_id}/snapshot/preamble.typ");
        let text = self.read_text(&rel).ok()?;
        (!text.trim().is_empty()).then_some(text)
    }

    /// Reads archived template metadata from `trds/<id>/snapshot/template.toml` if present.
    pub fn load_trd_snapshot_template(&self, trd_id: &str) -> Option<SnapshotTemplateMeta> {
        check_id(trd_id).ok()?;
        let rel = format!("trds/{trd_id}/snapshot/template.toml");
        let raw = self.read_text(&rel).ok()?;
        toml::from_str(&raw).ok()
    }

    /// Reads archived preamble from `trds/<id>/snapshot/preamble.typ` if present and non-empty.
    pub fn load_trd_snapshot_preamble(&self, trd_id: &str) -> Option<String> {
        check_id(trd_id).ok()?;
        let rel = format!("trds/{trd_id}/snapshot/preamble.typ");
        let text = self.read_text(&rel).ok()?;
        (!text.trim().is_empty()).then_some(text)
    }

    /// Returns `true` when `trds/<trd_id>/template.typ` exists inside the workspace.
    pub fn has_trd_template(&self, trd_id: &str) -> bool {
        check_id(trd_id).is_ok()
            && self
                .root
                .join("trds")
                .join(trd_id)
                .join("template.typ")
                .is_file()
    }

    /// Lists available workspace template names by scanning `templates/*.typ` on disk,
    /// excluding the reserved `preamble` file (`templates/preamble.typ`).
    pub fn list_templates(&self) -> Vec<String> {
        let mut names = Vec::new();
        if let Ok(entries) = fs::read_dir(self.root.join("templates")) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file()
                    && path.extension().and_then(|e| e.to_str()) == Some("typ")
                    && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
                    && stem != RESERVED_PREAMBLE_NAME
                    && check_id(stem).is_ok()
                {
                    names.push(stem.to_owned());
                }
            }
            names.sort();
            names.dedup();
        }
        names
    }

    /// Returns a handle to this workspace wired to `tracker` so writes performed through
    /// it are automatically suppressed by a [`WorkspaceWatcher`].
    pub fn with_write_tracker(&self, tracker: SelfWriteTracker) -> Self {
        Self {
            root: self.root.clone(),
            read_only: self.read_only,
            self_writes: Some(tracker),
        }
    }

    /// Starts recursively watching this workspace and returns `(tracked_ws, watcher, rx)`.
    /// Writes through `tracked_ws` are automatically filtered out as self-writes.
    pub fn watch(&self) -> Result<(Self, WorkspaceWatcher, mpsc::Receiver<WatchEvent>)> {
        let (watcher, rx) = WorkspaceWatcher::start(&self.root)?;
        let tracked = self.with_write_tracker(watcher.tracker());
        Ok((tracked, watcher, rx))
    }

    /// The path on disk of `rel` (relative to the workspace, `/`-separated), if it stays
    /// inside the workspace: no absolute paths, no `..`, and no symbolic link out of it.
    pub fn resolve(&self, rel: &str) -> Result<PathBuf> {
        let escape = || Error::PathEscape(rel.to_owned());
        let mut path = self.root.clone();
        let mut checking = true;
        for c in Path::new(rel).components() {
            match c {
                Component::Normal(part) => {
                    path.push(part);
                    if checking {
                        match fs::symlink_metadata(&path) {
                            Ok(meta) => {
                                if meta.file_type().is_symlink() {
                                    match canonicalize_clean(&path) {
                                        Ok(real) => {
                                            if !real.starts_with(&self.root) {
                                                return Err(escape());
                                            }
                                        }
                                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                                            if let Ok(target) = fs::read_link(&path) {
                                                let clean_target = strip_verbatim_prefix(&target);
                                                if clean_target.is_absolute()
                                                    && !clean_target.starts_with(&self.root)
                                                {
                                                    return Err(escape());
                                                }
                                            }
                                            checking = false;
                                        }
                                        Err(e) => return Err(io(path.display(), e)),
                                    }
                                }
                            }
                            Err(e)
                                if matches!(
                                    e.kind(),
                                    std::io::ErrorKind::NotFound
                                        | std::io::ErrorKind::NotADirectory
                                ) =>
                            {
                                checking = false;
                            }
                            Err(e) => return Err(io(path.display(), e)),
                        }
                    }
                }
                Component::CurDir => {}
                _ => return Err(escape()),
            }
        }
        Ok(path)
    }

    /// Reads raw bytes of a workspace file.
    pub fn read_bytes(&self, rel: &str) -> Result<Vec<u8>> {
        let path = self.resolve(rel)?;
        fs::read(&path).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => Error::NotFound(rel.to_owned()),
            _ => io(rel, e),
        })
    }

    /// Reads a text file.
    pub fn read_text(&self, rel: &str) -> Result<String> {
        let path = self.resolve(rel)?;
        fs::read_to_string(&path).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => Error::NotFound(rel.to_owned()),
            _ => io(rel, e),
        })
    }

    /// Reads a text file together with its lowercase SHA-256 hex digest.
    pub fn read_text_with_hash(&self, rel: &str) -> Result<(String, String)> {
        let text = self.read_text(rel)?;
        let hash = content_hash(text.as_bytes());
        Ok((text, hash))
    }

    fn read_optional(&self, rel: &str) -> Result<Option<String>> {
        match self.read_text(rel) {
            Ok(text) => Ok(Some(text)),
            Err(Error::NotFound(_)) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Whether writing `rel` is allowed: not read-only, and not in a frozen contest.
    pub fn check_writable(&self, rel: &str) -> Result<()> {
        if self.read_only {
            return Err(Error::ReadOnly);
        }
        let path = self.resolve(rel)?;
        let inside = path.strip_prefix(self.root.join("contests")).ok();
        if let Some(id) = inside.and_then(|p| p.components().next()) {
            let id = id.as_os_str().to_string_lossy();
            if inside.is_some_and(|p| p.components().count() > 1)
                && check_id(&id).is_ok()
                && let Ok(c) = self.load_contest(&id)
            {
                if c.meta.archived {
                    return Err(Error::Archived(id.into_owned()));
                }
                if c.meta.frozen {
                    return Err(Error::Frozen(id.into_owned()));
                }
            }
        }
        let inside_trd = path.strip_prefix(self.root.join("trds")).ok();
        if let Some(id) = inside_trd.and_then(|p| p.components().next()) {
            let id = id.as_os_str().to_string_lossy();
            if inside_trd.is_some_and(|p| p.components().count() > 1)
                && check_id(&id).is_ok()
                && let Ok(trd) = self.load_trd(&id)
            {
                if trd.meta.archived {
                    return Err(Error::TrdArchived(id.into_owned()));
                }
                if trd.meta.frozen {
                    return Err(Error::TrdFrozen(id.into_owned()));
                }
            }
        }
        Ok(())
    }

    /// Writes `bytes` to `rel` atomically, creating its directory.
    pub fn write(&self, rel: &str, bytes: &[u8]) -> Result<()> {
        self.write_checked(rel, bytes, None).map(|_| ())
    }

    /// Atomically writes `bytes` to `rel`, returning its new SHA-256 hex digest.
    ///
    /// When `expected_hash` is `Some(expected)`, verifies that the file currently on
    /// disk hashes to `expected` (with `""` matching a missing file) and returns
    /// [`Error::Conflict`] if another writer modified it.
    pub fn write_checked(
        &self,
        rel: &str,
        bytes: &[u8],
        expected_hash: Option<&str>,
    ) -> Result<String> {
        self.check_writable(rel)?;
        let path = self.resolve(rel)?;

        if let Some(expected) = expected_hash {
            let actual = match fs::read(&path) {
                Ok(existing) => content_hash(&existing),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
                Err(e) => return Err(io(rel, e)),
            };
            if actual != expected {
                return Err(Error::Conflict {
                    path: rel.to_owned(),
                    expected: expected.to_owned(),
                    actual,
                });
            }
        }

        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| io(rel, e))?;
            // Creating it may have gone through a link: check again.
            self.resolve(rel)?;
        }
        self.commit_atomic(rel, &path, bytes)
    }

    /// Internal write path used exclusively by [`Workspace::unfreeze_contest`] to flip
    /// `frozen = false` on a frozen contest while still enforcing read-only & sandbox.
    pub(crate) fn write_unfreeze_bypass(&self, rel: &str, bytes: &[u8]) -> Result<String> {
        if self.read_only {
            return Err(Error::ReadOnly);
        }
        let path = self.resolve(rel)?;
        self.commit_atomic(rel, &path, bytes)
    }

    fn commit_atomic(&self, rel: &str, path: &Path, bytes: &[u8]) -> Result<String> {
        let new_hash = content_hash(bytes);
        if let Some(tracker) = &self.self_writes {
            tracker.record(rel, &new_hash);
        }
        if let Err(e) = write_atomic(path, bytes) {
            if let Some(tracker) = &self.self_writes {
                tracker.unrecord(rel);
            }
            return Err(io(rel, e));
        }
        Ok(new_hash)
    }

    // ---- Problems ----------------------------------------------------------------------

    /// The IDs of the problems (directories of `problems/` with a `problem.toml`), sorted.
    pub fn list_problems(&self) -> Result<Vec<String>> {
        self.list_dirs("problems", "problem.toml")
    }

    /// Loads only `problems/<id>/problem.toml` without reading statements, editorial, or samples.
    pub fn load_problem_meta(&self, id: &str) -> Result<ProblemMeta> {
        check_id(id)?;
        let meta_path = format!("problems/{id}/problem.toml");
        let raw = self.read_text(&meta_path).map_err(|e| match e {
            Error::NotFound(_) => Error::NotFound(format!("problem {id:?}")),
            other => other,
        })?;
        parse_toml(&meta_path, &raw)
    }

    pub fn load_problem(&self, id: &str) -> Result<Problem> {
        check_id(id)?;
        self.load_problem_at(id, &format!("problems/{id}"))
            .map_err(|e| match e {
                Error::NotFound(p) if p == format!("problems/{id}/problem.toml") => {
                    Error::NotFound(format!("problem {id:?}"))
                }
                e => e,
            })
    }

    fn load_problem_at(&self, id: &str, dir: &str) -> Result<Problem> {
        let meta_path = format!("{dir}/problem.toml");
        let meta: ProblemMeta = parse_toml(&meta_path, &self.read_text(&meta_path)?)?;
        let mut statements = Vec::new();
        for language in [None, Some(Language::Chinese), Some(Language::English)] {
            let name = match language {
                None => "statement.md".to_owned(),
                Some(l) => format!("statement.{}.md", l.suffix()),
            };
            let path = format!("{dir}/{name}");
            if let Some(text) = self.read_optional(&path)? {
                statements.push(Statement {
                    language,
                    file: TextFile { path, text },
                });
            }
        }
        let editorial_path = format!("{dir}/editorial.md");
        let editorial = self.read_optional(&editorial_path)?.map(|text| TextFile {
            path: editorial_path,
            text,
        });
        Ok(Problem {
            id: id.to_owned(),
            dir: dir.to_owned(),
            meta,
            statements,
            samples: self.load_samples(&format!("{dir}/samples"))?,
            editorial,
        })
    }

    /// `samples/<name>.in` with `<name>.ans` (or `.out`), sorted by name.
    ///
    /// Unpaired `.in` files are gracefully skipped so compilation can proceed and
    /// `xcpc-lint` can report them as warnings via [`Self::inspect_samples`].
    fn load_samples(&self, dir: &str) -> Result<Vec<Sample>> {
        let path = self.resolve(dir)?;
        let entries = match fs::read_dir(&path) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(io(dir, e)),
        };
        let mut names: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| !n.starts_with('.'))
            .filter_map(|n| n.strip_suffix(".in").map(str::to_owned))
            .filter(|stem| !stem.is_empty())
            .collect();
        names.sort();
        let mut samples = Vec::with_capacity(names.len());
        for name in names {
            let output = match self.read_optional(&format!("{dir}/{name}.ans"))? {
                Some(text) => text,
                None => match self.read_optional(&format!("{dir}/{name}.out"))? {
                    Some(text) => text,
                    None => continue,
                },
            };
            let input = self.read_text(&format!("{dir}/{name}.in"))?;
            samples.push(Sample {
                name,
                input,
                output,
            });
        }
        Ok(samples)
    }

    /// Inspects a `samples/` directory for valid pairs, orphan `.in` / `.ans` / `.out`
    /// files, and whether the paired stems are contiguously numbered from `01`.
    pub fn inspect_samples(&self, dir: &str) -> Result<SampleInventory> {
        let path = self.resolve(dir)?;
        let entries = match fs::read_dir(&path) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(SampleInventory::default());
            }
            Err(e) => return Err(io(dir, e)),
        };

        let mut inputs: BTreeSet<String> = BTreeSet::new();
        let mut ans_outputs: BTreeSet<String> = BTreeSet::new();
        let mut out_outputs: BTreeSet<String> = BTreeSet::new();

        for entry in entries {
            let entry = entry.map_err(|e| io(dir, e))?;
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            if name.starts_with('.') {
                continue;
            }
            if !self.resolve(&format!("{dir}/{name}"))?.is_file() {
                continue;
            }
            if let Some(stem) = name.strip_suffix(".in").filter(|s| !s.is_empty()) {
                inputs.insert(stem.to_owned());
            } else if let Some(stem) = name.strip_suffix(".ans").filter(|s| !s.is_empty()) {
                ans_outputs.insert(stem.to_owned());
            } else if let Some(stem) = name.strip_suffix(".out").filter(|s| !s.is_empty()) {
                out_outputs.insert(stem.to_owned());
            }
        }

        let mut pairs = Vec::new();
        let mut orphan_inputs = Vec::new();
        for stem in &inputs {
            if ans_outputs.contains(stem) || out_outputs.contains(stem) {
                pairs.push(stem.clone());
            } else {
                orphan_inputs.push(format!("{stem}.in"));
            }
        }

        let mut orphan_outputs = Vec::new();
        for stem in &ans_outputs {
            if !inputs.contains(stem) {
                orphan_outputs.push(format!("{stem}.ans"));
            }
        }
        for stem in &out_outputs {
            if !inputs.contains(stem) {
                orphan_outputs.push(format!("{stem}.out"));
            }
        }
        orphan_outputs.sort();

        let non_contiguous = pairs
            .iter()
            .enumerate()
            .any(|(i, stem)| *stem != format!("{:02}", i + 1));

        Ok(SampleInventory {
            pairs,
            orphan_inputs,
            orphan_outputs,
            non_contiguous,
        })
    }

    /// Saves `samples` into `problems/<id>/samples/` as `01.in`/`01.ans`..`NN.in`/`NN.ans`.
    ///
    /// Strips UTF-8 BOM (`\u{FEFF}`) and `\r`, writes all target files atomically before
    /// pruning surplus old sample files (so reordering never loses data), and removes any
    /// stale `.out` / out-of-range sample files.
    pub fn save_samples(&self, id: &str, samples: &[(String, String)]) -> Result<()> {
        check_id(id)?;
        if self.read_only {
            return Err(Error::ReadOnly);
        }
        let meta_rel = format!("problems/{id}/problem.toml");
        if !self.resolve(&meta_rel)?.is_file() {
            return Err(Error::NotFound(format!("problem {id:?}")));
        }

        let samples_dir_rel = format!("problems/{id}/samples");
        let samples_dir = self.resolve(&samples_dir_rel)?;

        let mut written: HashSet<String> = HashSet::with_capacity(samples.len() * 2);
        for (idx, (input, output)) in samples.iter().enumerate() {
            let stem = format!("{:02}", idx + 1);
            let in_name = format!("{stem}.in");
            let ans_name = format!("{stem}.ans");
            let clean_in = clean_sample_text(input);
            let clean_out = clean_sample_text(output);
            self.write(&format!("{samples_dir_rel}/{in_name}"), clean_in.as_bytes())?;
            self.write(
                &format!("{samples_dir_rel}/{ans_name}"),
                clean_out.as_bytes(),
            )?;
            written.insert(in_name);
            written.insert(ans_name);
        }

        if samples_dir.is_dir() {
            let entries = fs::read_dir(&samples_dir).map_err(|e| io(&samples_dir_rel, e))?;
            for entry in entries {
                let entry = entry.map_err(|e| io(&samples_dir_rel, e))?;
                let Ok(name) = entry.file_name().into_string() else {
                    continue;
                };
                if name.starts_with('.') {
                    continue;
                }
                let is_sample_ext =
                    name.ends_with(".in") || name.ends_with(".ans") || name.ends_with(".out");
                if is_sample_ext && !written.contains(&name) {
                    let file_rel = format!("{samples_dir_rel}/{name}");
                    let file_path = self.resolve(&file_rel)?;
                    if file_path.is_file() {
                        fs::remove_file(&file_path).map_err(|e| io(&file_rel, e))?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Writes a problem's `problem.toml`.
    pub fn save_problem_meta(&self, id: &str, meta: &ProblemMeta) -> Result<()> {
        check_id(id)?;
        if let Some(group) = &meta.group {
            normalize_group_name(group)?;
        }
        let text = toml::to_string(meta).expect("problem metadata serializes");
        self.write(&format!("problems/{id}/problem.toml"), text.as_bytes())
    }

    // ---- Contests ----------------------------------------------------------------------

    /// The IDs of the contests (directories of `contests/` with a `contest.toml`), sorted.
    pub fn list_contests(&self) -> Result<Vec<String>> {
        self.list_dirs("contests", "contest.toml")
    }

    pub fn load_contest(&self, id: &str) -> Result<Contest> {
        check_id(id)?;
        let dir = format!("contests/{id}");
        let path = format!("{dir}/contest.toml");
        let text = self.read_text(&path).map_err(|e| match e {
            Error::NotFound(_) => Error::NotFound(format!("contest {id:?}")),
            e => e,
        })?;
        let meta: ContestMeta = parse_toml(&path, &text)?;
        for item in &meta.problems {
            check_id(&item.id).map_err(|e| Error::Invalid {
                path: path.clone(),
                message: e.to_string(),
            })?;
            if !is_label(&item.label) {
                return Err(Error::Invalid {
                    path: path.clone(),
                    message: format!("invalid label {:?}", item.label),
                });
            }
        }
        Ok(Contest {
            id: id.to_owned(),
            dir,
            meta,
        })
    }

    /// A contest's problems in order: from the problem library, or from the snapshot
    /// (`snapshot/<label>/`) once it is frozen.
    pub fn contest_problems(&self, contest: &Contest) -> Result<Vec<(ContestItem, Problem)>> {
        contest
            .meta
            .problems
            .iter()
            .map(|item| {
                let problem = if contest.meta.frozen {
                    let dir = format!("{}/snapshot/{}", contest.dir, item.label);
                    self.load_problem_at(&item.id, &dir)?
                } else {
                    self.load_problem(&item.id)?
                };
                Ok((item.clone(), problem))
            })
            .collect()
    }

    /// Writes a contest's `contest.toml`. Freezing goes through [`Self::freeze_commit`];
    /// nothing may write a frozen contest except [`Self::unfreeze_contest`].
    pub fn save_contest_meta(&self, id: &str, meta: &ContestMeta) -> Result<()> {
        check_id(id)?;
        let text = toml::to_string(meta).expect("contest metadata serializes");
        self.write(&format!("contests/{id}/contest.toml"), text.as_bytes())
    }

    fn list_dirs(&self, parent: &str, marker: &str) -> Result<Vec<String>> {
        let path = self.resolve(parent)?;
        let entries = match fs::read_dir(&path) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(io(parent, e)),
        };
        let mut ids: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|id| check_id(id).is_ok())
            .filter(|id| path.join(id).join(marker).is_file())
            .collect();
        ids.sort();
        Ok(ids)
    }
}

fn system_time_to_rfc3339_utc(time: SystemTime) -> String {
    let secs = match time.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(err) => -(err.duration().as_secs() as i64),
    };
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let hour = rem / 3_600;
    let minute = (rem % 3_600) / 60;
    let second = rem % 60;

    // Howard Hinnant's civil-from-days algorithm.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = y + i64::from(m <= 2);

    format!("{year:04}-{m:02}-{d:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Extracts a concise plain-text excerpt from the first 1–2 non-empty lines of
/// `editorial.md`, stripping Markdown heading `#`, blockquote, list, and emphasis
/// markers, and capping cleanly at 140 Unicode scalar values.
fn extract_editorial_excerpt(markdown: &str) -> String {
    const MAX_CHARS: usize = 140;
    let mut lines = Vec::with_capacity(2);
    for raw_line in markdown.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with("```")
            || trimmed.starts_with("~~~")
            || is_horizontal_rule(trimmed)
        {
            continue;
        }
        let cleaned = clean_markdown_excerpt_line(trimmed);
        if cleaned.is_empty() {
            continue;
        }
        lines.push(cleaned);
        if lines.len() >= 2 {
            break;
        }
    }
    let joined = lines.join(" ");
    if joined.chars().count() <= MAX_CHARS {
        return joined;
    }
    let truncated: String = joined.chars().take(MAX_CHARS - 1).collect();
    format!("{}…", truncated.trim_end())
}

fn is_horizontal_rule(s: &str) -> bool {
    let mut ch = None;
    let mut count = 0usize;
    for c in s.chars() {
        if c.is_whitespace() {
            continue;
        }
        if matches!(c, '-' | '*' | '_') && ch.is_none_or(|prev| prev == c) {
            ch = Some(c);
            count += 1;
        } else {
            return false;
        }
    }
    count >= 3
}

fn clean_markdown_excerpt_line(line: &str) -> String {
    let mut s = line.trim();
    // Strip leading blockquote markers (`>`).
    while let Some(rest) = s.strip_prefix('>') {
        s = rest.trim_start();
    }
    // Strip ATX heading markers (`#`, `##`, ...).
    let hash_count = s.bytes().take_while(|&b| b == b'#').count();
    if hash_count > 0 {
        let after = &s[hash_count..];
        if after.is_empty() || after.starts_with(char::is_whitespace) {
            s = after.trim_start();
            // Also strip optional closing `#`s.
            s = s.trim_end_matches('#').trim_end();
        }
    }
    // Strip unordered list bullets (`- `, `* `, `+ `).
    if let Some(rest) = s
        .strip_prefix("- ")
        .or_else(|| s.strip_prefix("* "))
        .or_else(|| s.strip_prefix("+ "))
    {
        s = rest.trim_start();
    } else {
        // Strip ordered list prefix (`1. `, `2) `).
        let digits = s.bytes().take_while(|b| b.is_ascii_digit()).count();
        if digits > 0 {
            let after = &s[digits..];
            if let Some(rest) = after
                .strip_prefix(". ")
                .or_else(|| after.strip_prefix(") "))
            {
                s = rest.trim_start();
            }
        }
    }

    // Strip common inline Markdown emphasis/code markers.
    let stripped = s
        .replace("**", "")
        .replace("__", "")
        .replace("~~", "")
        .replace('`', "");

    // Strip paired/boundary single `*` emphasis markers while preserving spaced `*` (e.g. `$a * b$`).
    let mut out = String::with_capacity(stripped.len());
    for token in stripped.split_whitespace() {
        let clean_tok = token.trim_matches('*');
        if clean_tok.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(clean_tok);
    }
    out
}

fn sanitize_template_image_name(raw_name: &str) -> (String, String) {
    let p = Path::new(raw_name);
    let ext_raw = p
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .filter(|e| !e.is_empty() && e.chars().all(|c| c.is_ascii_alphanumeric()));
    let ext = match ext_raw {
        Some(e) => format!(".{e}"),
        None => String::new(),
    };
    let stem_raw = p.file_stem().and_then(|s| s.to_str()).unwrap_or("image");
    let mut stem: String = stem_raw
        .trim()
        .chars()
        .map(|c| {
            if matches!(
                c,
                '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0'
            ) || c.is_control()
                || c.is_whitespace()
            {
                '-'
            } else {
                c
            }
        })
        .collect();
    stem = stem.trim_matches(['.', '-']).to_owned();
    let candidate = format!("{stem}{ext}");
    if stem.is_empty() || !is_safe_asset_name(&candidate) {
        (
            "image".to_owned(),
            if ext.is_empty() {
                ".png".to_owned()
            } else {
                ext
            },
        )
    } else {
        (stem, ext)
    }
}

fn clean_sample_text(s: &str) -> String {
    s.replace(['\u{FEFF}', '\r'], "")
}

/// A problem's label in a contest: `A`, `B`, …, `A1`.
fn is_label(label: &str) -> bool {
    !label.is_empty() && label.len() <= 8 && label.chars().all(|c| c.is_ascii_alphanumeric())
}

pub(crate) fn parse_toml<T: serde::de::DeserializeOwned>(path: &str, text: &str) -> Result<T> {
    toml::from_str(text).map_err(|e| Error::Invalid {
        path: path.to_owned(),
        message: e.to_string().trim_end().to_owned(),
    })
}

pub(crate) fn io(path: impl std::fmt::Display, source: std::io::Error) -> Error {
    Error::Io {
        path: path.to_string(),
        source,
    }
}

/// Writes `bytes` to `path` through a temporary file in the same directory, flushed to
/// disk, then renamed over it: readers see the old file or the new one, never a part.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .ok_or_else(|| std::io::Error::other("the path has no file name"))?;
    let tmp = dir.join(format!(
        ".{}.tmp-{}-{}",
        name.to_string_lossy(),
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod snippet_tests;
#[cfg(test)]
mod tests;
