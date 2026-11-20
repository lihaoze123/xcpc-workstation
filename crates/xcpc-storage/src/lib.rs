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

use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use xcpc_domain::{
    ContestItem, ContestMeta, Language, Problem, ProblemMeta, Sample, Statement, TextFile, check_id,
};

/// The file that marks a workspace's root.
pub const WORKSPACE_FILE: &str = "workspace.toml";

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
        }
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

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
}

impl Workspace {
    /// Opens the workspace at `root` (which must exist).
    pub fn open(root: impl AsRef<Path>, read_only: bool) -> Result<Self> {
        let root = root.as_ref();
        let root = root.canonicalize().map_err(|e| io(root.display(), e))?;
        if !root.is_dir() {
            return Err(Error::NotFound(root.display().to_string()));
        }
        Ok(Self { root, read_only })
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

    /// Reads a text file.
    pub fn read_text(&self, rel: &str) -> Result<String> {
        let path = self.resolve(rel)?;
        fs::read_to_string(&path).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => Error::NotFound(rel.to_owned()),
            _ => io(rel, e),
        })
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
        self.check_writable(rel)?;
        let path = self.resolve(rel)?;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| io(rel, e))?;
            // Creating it may have gone through a link: check again.
            self.resolve(rel)?;
        }
        write_atomic(&path, bytes).map_err(|e| io(rel, e))
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
            .filter_map(|n| n.strip_suffix(".in").map(str::to_owned))
            .collect();
        names.sort();
        names
            .into_iter()
            .map(|name| {
                let input = self.read_text(&format!("{dir}/{name}.in"))?;
                let output = match self.read_optional(&format!("{dir}/{name}.ans"))? {
                    Some(text) => text,
                    None => self
                        .read_optional(&format!("{dir}/{name}.out"))?
                        .ok_or_else(|| Error::Invalid {
                            path: format!("{dir}/{name}.in"),
                            message: format!("no {name}.ans (or {name}.out) beside it"),
                        })?,
                };
                Ok(Sample {
                    name,
                    input,
                    output,
                })
            })
            .collect()
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

    /// Writes a contest's `contest.toml`. Freezing goes through here; nothing may
    /// write a frozen one, unfreezing included (that is the GUI's and the CLI's, with
    /// a confirmation, in a later phase).
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

/// A problem's label in a contest: `A`, `B`, …, `A1`.
fn is_label(label: &str) -> bool {
    !label.is_empty() && label.len() <= 8 && label.chars().all(|c| c.is_ascii_alphanumeric())
}

fn parse_toml<T: serde::de::DeserializeOwned>(path: &str, text: &str) -> Result<T> {
    toml::from_str(text).map_err(|e| Error::Invalid {
        path: path.to_owned(),
        message: e.to_string().trim_end().to_owned(),
    })
}

fn io(path: impl std::fmt::Display, source: std::io::Error) -> Error {
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
