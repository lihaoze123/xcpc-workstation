use std::collections::HashMap;
use std::sync::RwLock;

use crate::error::{PluginError, Result};

const KEYRING_SERVICE: &str = "xcpc-workstation-plugin";

pub trait CredentialStore: Send + Sync {
    fn get(&self, plugin_id: &str, key: &str) -> Result<Option<String>>;
    fn set(&self, plugin_id: &str, key: &str, value: &str) -> Result<()>;
    fn delete(&self, plugin_id: &str, key: &str) -> Result<()>;
}

/// System keyring credential store backed by `keyring`, with graceful fallback
/// to an in-memory map when no OS keyring backend is reachable (e.g., headless CI).
#[derive(Debug, Default)]
pub struct SystemKeyringStore {
    fallback: RwLock<HashMap<(String, String), String>>,
}

impl SystemKeyringStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn account_name(plugin_id: &str, key: &str) -> String {
        format!("{plugin_id}:{key}")
    }
}

impl CredentialStore for SystemKeyringStore {
    fn get(&self, plugin_id: &str, key: &str) -> Result<Option<String>> {
        let account = Self::account_name(plugin_id, key);
        match keyring::Entry::new(KEYRING_SERVICE, &account) {
            Ok(entry) => match entry.get_password() {
                Ok(val) => Ok(Some(val)),
                Err(keyring::Error::NoEntry) => {
                    let map = self.fallback.read().unwrap();
                    Ok(map.get(&(plugin_id.to_string(), key.to_string())).cloned())
                }
                Err(_) => {
                    let map = self.fallback.read().unwrap();
                    Ok(map.get(&(plugin_id.to_string(), key.to_string())).cloned())
                }
            },
            Err(_) => {
                let map = self.fallback.read().unwrap();
                Ok(map.get(&(plugin_id.to_string(), key.to_string())).cloned())
            }
        }
    }

    fn set(&self, plugin_id: &str, key: &str, value: &str) -> Result<()> {
        let account = Self::account_name(plugin_id, key);
        if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, &account)
            && entry.set_password(value).is_ok()
        {
            return Ok(());
        }
        let mut map = self.fallback.write().unwrap();
        map.insert((plugin_id.to_string(), key.to_string()), value.to_string());
        Ok(())
    }

    fn delete(&self, plugin_id: &str, key: &str) -> Result<()> {
        let account = Self::account_name(plugin_id, key);
        if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, &account) {
            match entry.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => {}
                Err(e) => {
                    // Best-effort cleanup of fallback as well.
                    let mut map = self.fallback.write().unwrap();
                    if map
                        .remove(&(plugin_id.to_string(), key.to_string()))
                        .is_some()
                    {
                        return Ok(());
                    }
                    return Err(PluginError::Credential(e.to_string()));
                }
            }
        }
        let mut map = self.fallback.write().unwrap();
        map.remove(&(plugin_id.to_string(), key.to_string()));
        Ok(())
    }
}

/// Deterministic in-memory credential store for tests.
#[derive(Debug, Default)]
pub struct MemoryCredentialStore {
    entries: RwLock<HashMap<(String, String), String>>,
}

impl MemoryCredentialStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl CredentialStore for MemoryCredentialStore {
    fn get(&self, plugin_id: &str, key: &str) -> Result<Option<String>> {
        let map = self.entries.read().unwrap();
        Ok(map.get(&(plugin_id.to_string(), key.to_string())).cloned())
    }

    fn set(&self, plugin_id: &str, key: &str, value: &str) -> Result<()> {
        let mut map = self.entries.write().unwrap();
        map.insert((plugin_id.to_string(), key.to_string()), value.to_string());
        Ok(())
    }

    fn delete(&self, plugin_id: &str, key: &str) -> Result<()> {
        let mut map = self.entries.write().unwrap();
        map.remove(&(plugin_id.to_string(), key.to_string()));
        Ok(())
    }
}
