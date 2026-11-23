//! 3-level content-hash cache in `~/.cache/xcpc/` (shared across workspaces).
//!
//! - Level 1 (`bin/`): compiled executables keyed by compiler + flags + source + testlib hash.
//! - Level 2 (`gen/`): generated test inputs keyed by generator binary hash + args.
//! - Level 3 (`run/`): solution execution + checker results keyed by solution binary + input +
//!   limits + checker binary + answer hash.
//!
//! All writes are atomic (temporary file in the same directory + rename) so concurrent
//! threads and processes never observe partially written artifacts.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::assets::TESTLIB_H;
use crate::error::{Result, io_err};

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Handle to the 3-level content-hash cache directory.
#[derive(Debug, Clone)]
pub struct JudgeCache {
    root: PathBuf,
}

impl JudgeCache {
    /// Opens the default cache directory (`XCPC_CACHE_DIR` if set, otherwise `~/.cache/xcpc`).
    pub fn open_default() -> Result<Self> {
        let root = default_cache_root();
        Self::open(root)
    }

    /// Opens a cache at an explicit directory (useful for tests).
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        for sub in ["bin", "gen", "run", "inc", "sanitize/bin"] {
            let dir = root.join(sub);
            fs::create_dir_all(&dir)
                .map_err(|e| io_err(format!("create cache dir {}", dir.display()), e))?;
        }
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Ensures the bundled `testlib.h` is materialized in the cache include directory
    /// and returns that include directory path.
    pub fn ensure_bundled_testlib_dir(&self) -> Result<PathBuf> {
        let hash = xcpc_storage::content_hash(TESTLIB_H.as_bytes());
        let short = &hash[..16];
        let inc_dir = self.root.join("inc").join(short);
        let header = inc_dir.join("testlib.h");
        if header.is_file() {
            return Ok(inc_dir);
        }
        fs::create_dir_all(&inc_dir)
            .map_err(|e| io_err(format!("create testlib cache dir {}", inc_dir.display()), e))?;
        write_atomic(&header, TESTLIB_H.as_bytes())?;
        Ok(inc_dir)
    }

    /// Path for a cached compiled executable (Level 1).
    pub fn binary_path(&self, key: &str) -> PathBuf {
        let ext = if cfg!(windows) { ".exe" } else { "" };
        self.root.join("bin").join(format!("{key}{ext}"))
    }

    /// Path for an isolated sanitizer-compiled executable (never touches normal `bin/` cache).
    pub fn sanitizer_binary_path(&self, key: &str) -> PathBuf {
        let ext = if cfg!(windows) { ".exe" } else { "" };
        self.root
            .join("sanitize")
            .join("bin")
            .join(format!("{key}{ext}"))
    }

    /// Path for a cached generated test input (Level 2).
    pub fn gen_input_path(&self, key: &str) -> PathBuf {
        self.root.join("gen").join(format!("{key}.in"))
    }

    /// Stores generated test input bytes atomically in Level 2 cache.
    pub fn put_gen_input(&self, key: &str, bytes: &[u8]) -> Result<PathBuf> {
        let path = self.gen_input_path(key);
        write_atomic(&path, bytes)?;
        Ok(path)
    }

    /// Path for a cached solution run result (Level 3).
    pub fn run_result_path(&self, key: &str) -> PathBuf {
        self.root.join("run").join(format!("{key}.json"))
    }

    /// Reads a cached JSON value from Level 3 cache, returning `None` on miss or corrupt JSON.
    pub fn get_run_json<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        let bytes = fs::read(self.run_result_path(key)).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// Stores a JSON value atomically in Level 3 cache.
    pub fn put_run_json<T: Serialize>(&self, key: &str, value: &T) -> Result<()> {
        let bytes = serde_json::to_vec(value)
            .map_err(|e| crate::error::JudgeError::Internal(format!("serialize cache: {e}")))?;
        write_atomic(&self.run_result_path(key), &bytes)
    }
}

/// Resolves the default cache root directory (`XCPC_CACHE_DIR` or `~/.cache/xcpc`).
pub fn default_cache_root() -> PathBuf {
    if let Ok(env_dir) = std::env::var("XCPC_CACHE_DIR") {
        let trimmed = env_dir.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    if let Some(base) = dirs::cache_dir() {
        return base.join("xcpc");
    }
    if let Some(home) = dirs::home_dir() {
        return home.join(".cache").join("xcpc");
    }
    std::env::temp_dir().join("xcpc-cache")
}

/// Atomically writes `bytes` to `path` via a temp file in the same directory + rename.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .map_err(|e| io_err(format!("create directory {}", parent.display()), e))?;
    let seq = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("artifact");
    let tmp = parent.join(format!(".{file_name}.{pid}.{seq}.tmp"));
    let write_res = (|| -> std::io::Result<()> {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.flush()?;
        Ok(())
    })();
    if let Err(e) = write_res {
        let _ = fs::remove_file(&tmp);
        return Err(io_err(format!("write {}", path.display()), e));
    }
    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(io_err(format!("rename into {}", path.display()), e));
    }
    Ok(())
}
