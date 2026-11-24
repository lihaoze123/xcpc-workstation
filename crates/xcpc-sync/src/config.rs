use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::Result;

pub const CONFIG_FILENAME: &str = "sync_config.json";

fn default_true() -> bool {
    true
}

/// Global OJ sync configuration stored atomically at `<dataDir>/sync_config.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncConfig {
    /// Single main handle per platform ID (e.g. `"codeforces" -> "tourist"`).
    #[serde(default)]
    pub handles: BTreeMap<String, String>,
    /// Whether desktop/CLI should silently trigger an incremental sync on startup.
    #[serde(default = "default_true")]
    pub auto_sync_on_startup: bool,
    /// RFC 3339 / ISO 8601 UTC timestamp of the last completed sync.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_synced_at: Option<String>,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            handles: BTreeMap::new(),
            auto_sync_on_startup: true,
            last_synced_at: None,
        }
    }
}

impl SyncConfig {
    /// Returns `<data_dir>/sync_config.json`.
    pub fn path(data_dir: &Path) -> PathBuf {
        data_dir.join(CONFIG_FILENAME)
    }

    /// Loads `<data_dir>/sync_config.json`, returning [`Self::default`] when absent or empty.
    pub fn load(data_dir: &Path) -> Result<Self> {
        let path = Self::path(data_dir);
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = std::fs::read_to_string(&path)?;
        if raw.trim().is_empty() {
            return Ok(Self::default());
        }
        Ok(serde_json::from_str(&raw)?)
    }

    /// Atomically persists `<data_dir>/sync_config.json` (temp file + rename).
    pub fn save(&self, data_dir: &Path) -> Result<()> {
        let path = Self::path(data_dir);
        let bytes = serde_json::to_vec_pretty(self)?;
        write_atomic(&path, &bytes)
    }

    /// Sets the configured handle for `platform` (trimmed; an empty handle clears it).
    pub fn set_handle(&mut self, platform: &str, handle: &str) {
        let key = platform.trim().to_ascii_lowercase();
        if key.is_empty() {
            return;
        }
        let trimmed = handle.trim();
        if trimmed.is_empty() {
            self.handles.remove(&key);
        } else {
            self.handles.insert(key, trimmed.to_string());
        }
    }

    /// Returns the configured non-empty handle for `platform` (case-insensitive platform match).
    pub fn handle_for(&self, platform: &str) -> Option<&str> {
        let needle = platform.trim();
        if needle.is_empty() {
            return None;
        }
        if let Some(val) = self.handles.get(needle) {
            let trimmed = val.trim();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
        let lower = needle.to_ascii_lowercase();
        if let Some(val) = self.handles.get(&lower) {
            let trimmed = val.trim();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
        self.handles
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(needle))
            .map(|(_, v)| v.trim())
            .filter(|v| !v.is_empty())
    }
}

pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
        let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
        tmp.write_all(bytes)?;
        tmp.flush()?;
        tmp.persist(path).map_err(|e| e.error)?;
    } else {
        std::fs::write(path, bytes)?;
    }
    Ok(())
}
