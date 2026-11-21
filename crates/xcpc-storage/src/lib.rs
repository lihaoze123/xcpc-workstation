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

use std::collections::{BTreeSet, HashSet};
use std::fmt::Write as _;
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;

use sha2::{Digest, Sha256};
use xcpc_domain::{
    ContestItem, ContestMeta, Language, MAX_CONTEST_PROBLEMS, Problem, ProblemMeta, Sample,
    SampleInventory, Statement, TextFile, check_id,
};

pub mod assets;
pub mod edit;
pub mod example;
pub mod index;
pub mod watcher;

pub use assets::{
    ASSET_MARKDOWN_DOCS, AssetEntry, AssetRef, CleanUnusedFailure, CleanUnusedResult,
    RenameAssetResult, ScannedAssetRef, is_existing_asset_name, is_safe_asset_name,
    is_windows_reserved_name, read_image_dimensions,
};
pub use example::init_example_workspace;
pub use index::WorkspaceIndex;
pub use watcher::{SelfWriteTracker, WatchEvent, WorkspaceWatcher};

/// The file that marks a workspace's root.
pub const WORKSPACE_FILE: &str = "workspace.toml";

/// Static workspace-level `AGENTS.md` written by [`init_workspace`] when absent.
/// Once created, it belongs to the user and is never overwritten.
pub const WORKSPACE_AGENTS_MD: &str = r#"# XCPC Workspace Guide

This directory is an **XCPC Workstation** workspace (`workspace.toml` marks the root).

## Directory Layout

```text
workspace.toml           # Workspace root marker & settings
problems/<slug>/         # One directory per problem (slug = immutable ID)
  problem.toml           # Metadata, [limits], [[source]] (never store training status here)
  statement.md           # Or statement.zh.md / statement.en.md (Markdown + $LaTeX$)
  editorial.md           # Optional A4 editorial
  samples/               # 01.in, 01.ans, 02.in, 02.ans, ... (do not paste samples into statement.md)
  assets/                # Figures referenced by relative path: ![](assets/fig.png)
contests/<id>/
  contest.toml           # Contest metadata, ordered [[problem]] live refs + metadata overrides
  snapshot/              # Created when frozen (read-only archive, committed to Git)
build/                   # Generated PDFs & temporary outputs (gitignored)
.xcpc/                   # Local caches & derived data (gitignored)
```

## Authoring Rules

- **Problem IDs**: lowercase ASCII letters, digits, `-`, `_`, starting with a letter or digit (`<= 64` chars). Never rename a problem directory after creation.
- **Statements**: use level-2 Markdown headings (`## 输入格式` / `## Input`, `## 输出格式` / `## Output`, `## 交互` / `## Interaction`, `## 提示` / `## Note`). Samples are rendered automatically from `samples/`.
- **Contests**: live references with metadata-only overrides (`title`, `[problem.limits]`, `show-metadata`). Labels `A`–`Z` (max 26 problems) follow the order in `contest.toml`.
- **Frozen contests**: strictly read-only (including `snapshot/`). Unfreezing requires human confirmation via the GUI or `xcpc contest unfreeze` and is not available over MCP.

## CLI & MCP

- `xcpc check <problem|contest> <id> [--editorial] [--json]`
- `xcpc build <problem|contest> <id> [--editorial]`
- `xcpc render <problem|contest> <id> [--page N]`
- `xcpc search` / `xcpc show` / `xcpc contest compose|override|freeze|unfreeze`
- `xcpc mcp [--read-only]`
"#;

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
}

impl Error {
    /// A stable name for machines (`--json`, MCP).
    pub fn code(&self) -> &'static str {
        match self {
            Error::NotFound(_) => "not_found",
            Error::InvalidId(_) => "invalid_id",
            Error::PathEscape(_) => "path_escape",
            Error::Frozen(_) => "contest_frozen",
            Error::ReadOnly => "read_only",
            Error::Invalid { .. } => "invalid_file",
            Error::Io { .. } => "io",
            Error::Conflict { .. } => "conflict",
            Error::TooManyProblems(_) => "too_many_problems",
            Error::Referenced { .. } => "problem_referenced",
            Error::AlreadyFrozen(_) => "already_frozen",
            Error::NotFrozen(_) => "not_frozen",
            Error::DisallowedPath(_) => "disallowed_path",
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
    let dir = dir.as_ref();
    fs::create_dir_all(dir).map_err(|e| io(dir.display(), e))?;
    for sub in ["problems", "contests"] {
        let p = dir.join(sub);
        fs::create_dir_all(&p).map_err(|e| io(p.display(), e))?;
    }

    let ws_file = dir.join(WORKSPACE_FILE);
    if fs::symlink_metadata(&ws_file).is_err() {
        write_atomic(&ws_file, b"").map_err(|e| io(ws_file.display(), e))?;
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

/// A contest as loaded from `contests/<id>/contest.toml`.
#[derive(Debug, Clone, PartialEq)]
pub struct Contest {
    pub id: String,
    /// `contests/<id>`.
    pub dir: String,
    pub meta: ContestMeta,
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
        let root = root.canonicalize().map_err(|e| io(root.display(), e))?;
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
        let start = start.canonicalize().map_err(|e| io(start.display(), e))?;
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
        for c in Path::new(rel).components() {
            match c {
                Component::Normal(part) => path.push(part),
                Component::CurDir => {}
                _ => return Err(escape()),
            }
        }
        // The nearest part that exists, with its links followed, must be inside.
        let existing = path.ancestors().find(|p| p.exists()).unwrap_or(&self.root);
        let real = existing
            .canonicalize()
            .map_err(|e| io(existing.display(), e))?;
        if !real.starts_with(&self.root) {
            return Err(escape());
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
                && self.load_contest(&id).is_ok_and(|c| c.meta.frozen)
            {
                return Err(Error::Frozen(id.into_owned()));
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
mod tests;
