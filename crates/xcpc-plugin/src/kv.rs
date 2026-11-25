use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::error::Result;

/// Plugin-scoped key-value store.
/// Persists atomically to `<data_dir>/plugin_kv/<plugin_id>.json` when file-backed,
/// or keeps values in memory when `path` is `None`.
#[derive(Debug)]
pub struct KvStore {
    path: Option<PathBuf>,
    data: Mutex<BTreeMap<String, serde_json::Value>>,
}

impl KvStore {
    pub fn in_memory() -> Self {
        Self {
            path: None,
            data: Mutex::new(BTreeMap::new()),
        }
    }

    pub fn open(data_dir: &Path, plugin_id: &str) -> Result<Self> {
        crate::manifest::validate_plugin_id(plugin_id)?;
        let dir = data_dir.join("plugin_kv");
        let path = dir.join(format!("{plugin_id}.json"));
        let map = if path.exists() {
            let raw = std::fs::read_to_string(&path)?;
            if raw.trim().is_empty() {
                BTreeMap::new()
            } else {
                serde_json::from_str(&raw)?
            }
        } else {
            BTreeMap::new()
        };
        Ok(Self {
            path: Some(path),
            data: Mutex::new(map),
        })
    }

    pub fn get(&self, key: &str) -> Result<Option<serde_json::Value>> {
        let guard = self.data.lock().unwrap();
        Ok(guard.get(key).cloned())
    }

    pub fn set(&self, key: &str, value: serde_json::Value) -> Result<()> {
        let mut guard = self.data.lock().unwrap();
        guard.insert(key.to_string(), value);
        self.persist_locked(&guard)
    }

    pub fn delete(&self, key: &str) -> Result<bool> {
        let mut guard = self.data.lock().unwrap();
        let removed = guard.remove(key).is_some();
        if removed {
            self.persist_locked(&guard)?;
        }
        Ok(removed)
    }

    fn persist_locked(&self, map: &BTreeMap<String, serde_json::Value>) -> Result<()> {
        let Some(ref path) = self.path else {
            return Ok(());
        };
        let bytes = serde_json::to_vec_pretty(map)?;
        write_atomic(path, &bytes)
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
