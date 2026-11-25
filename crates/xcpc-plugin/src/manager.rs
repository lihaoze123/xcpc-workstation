use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::builtin::{BUILTIN_PLUGINS, find_builtin};
use crate::credentials::{CredentialStore, MemoryCredentialStore};
use crate::error::{PluginError, Result};
use crate::http::{HttpClient, HttpRequest, MockHttpClient};
use crate::kv::{KvStore, write_atomic};
use crate::manifest::{
    PluginCapability, PluginManifest, PluginPermissionSummary, validate_plugin_id,
};
use crate::runtime::{HostContext, PluginLogEntry, PluginRuntime, SandboxLimits};
use crate::workspace::WorkspaceReader;

pub const OFFICIAL_PLUGIN_REPO_URL: &str =
    "https://raw.githubusercontent.com/lihaoze123/xcpc-plugins/main/index.json";
pub const OFFICIAL_PLUGIN_REPO_NAME: &str = "XCPC Official";
pub const OFFICIAL_PLUGIN_REPO_ENV: &str = "XCPC_OFFICIAL_PLUGIN_REPO";

/// Returns the effective official plugin repository URL, honoring `XCPC_OFFICIAL_PLUGIN_REPO` if set.
pub fn official_repo_url() -> String {
    if let Ok(raw) = std::env::var(OFFICIAL_PLUGIN_REPO_ENV) {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    OFFICIAL_PLUGIN_REPO_URL.to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginOrigin {
    Builtin,
    LocalOverride,
    Local,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InstalledPlugin {
    pub manifest: PluginManifest,
    pub origin: PluginOrigin,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_dir: Option<PathBuf>,
    #[serde(default)]
    pub is_linked: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_target: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginLinkEntry {
    pub id: String,
    pub target: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct PluginLinksFile {
    #[serde(default)]
    links: Vec<PluginLinkEntry>,
}

#[derive(Debug, Clone)]
pub struct LoadedPlugin {
    pub info: InstalledPlugin,
    pub entry_source: String,
    pub ui_source: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct PluginStateEntry {
    enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct PluginsStateFile {
    #[serde(default)]
    plugins: BTreeMap<String, PluginStateEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginRepoConfig {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct PluginReposFile {
    #[serde(default)]
    repos: Vec<PluginRepoConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoPluginEntry {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    pub download_url: String,
    pub sha256: String,
    #[serde(default)]
    pub capabilities: Vec<PluginCapability>,
    #[serde(default)]
    pub has_ui: bool,
    #[serde(default)]
    pub official: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permissions: Option<PluginPermissionSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoCatalog {
    pub name: String,
    #[serde(default)]
    pub plugins: Vec<RepoPluginEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScaffoldKind {
    Oj,
    Tool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginTestReport {
    pub plugin_id: String,
    pub action: String,
    pub duration_ms: u64,
    pub result: serde_json::Value,
    pub logs: Vec<PluginLogEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginCheckIssue {
    pub severity: String,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginCheckReport {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default)]
    pub exports: Vec<String>,
    #[serde(default)]
    pub has_ui: bool,
    #[serde(default)]
    pub issues: Vec<PluginCheckIssue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginPackReport {
    pub plugin_id: String,
    pub version: String,
    pub output_path: PathBuf,
    pub sha256: String,
    pub size_bytes: u64,
    pub files: Vec<String>,
}

pub struct PluginManager {
    data_dir: PathBuf,
}

impl PluginManager {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
        }
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn plugins_dir(&self) -> PathBuf {
        self.data_dir.join("plugins")
    }

    fn state_path(&self) -> PathBuf {
        self.data_dir.join("plugins_state.json")
    }

    fn repos_path(&self) -> PathBuf {
        self.data_dir.join("plugin_repos.json")
    }

    pub fn links_path(&self) -> PathBuf {
        self.data_dir.join("plugin_links.json")
    }

    pub fn list_links(&self) -> Result<Vec<PluginLinkEntry>> {
        let path = self.links_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let raw = std::fs::read_to_string(path)?;
        if raw.trim().is_empty() {
            return Ok(Vec::new());
        }
        let file: PluginLinksFile = serde_json::from_str(&raw)?;
        Ok(file.links)
    }

    fn save_links(&self, links: &[PluginLinkEntry]) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(&PluginLinksFile {
            links: links.to_vec(),
        })?;
        write_atomic(&self.links_path(), &bytes)
    }

    fn load_state(&self) -> Result<PluginsStateFile> {
        let path = self.state_path();
        if !path.exists() {
            return Ok(PluginsStateFile::default());
        }
        let raw = std::fs::read_to_string(path)?;
        if raw.trim().is_empty() {
            return Ok(PluginsStateFile::default());
        }
        Ok(serde_json::from_str(&raw)?)
    }

    fn save_state(&self, state: &PluginsStateFile) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(state)?;
        write_atomic(&self.state_path(), &bytes)
    }

    /// Resolves a candidate directory inside `<dataDir>/plugins/` enforcing the symlink security model:
    /// - Regular directories must canonicalize inside `<dataDir>/plugins/`.
    /// - Symlinks are allowed ONLY when `(dir_name, canonical_target)` is explicitly registered in `plugin_links.json`.
    fn validate_plugin_root(
        &self,
        entry_path: &Path,
        dir_name: &str,
        canonical_plugins_dir: &Path,
        links: &[PluginLinkEntry],
    ) -> Result<Option<(PathBuf, bool, Option<PathBuf>)>> {
        let meta = match std::fs::symlink_metadata(entry_path) {
            Ok(m) => m,
            Err(_) => return Ok(None),
        };
        if meta.file_type().is_symlink() {
            let Some(reg) = links.iter().find(|l| l.id == dir_name) else {
                // Unregistered symlink is strictly rejected.
                return Err(PluginError::ZipSlip(format!(
                    "unregistered symlink plugin '{}'",
                    entry_path.display()
                )));
            };
            let canonical_entry = std::fs::canonicalize(entry_path).map_err(|e| {
                PluginError::InvalidPackage(format!(
                    "broken plugin symlink '{}': {e}",
                    entry_path.display()
                ))
            })?;
            let canonical_registered = std::fs::canonicalize(&reg.target).map_err(|e| {
                PluginError::InvalidPackage(format!(
                    "broken registered plugin link target '{}': {e}",
                    reg.target
                ))
            })?;
            if canonical_entry != canonical_registered || !canonical_entry.is_dir() {
                return Err(PluginError::ZipSlip(format!(
                    "symlink '{}' does not match registered target '{}'",
                    entry_path.display(),
                    reg.target
                )));
            }
            Ok(Some((canonical_entry.clone(), true, Some(canonical_entry))))
        } else if meta.is_dir() {
            let canonical_entry = std::fs::canonicalize(entry_path)?;
            if !canonical_entry.starts_with(canonical_plugins_dir) {
                return Err(PluginError::ZipSlip(format!(
                    "plugin directory '{}' escapes plugins root",
                    entry_path.display()
                )));
            }
            Ok(Some((canonical_entry, false, None)))
        } else {
            Ok(None)
        }
    }

    /// Lists all discovered plugins (built-ins + local overrides/installs/registered links, local wins on ID collision).
    pub fn list_plugins(&self) -> Result<Vec<InstalledPlugin>> {
        let state = self.load_state()?;
        let links = self.list_links().unwrap_or_default();
        let mut by_id: BTreeMap<String, InstalledPlugin> = BTreeMap::new();

        for builtin in BUILTIN_PLUGINS {
            let manifest = PluginManifest::from_toml_str(builtin.manifest_toml)?;
            let st = state.plugins.get(&manifest.id);
            by_id.insert(
                manifest.id.clone(),
                InstalledPlugin {
                    manifest,
                    origin: PluginOrigin::Builtin,
                    enabled: st.map(|s| s.enabled).unwrap_or(true),
                    last_error: st.and_then(|s| s.last_error.clone()),
                    local_dir: None,
                    is_linked: false,
                    link_target: None,
                },
            );
        }

        let plugins_dir = self.plugins_dir();
        if plugins_dir.is_dir() {
            let canonical_plugins_dir = std::fs::canonicalize(&plugins_dir)?;
            for entry in std::fs::read_dir(&plugins_dir)? {
                let entry = entry?;
                let path = entry.path();
                let Some(dir_name) = entry.file_name().to_str().map(str::to_string) else {
                    continue;
                };
                if dir_name.starts_with('.') {
                    continue;
                }
                let Ok(Some((resolved_root, is_linked, link_target))) =
                    self.validate_plugin_root(&path, &dir_name, &canonical_plugins_dir, &links)
                else {
                    continue;
                };
                let manifest_raw =
                    match read_plugin_file_safe(&resolved_root, "manifest.toml", "manifest") {
                        Ok(raw) => raw,
                        Err(_) => continue,
                    };
                let manifest = match PluginManifest::from_toml_str(&manifest_raw) {
                    Ok(m) => m,
                    Err(_) => continue,
                };
                // For linked plugins, enforce that manifest.id matches the registered symlink name.
                if is_linked && manifest.id != dir_name {
                    continue;
                }
                let origin = if find_builtin(&manifest.id).is_some() {
                    PluginOrigin::LocalOverride
                } else {
                    PluginOrigin::Local
                };
                let st = state.plugins.get(&manifest.id);
                by_id.insert(
                    manifest.id.clone(),
                    InstalledPlugin {
                        manifest,
                        origin,
                        enabled: st.map(|s| s.enabled).unwrap_or(true),
                        last_error: st.and_then(|s| s.last_error.clone()),
                        local_dir: Some(resolved_root),
                        is_linked,
                        link_target,
                    },
                );
            }
        }

        Ok(by_id.into_values().collect())
    }

    /// Loads a plugin's manifest and JS sources by ID.
    pub fn load_plugin(&self, id: &str) -> Result<LoadedPlugin> {
        // Explicit check: if `<plugins_dir>/<id>` exists as an unregistered symlink, return a clear security error.
        let direct_path = self.plugins_dir().join(id);
        if let Ok(meta) = std::fs::symlink_metadata(&direct_path)
            && meta.file_type().is_symlink()
        {
            let canonical_plugins_dir = std::fs::canonicalize(self.plugins_dir())?;
            let links = self.list_links().unwrap_or_default();
            let _ = self.validate_plugin_root(&direct_path, id, &canonical_plugins_dir, &links)?;
        }

        let plugins = self.list_plugins()?;
        let info = plugins
            .into_iter()
            .find(|p| p.manifest.id == id)
            .ok_or_else(|| PluginError::NotFound(id.to_string()))?;

        if let Some(ref dir) = info.local_dir {
            let entry_source = read_plugin_file_safe(dir, &info.manifest.entry, "entry")?;
            let ui_source = match info.manifest.ui_entry {
                Some(ref ui) => Some(read_plugin_file_safe(dir, ui, "ui_entry")?),
                None => None,
            };
            Ok(LoadedPlugin {
                info,
                entry_source,
                ui_source,
            })
        } else if let Some(builtin) = find_builtin(id) {
            Ok(LoadedPlugin {
                info,
                entry_source: builtin.index_js.to_string(),
                ui_source: builtin.ui_js.map(str::to_string),
            })
        } else {
            Err(PluginError::NotFound(id.to_string()))
        }
    }

    /// Links a local development plugin directory into `<dataDir>/plugins/<id>` and registers it in `plugin_links.json`.
    pub fn link_plugin_dir(&self, dir: &Path) -> Result<InstalledPlugin> {
        if !dir.is_dir() {
            return Err(PluginError::InvalidPackage(format!(
                "plugin directory does not exist: '{}'",
                dir.display()
            )));
        }
        let canonical_target = std::fs::canonicalize(dir)?;
        let manifest_raw = read_plugin_file_safe(&canonical_target, "manifest.toml", "manifest")?;
        let manifest = PluginManifest::from_toml_str(&manifest_raw)?;
        validate_plugin_id(&manifest.id)?;
        // Validate entry files exist and do not escape `canonical_target`.
        let _ = read_plugin_file_safe(&canonical_target, &manifest.entry, "entry")?;
        if let Some(ref ui) = manifest.ui_entry {
            let _ = read_plugin_file_safe(&canonical_target, ui, "ui_entry")?;
        }

        let plugins_dir = self.plugins_dir();
        std::fs::create_dir_all(&plugins_dir)?;
        let link_path = plugins_dir.join(&manifest.id);

        if let Ok(meta) = std::fs::symlink_metadata(&link_path) {
            if meta.file_type().is_symlink() {
                remove_symlink_path(&link_path)?;
            } else {
                return Err(PluginError::InvalidPackage(format!(
                    "plugin '{}' is already installed locally at '{}'; remove it before linking",
                    manifest.id,
                    link_path.display()
                )));
            }
        }

        create_dir_symlink(&canonical_target, &link_path)?;

        let mut links = self.list_links()?;
        links.retain(|l| l.id != manifest.id);
        links.push(PluginLinkEntry {
            id: manifest.id.clone(),
            target: canonical_target.to_string_lossy().to_string(),
        });
        self.save_links(&links)?;

        let plugins = self.list_plugins()?;
        plugins
            .into_iter()
            .find(|p| p.manifest.id == manifest.id)
            .ok_or(PluginError::NotFound(manifest.id))
    }

    /// Unlinks a registered development plugin symlink without touching the developer's source directory.
    pub fn unlink_plugin(&self, id: &str) -> Result<bool> {
        validate_plugin_id(id)?;
        let mut links = self.list_links()?;
        let before = links.len();
        links.retain(|l| l.id != id);
        let removed_reg = links.len() != before;
        if removed_reg {
            self.save_links(&links)?;
        }

        let link_path = self.plugins_dir().join(id);
        let mut removed_link = false;
        if let Ok(meta) = std::fs::symlink_metadata(&link_path)
            && meta.file_type().is_symlink()
        {
            remove_symlink_path(&link_path)?;
            removed_link = true;
        }
        Ok(removed_reg || removed_link)
    }

    /// Removes a locally installed or linked plugin. Never deletes the target of a symlinked dev plugin.
    pub fn uninstall_plugin(&self, id: &str) -> Result<bool> {
        validate_plugin_id(id)?;
        let path = self.plugins_dir().join(id);
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            return self.unlink_plugin(id);
        };
        if meta.file_type().is_symlink() {
            return self.unlink_plugin(id);
        }
        if meta.is_dir() {
            std::fs::remove_dir_all(&path)?;
            let mut links = self.list_links().unwrap_or_default();
            let before = links.len();
            links.retain(|l| l.id != id);
            if links.len() != before {
                let _ = self.save_links(&links);
            }
            return Ok(true);
        }
        Ok(false)
    }

    pub fn kv_store(&self, plugin_id: &str) -> Result<KvStore> {
        KvStore::open(&self.data_dir, plugin_id)
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<()> {
        // Ensure plugin exists.
        let _ = self.load_plugin(id)?;
        let mut state = self.load_state()?;
        let entry = state.plugins.entry(id.to_string()).or_default();
        entry.enabled = enabled;
        self.save_state(&state)
    }

    pub fn record_error(&self, id: &str, err_msg: impl Into<String>) -> Result<()> {
        let mut state = self.load_state()?;
        let entry = state
            .plugins
            .entry(id.to_string())
            .or_insert_with(|| PluginStateEntry {
                enabled: true,
                last_error: None,
            });
        entry.last_error = Some(err_msg.into());
        self.save_state(&state)
    }

    pub fn clear_error(&self, id: &str) -> Result<()> {
        let mut state = self.load_state()?;
        if let Some(entry) = state.plugins.get_mut(id) {
            entry.last_error = None;
            self.save_state(&state)?;
        }
        Ok(())
    }

    /// Exports a built-in plugin's embedded files into `<dataDir>/plugins/<id>/` so users can customize it.
    pub fn export_builtin_copy(&self, id: &str) -> Result<PathBuf> {
        let builtin = find_builtin(id).ok_or_else(|| PluginError::NotFound(id.to_string()))?;
        let dest = self.plugins_dir().join(builtin.id);
        std::fs::create_dir_all(&dest)?;
        write_atomic(
            &dest.join("manifest.toml"),
            builtin.manifest_toml.as_bytes(),
        )?;
        write_atomic(&dest.join("index.js"), builtin.index_js.as_bytes())?;
        if let Some(ui) = builtin.ui_js {
            write_atomic(&dest.join("ui.js"), ui.as_bytes())?;
        }
        Ok(dest)
    }

    /// Installs a plugin from a local directory into `<dataDir>/plugins/<id>/`.
    pub fn install_from_dir(&self, src: &Path) -> Result<InstalledPlugin> {
        let manifest_path = src.join("manifest.toml");
        if !manifest_path.is_file() {
            return Err(PluginError::InvalidPackage(
                "missing manifest.toml in plugin directory".into(),
            ));
        }
        let manifest = PluginManifest::from_file(&manifest_path)?;
        let entry_path = src.join(&manifest.entry);
        if !entry_path.is_file() {
            return Err(PluginError::InvalidPackage(format!(
                "missing entry file '{}'",
                manifest.entry
            )));
        }
        if let Some(ref ui) = manifest.ui_entry
            && !src.join(ui).is_file()
        {
            return Err(PluginError::InvalidPackage(format!(
                "missing ui_entry file '{ui}'"
            )));
        }

        let plugins_dir = self.plugins_dir();
        std::fs::create_dir_all(&plugins_dir)?;
        let dest = plugins_dir.join(&manifest.id);

        let canonical_src = std::fs::canonicalize(src)?;
        if dest.exists()
            && let Ok(canonical_dest) = std::fs::canonicalize(&dest)
            && (canonical_src == canonical_dest || canonical_dest.starts_with(&canonical_src))
        {
            return Err(PluginError::InvalidPackage(
                "source and destination plugin directories overlap".into(),
            ));
        }

        // Stage into a sibling temp directory first so any copy failure (e.g. symlink in `src`)
        // never destroys an existing installed plugin.
        let staging = tempfile::Builder::new()
            .prefix(&format!(".{}.staging-", manifest.id))
            .tempdir_in(&plugins_dir)?;
        copy_dir_recursive(src, staging.path())?;

        let was_symlink = std::fs::symlink_metadata(&dest)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false);
        if was_symlink {
            let _ = self.unlink_plugin(&manifest.id);
        }

        let backup = if dest.exists() {
            let backup_dir = tempfile::Builder::new()
                .prefix(&format!(".{}.backup-", manifest.id))
                .tempdir_in(&plugins_dir)?;
            let backup_path = backup_dir.keep();
            std::fs::remove_dir(&backup_path)?;
            std::fs::rename(&dest, &backup_path)?;
            Some(backup_path)
        } else {
            None
        };

        let staging_path = staging.keep();
        if let Err(err) = std::fs::rename(&staging_path, &dest) {
            if let Some(ref backup_path) = backup {
                let _ = std::fs::rename(backup_path, &dest);
            }
            let _ = std::fs::remove_dir_all(&staging_path);
            return Err(err.into());
        }
        if let Some(backup_path) = backup {
            let _ = std::fs::remove_dir_all(backup_path);
        }

        let plugins = self.list_plugins()?;
        plugins
            .into_iter()
            .find(|p| p.manifest.id == manifest.id)
            .ok_or(PluginError::NotFound(manifest.id))
    }

    /// Installs a plugin from a `.zip` file with strict Zip Slip path traversal protection.
    pub fn install_from_zip(&self, zip_path: &Path) -> Result<InstalledPlugin> {
        let bytes = std::fs::read(zip_path)?;
        self.install_from_zip_bytes(&bytes, None)
    }

    /// Installs a plugin from in-memory `.zip` bytes, optionally verifying its SHA-256 hex digest first.
    pub fn install_from_zip_bytes(
        &self,
        zip_bytes: &[u8],
        expected_sha256: Option<&str>,
    ) -> Result<InstalledPlugin> {
        self.install_from_zip_bytes_inner(zip_bytes, expected_sha256, None)
    }

    fn install_from_zip_bytes_inner(
        &self,
        zip_bytes: &[u8],
        expected_sha256: Option<&str>,
        expected_id: Option<&str>,
    ) -> Result<InstalledPlugin> {
        if let Some(expected) = expected_sha256 {
            let actual = sha256_hex(zip_bytes);
            if !actual.eq_ignore_ascii_case(expected.trim()) {
                return Err(PluginError::ChecksumMismatch {
                    expected: expected.trim().to_ascii_lowercase(),
                    actual,
                });
            }
        }

        let staging = tempfile::tempdir()?;
        extract_zip_safe(zip_bytes, staging.path())?;

        // Locate manifest.toml either at archive root or inside a single top-level directory.
        let plugin_root = locate_extracted_plugin_root(staging.path())?;
        if let Some(expected_id) = expected_id {
            let manifest = PluginManifest::from_file(&plugin_root.join("manifest.toml"))?;
            if manifest.id != expected_id {
                return Err(PluginError::InvalidPackage(format!(
                    "extracted plugin id '{}' does not match repository entry id '{expected_id}'",
                    manifest.id
                )));
            }
        }
        self.install_from_dir(&plugin_root)
    }

    // --- Decentralized Repo Subscriptions ---

    pub fn list_repos(&self) -> Result<Vec<PluginRepoConfig>> {
        let path = self.repos_path();
        if !path.exists() {
            return Ok(vec![PluginRepoConfig {
                url: official_repo_url(),
                name: Some(OFFICIAL_PLUGIN_REPO_NAME.to_string()),
            }]);
        }
        let raw = std::fs::read_to_string(path)?;
        if raw.trim().is_empty() {
            return Ok(Vec::new());
        }
        let file: PluginReposFile = serde_json::from_str(&raw)?;
        Ok(file.repos)
    }

    pub fn add_repo(&self, url: impl Into<String>, name: Option<String>) -> Result<()> {
        let url = url.into().trim().to_string();
        if url.is_empty() {
            return Err(PluginError::InvalidPackage(
                "repo URL cannot be empty".into(),
            ));
        }
        let mut repos = self.list_repos()?;
        if let Some(existing) = repos.iter_mut().find(|r| r.url == url) {
            if name.is_some() {
                existing.name = name;
            }
        } else {
            repos.push(PluginRepoConfig { url, name });
        }
        let bytes = serde_json::to_vec_pretty(&PluginReposFile { repos })?;
        write_atomic(&self.repos_path(), &bytes)
    }

    pub fn remove_repo(&self, url: &str) -> Result<bool> {
        let mut repos = self.list_repos()?;
        let before = repos.len();
        repos.retain(|r| r.url != url);
        if repos.len() == before {
            return Ok(false);
        }
        let bytes = serde_json::to_vec_pretty(&PluginReposFile { repos })?;
        write_atomic(&self.repos_path(), &bytes)?;
        Ok(true)
    }

    pub fn fetch_repo_catalog(&self, url: &str, http: &dyn HttpClient) -> Result<RepoCatalog> {
        let resp = http.send(&HttpRequest {
            url: url.to_string(),
            method: "GET".to_string(),
            headers: BTreeMap::new(),
            body: None,
            timeout_ms: Some(15_000),
        })?;
        if !(200..300).contains(&resp.status) {
            return Err(PluginError::Http(format!(
                "HTTP {} fetching repo catalog {url}",
                resp.status
            )));
        }
        let catalog: RepoCatalog = serde_json::from_str(&resp.body)?;
        Ok(catalog)
    }

    pub fn install_from_repo_entry(
        &self,
        entry: &RepoPluginEntry,
        http: &dyn HttpClient,
    ) -> Result<InstalledPlugin> {
        let zip_bytes = http.get_bytes(&entry.download_url)?;
        self.install_from_zip_bytes_inner(&zip_bytes, Some(&entry.sha256), Some(&entry.id))
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for b in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

const MAX_ZIP_FILE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_ZIP_TOTAL_BYTES: u64 = 64 * 1024 * 1024;

/// Reads a plugin-relative file after verifying its canonical path stays within `dir`.
fn read_plugin_file_safe(dir: &Path, rel_path: &str, field_name: &str) -> Result<String> {
    let canonical_dir = std::fs::canonicalize(dir).map_err(|e| {
        PluginError::InvalidPackage(format!(
            "failed to resolve plugin directory '{}': {e}",
            dir.display()
        ))
    })?;
    let candidate = dir.join(rel_path);
    let canonical_file = std::fs::canonicalize(&candidate).map_err(|e| {
        PluginError::InvalidPackage(format!("failed to read {field_name} '{rel_path}': {e}"))
    })?;
    if !canonical_file.starts_with(&canonical_dir) {
        return Err(PluginError::ZipSlip(format!(
            "{field_name} '{rel_path}' escapes plugin directory"
        )));
    }
    std::fs::read_to_string(&canonical_file).map_err(|e| {
        PluginError::InvalidPackage(format!("failed to read {field_name} '{rel_path}': {e}"))
    })
}

/// Extracts a ZIP archive into `dest_dir` while strictly rejecting any path traversal (Zip Slip),
/// symlinks, or decompressed size bombs.
pub fn extract_zip_safe(zip_bytes: &[u8], dest_dir: &Path) -> Result<()> {
    let cursor = Cursor::new(zip_bytes);
    let mut archive = zip::ZipArchive::new(cursor)
        .map_err(|e| PluginError::InvalidPackage(format!("invalid zip archive: {e}")))?;

    let canonical_dest = std::fs::canonicalize(dest_dir)?;
    let mut total_written: u64 = 0;

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| PluginError::InvalidPackage(format!("zip entry error: {e}")))?;

        let raw_name = file.name().to_string();
        if raw_name.contains('\\') || raw_name.starts_with('/') {
            return Err(PluginError::ZipSlip(raw_name));
        }

        // Reject symlinks (Unix mode 0o120000).
        if let Some(mode) = file.unix_mode()
            && (mode & 0o170000) == 0o120000
        {
            return Err(PluginError::ZipSlip(format!("symlink entry '{raw_name}'")));
        }

        let rel_path = Path::new(&raw_name);
        if rel_path.is_absolute() {
            return Err(PluginError::ZipSlip(raw_name));
        }
        for comp in rel_path.components() {
            match comp {
                Component::Normal(_) | Component::CurDir => {}
                Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                    return Err(PluginError::ZipSlip(raw_name));
                }
            }
        }

        let out_path = canonical_dest.join(rel_path);
        if !out_path.starts_with(&canonical_dest) {
            return Err(PluginError::ZipSlip(raw_name));
        }

        if file.is_dir() {
            std::fs::create_dir_all(&out_path)?;
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let remaining_total = MAX_ZIP_TOTAL_BYTES.saturating_sub(total_written);
            let max_for_entry = MAX_ZIP_FILE_BYTES.min(remaining_total);
            let mut out_file = std::fs::File::create(&out_path)?;
            let mut limited = (&mut file).take(max_for_entry.saturating_add(1));
            let written = std::io::copy(&mut limited, &mut out_file)?;
            if written > max_for_entry {
                drop(out_file);
                let _ = std::fs::remove_file(&out_path);
                return Err(PluginError::InvalidPackage(format!(
                    "zip entry '{raw_name}' exceeds decompressed size limit"
                )));
            }
            total_written = total_written.saturating_add(written);
        }
    }

    Ok(())
}

fn locate_extracted_plugin_root(staging_dir: &Path) -> Result<PathBuf> {
    if staging_dir.join("manifest.toml").is_file() {
        return Ok(staging_dir.to_path_buf());
    }
    let mut subdirs = Vec::new();
    for entry in std::fs::read_dir(staging_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            subdirs.push(path);
        }
    }
    if subdirs.len() == 1 && subdirs[0].join("manifest.toml").is_file() {
        return Ok(subdirs.remove(0));
    }
    Err(PluginError::InvalidPackage(
        "zip archive does not contain manifest.toml at root or single top-level directory".into(),
    ))
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            return Err(PluginError::ZipSlip(format!(
                "symlink not permitted in plugin dir: {}",
                entry.path().display()
            )));
        }
        let target = dest.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_recursive(&entry.path(), &target)?;
        } else if file_type.is_file() {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

// --- DX Helpers ---

const XCPC_PLUGIN_DTS: &str = r#"// XCPC Workstation Plugin Host API Definitions

export interface HostHttpRequest {
  url: string;
  method?: "GET" | "POST" | "PUT" | "DELETE" | "PATCH" | "HEAD";
  headers?: Record<string, string>;
  body?: string;
  credentialHeader?:
    | { header: string; key: string; prefix?: string }
    | Record<string, string>;
  timeoutMs?: number;
}

export interface HostHttpResponse {
  status: number;
  headers: Record<string, string>;
  body: string;
}

export interface HtmlElementMatch {
  text: string;
  html: string;
  attrs: Record<string, string>;
}

export interface WorkspaceProblemSummary {
  id: string;
  title: string;
  tags: string[];
  difficulty?: string;
}

export interface WorkspaceProblemDetail extends WorkspaceProblemSummary {
  statement_md?: string;
  editorial_md?: string;
}

export interface RenderedRgba {
  width: number;
  height: number;
  rgba_base64: string;
}

export type HighlightTokenKind =
  | "keyword"
  | "type"
  | "string"
  | "number"
  | "comment"
  | "operator"
  | "ident"
  | "plain";

export interface HighlightToken {
  kind: HighlightTokenKind;
  text: string;
}

export interface HighlightLine {
  tokens: HighlightToken[];
}

export interface HighlightResult {
  language: string;
  lines: HighlightLine[];
}

export interface GraphNode {
  id: string;
  label?: string;
  x?: number;
  y?: number;
  color?: string;
}

export interface GraphEdge {
  from: string;
  to: string;
  weight?: string;
  label?: string;
  directed?: boolean;
}

export type ToolOutputBlock =
  | { type: "text"; title?: string; content: string; monospace?: boolean }
  | { type: "markdown"; title?: string; content: string }
  | { type: "table"; title?: string; headers: string[]; rows: string[][]; align?: string[] }
  | {
      type: "graph";
      title?: string;
      directed?: boolean;
      weighted?: boolean;
      one_indexed?: boolean;
      nodes: GraphNode[];
      edges: GraphEdge[];
      layout?: string;
    }
  | { type: "svg"; title?: string; svg: string; width?: number; height?: number }
  | { type: "image"; title?: string; png_base64: string; width?: number; height?: number };

export interface ToolExecutionResult {
  blocks: ToolOutputBlock[];
  summary?: string;
}

export type PluginCommandAction =
  | { type: "message"; text: string }
  | { type: "open_tool"; plugin_id: string; tool_id?: string; initial_inputs?: unknown }
  | { type: "tool_result"; result: ToolExecutionResult }
  | { type: "none" };

export interface ImportFileInput {
  name: string;
  content_base64: string;
  text?: string;
}

export interface DraftSample {
  input: string;
  output: string;
}

export interface ProblemDraft {
  id?: string;
  title: string;
  statement_md: string;
  editorial_md?: string;
  tags?: string[];
  rating?: number;
  time_limit?: string;
  memory_limit?: string;
  samples: DraftSample[];
  source?: { platform: string; id: string; url?: string };
  assets?: Record<string, string>;
}

export interface ContestDraft {
  id?: string;
  title: string;
  subtitle?: string;
  problems: ProblemDraft[];
}

export type ImportDraft =
  | ({ kind: "problem" } & ProblemDraft)
  | ({ kind: "contest" } & ContestDraft);

export interface ExportContext {
  kind: "problem" | "contest";
  id: string;
  title: string;
  metadata: Record<string, unknown>;
  statement_md?: string;
  editorial_md?: string;
  samples?: DraftSample[];
  pdf_base64?: string;
  problems?: ExportContext[];
}

export interface ExportFileEntry {
  path: string;
  content_base64?: string;
  text?: string;
}

export interface ExportOutput {
  files: ExportFileEntry[];
  summary?: string;
}

export interface PluginModuleExports {
  runTool?(
    toolId: string,
    inputs: Record<string, unknown>,
    ctx?: { plugin_id: string; tool_id: string }
  ): ToolExecutionResult | Promise<ToolExecutionResult>;
  runCommand?(
    commandId: string,
    ctx?: Record<string, unknown>
  ): PluginCommandAction | Promise<PluginCommandAction>;
  importPackage?(
    files: ImportFileInput[],
    importerId?: string
  ): ImportDraft | Promise<ImportDraft>;
  exportPackage?(
    ctx: ExportContext,
    exporterId?: string
  ): ExportOutput | Promise<ExportOutput>;
}

export interface HostApi {
  readonly http: {
    fetch(req: HostHttpRequest): HostHttpResponse;
  };
  readonly credentials: {
    get(key: string): string | null;
  };
  readonly html: {
    select(html: string, cssSelector: string): HtmlElementMatch[];
  };
  readonly kv: {
    get(key: string): unknown | null;
    set(key: string, value: unknown): void;
    delete(key: string): boolean;
  };
  readonly workspace: {
    listProblems(): WorkspaceProblemSummary[];
    getProblem(id: string): WorkspaceProblemDetail | null;
  };
  readonly render: {
    svgToPng(svg: string, scale?: number): string;
    svgToRgba(svg: string, scale?: number): RenderedRgba;
  };
  highlight(
    code: string,
    lang?: "cpp" | "c" | "rust" | "python" | "js" | "ts" | "json" | "toml" | "text" | string
  ): HighlightResult;
  log(level: "debug" | "info" | "warn" | "error", msg: unknown): void;
}

declare global {
  const host: HostApi;
}
"#;

/// Scaffolds a new plugin directory with `manifest.toml`, `index.js`, and `xcpc-plugin.d.ts`.
pub fn init_plugin_scaffold(dir: &Path, kind: ScaffoldKind) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    let folder_name = dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("my-plugin")
        .to_ascii_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>();
    let id = if folder_name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    {
        folder_name
    } else {
        format!("p-{folder_name}")
    };

    let (manifest, index_js) = match kind {
        ScaffoldKind::Oj => (
            format!(
                r#"id = "{id}"
name = "My OJ Plugin"
version = "0.1.0"
description = "Custom OJ data source plugin."
entry = "index.js"
capabilities = ["oj_source"]

[permissions]
domains = ["example.com"]
workspace_read = false
"#
            ),
            r#"/// <reference path="./xcpc-plugin.d.ts" />

function verify(ctx) {
  return { ok: true, handle: ctx.handle };
}

function syncSummary(ctx) {
  return {
    handle: ctx.handle,
    solved_count: 0,
    submission_count: 0,
    solved_problem_keys: [],
  };
}

function normalizeRating(raw) {
  return typeof raw === "number" ? Math.round(raw) : null;
}

function parseProblemUrl(_url) {
  return null;
}

module.exports = { verify, syncSummary, normalizeRating, parseProblemUrl };
"#
            .to_string(),
        ),
        ScaffoldKind::Tool => (
            format!(
                r#"id = "{id}"
name = "My Tool Plugin"
version = "0.1.0"
description = "Custom offline tool plugin."
entry = "index.js"
capabilities = ["tool"]

[permissions]
domains = []
workspace_read = false

[[tools]]
id = "hello"
name = "Hello Tool"
description = "Example tool action."

[[tools.inputs]]
name = "text"
label = "Input Text"
kind = "string"
required = true
"#
            ),
            r#"/// <reference path="./xcpc-plugin.d.ts" />

function runTool(toolId, inputs) {
  if (toolId === "hello") {
    const text = String((inputs && inputs.text) || "");
    return {
      blocks: [
        { type: "text", content: "Hello, " + text + "!" },
      ],
    };
  }
  throw new Error("Unknown tool: " + toolId);
}

module.exports = { runTool };
"#
            .to_string(),
        ),
    };

    write_atomic(&dir.join("manifest.toml"), manifest.as_bytes())?;
    write_atomic(&dir.join("index.js"), index_js.as_bytes())?;
    write_atomic(&dir.join("xcpc-plugin.d.ts"), XCPC_PLUGIN_DTS.as_bytes())?;
    Ok(())
}

/// Runs an exported action directly against a local plugin directory without installing it.
pub fn test_plugin_dir(
    dir: &Path,
    action: &str,
    args_json: &str,
    http_client: Arc<dyn HttpClient>,
    credentials: Arc<dyn CredentialStore>,
    workspace: Option<Arc<dyn WorkspaceReader>>,
) -> Result<PluginTestReport> {
    let manifest = PluginManifest::from_file(&dir.join("manifest.toml"))?;
    let source = read_plugin_file_safe(dir, &manifest.entry, "entry")?;
    if let Some(ref ui) = manifest.ui_entry {
        let _ = read_plugin_file_safe(dir, ui, "ui_entry")?;
    }

    let parsed_args: serde_json::Value = if args_json.trim().is_empty() {
        serde_json::Value::Array(Vec::new())
    } else {
        serde_json::from_str(args_json)?
    };
    let args = match parsed_args {
        serde_json::Value::Array(arr) => arr,
        other => vec![other],
    };

    let host_ctx = HostContext {
        manifest: manifest.clone(),
        http_client,
        credentials,
        kv: Arc::new(KvStore::in_memory()),
        workspace,
        limits: SandboxLimits::default(),
    };

    let start = Instant::now();
    let call = PluginRuntime::call_function(&source, action, &args, &host_ctx)?;
    let duration_ms = start.elapsed().as_millis() as u64;

    Ok(PluginTestReport {
        plugin_id: manifest.id,
        action: action.to_string(),
        duration_ms,
        result: call.value,
        logs: call.logs,
    })
}

fn create_dir_symlink(target: &Path, link_path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link_path)?;
        Ok(())
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_dir(target, link_path)?;
        Ok(())
    }
}

fn remove_symlink_path(link_path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        std::fs::remove_file(link_path)?;
        Ok(())
    }
    #[cfg(windows)]
    {
        std::fs::remove_dir(link_path).or_else(|_| std::fs::remove_file(link_path))?;
        Ok(())
    }
}

fn derive_scaffold_id(dir: &Path) -> String {
    let folder_name = dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("my-plugin")
        .to_ascii_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>();
    if folder_name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    {
        folder_name
    } else {
        format!("p-{folder_name}")
    }
}

/// Scaffolds a Vite + TypeScript/JSX plugin project using `@xcpc/plugin-sdk`.
pub fn init_plugin_scaffold_vite(dir: &Path, kind: ScaffoldKind) -> Result<()> {
    std::fs::create_dir_all(dir.join("src"))?;
    let id = derive_scaffold_id(dir);

    let package_json = format!(
        r#"{{
  "name": "xcpc-plugin-{id}",
  "version": "0.1.0",
  "private": true,
  "type": "module",
  "scripts": {{
    "dev": "vite build --watch",
    "build": "vite build",
    "typecheck": "tsc --noEmit"
  }},
  "devDependencies": {{
    "@xcpc/plugin-sdk": "^0.1.0",
    "typescript": "^5.7.0",
    "vite": "^6.0.0"
  }}
}}
"#
    );

    let vite_config_ts = r#"import { defineConfig } from "vite";
import { xcpcPlugin } from "@xcpc/plugin-sdk/vite";

export default defineConfig({
  plugins: [xcpcPlugin()],
});
"#;

    let tsconfig_json = r#"{
  "compilerOptions": {
    "target": "ES2022",
    "module": "ESNext",
    "moduleResolution": "Bundler",
    "strict": true,
    "noEmit": true,
    "jsx": "react",
    "jsxFactory": "h",
    "jsxFragmentFactory": "Fragment",
    "skipLibCheck": true
  },
  "include": ["src", "vite.config.ts"]
}
"#;

    let gitignore = "node_modules/\ndist/\n*.zip\n";

    let (manifest_toml, index_ts, ui_tsx) = match kind {
        ScaffoldKind::Oj => (
            format!(
                r#"# yaml-language-server: $schema=./node_modules/@xcpc/plugin-sdk/manifest.schema.json
id = "{id}"
name = "My OJ Plugin"
version = "0.1.0"
description = "Custom OJ data source plugin."
entry = "index.js"
capabilities = ["oj_source"]

[permissions]
domains = ["example.com"]
workspace_read = false
"#
            ),
            r#"import type { OjContext, OjSummarySync, OjVerifyResult, ParsedProblemUrl } from "@xcpc/plugin-sdk/host";

export function verify(ctx: OjContext): OjVerifyResult {
  return { ok: true, handle: ctx.handle };
}

export function syncSummary(ctx: OjContext): OjSummarySync {
  return {
    handle: ctx.handle,
    solved_count: 0,
    submission_count: 0,
    solved_problem_keys: [],
  };
}

export function normalizeRating(raw: number): number | null {
  return typeof raw === "number" ? Math.round(raw) : null;
}

export function parseProblemUrl(_url: string): ParsedProblemUrl | null {
  return null;
}
"#
            .to_string(),
            None,
        ),
        ScaffoldKind::Tool => (
            format!(
                r#"# yaml-language-server: $schema=./node_modules/@xcpc/plugin-sdk/manifest.schema.json
id = "{id}"
name = "My Tool Plugin"
version = "0.1.0"
description = "Custom offline tool plugin."
entry = "index.js"
ui_entry = "ui.js"
capabilities = ["tool"]

[permissions]
domains = []
workspace_read = false

[[tools]]
id = "hello"
name = "Hello Tool"
description = "Example tool action."

[[tools.inputs]]
name = "text"
label = "Input Text"
kind = "string"
required = true
"#
            ),
            r#"import type { ToolExecutionResult } from "@xcpc/plugin-sdk/host";

export function runTool(
  toolId: string,
  inputs: Record<string, unknown>,
): ToolExecutionResult {
  if (toolId === "hello") {
    const text = String(inputs?.text ?? "");
    return {
      summary: `Greeted ${text || "world"}`,
      blocks: [
        { type: "text", content: `Hello, ${text || "world"}!` },
      ],
    };
  }
  throw new Error(`Unknown tool: ${toolId}`);
}
"#
            .to_string(),
            Some(
                r#"import { h, signal, ui, useTool, storage } from "@xcpc/plugin-sdk/ui";

export function render() {
  const name = signal<string>(storage.get<string>("name") ?? "XCPC");
  const result = useTool("hello", () => ({ text: name() }));

  return (
    <ui.Col style={{ flex: 1, padding: 16, gap: 12 }}>
      <ui.Card style={{ gap: 8 }}>
        <ui.Text style={{ fontSize: 13, fontWeight: 600 }}>Hello Tool</ui.Text>
        <ui.Input
          value={name}
          placeholder="Type something..."
          onChangeText={(next) => {
            name.set(next);
            storage.set("name", next);
          }}
        />
      </ui.Card>
      <ui.Output
        blocks={() => result.data()?.blocks ?? []}
        summary={() => result.data()?.summary}
      />
    </ui.Col>
  );
}
"#
                .to_string(),
            ),
        ),
    };

    write_atomic(&dir.join("package.json"), package_json.as_bytes())?;
    write_atomic(&dir.join("vite.config.ts"), vite_config_ts.as_bytes())?;
    write_atomic(&dir.join("tsconfig.json"), tsconfig_json.as_bytes())?;
    write_atomic(&dir.join(".gitignore"), gitignore.as_bytes())?;
    write_atomic(&dir.join("manifest.toml"), manifest_toml.as_bytes())?;
    write_atomic(&dir.join("src/index.ts"), index_ts.as_bytes())?;
    if let Some(ui) = ui_tsx {
        write_atomic(&dir.join("src/ui.tsx"), ui.as_bytes())?;
    }
    Ok(())
}

/// Performs static + runtime dry-run validation of a plugin directory.
pub fn check_plugin_dir(dir: &Path) -> PluginCheckReport {
    let mut issues: Vec<PluginCheckIssue> = Vec::new();
    let push_err = |issues: &mut Vec<PluginCheckIssue>, code: &str, msg: String| {
        issues.push(PluginCheckIssue {
            severity: "error".to_string(),
            code: code.to_string(),
            message: msg,
        });
    };

    if !dir.is_dir() {
        push_err(
            &mut issues,
            "DIR_NOT_FOUND",
            format!("plugin directory does not exist: '{}'", dir.display()),
        );
        return PluginCheckReport {
            ok: false,
            plugin_id: None,
            version: None,
            exports: Vec::new(),
            has_ui: false,
            issues,
        };
    }

    let manifest_raw = match read_plugin_file_safe(dir, "manifest.toml", "manifest") {
        Ok(raw) => raw,
        Err(e) => {
            push_err(&mut issues, e.code(), e.to_string());
            return PluginCheckReport {
                ok: false,
                plugin_id: None,
                version: None,
                exports: Vec::new(),
                has_ui: false,
                issues,
            };
        }
    };

    let manifest = match PluginManifest::from_toml_str(&manifest_raw) {
        Ok(m) => m,
        Err(e) => {
            push_err(&mut issues, e.code(), e.to_string());
            return PluginCheckReport {
                ok: false,
                plugin_id: None,
                version: None,
                exports: Vec::new(),
                has_ui: false,
                issues,
            };
        }
    };

    let plugin_id = manifest.id.clone();
    let version = manifest.version.clone();
    let has_ui = manifest.ui_entry.is_some();

    // Check entry file existence + sandbox + dry-run load.
    let mut exports = Vec::new();
    match read_plugin_file_safe(dir, &manifest.entry, "entry") {
        Ok(entry_source) => {
            let host_ctx = HostContext {
                manifest: manifest.clone(),
                http_client: Arc::new(MockHttpClient::new(|_| {
                    Err(PluginError::Http(
                        "network disabled during plugin check".into(),
                    ))
                })),
                credentials: Arc::new(MemoryCredentialStore::default()),
                kv: Arc::new(KvStore::in_memory()),
                workspace: None,
                limits: SandboxLimits::default(),
            };
            match PluginRuntime::list_exports(&entry_source, &host_ctx) {
                Ok(found) => {
                    exports = found;
                    let has_fn = |name: &str| exports.iter().any(|e| e == name);
                    for cap in &manifest.capabilities {
                        match cap {
                            PluginCapability::OjSource => {
                                if !has_fn("normalizeRating") {
                                    push_err(
                                        &mut issues,
                                        "MISSING_EXPORT",
                                        "oj_source capability requires exported function 'normalizeRating'".into(),
                                    );
                                }
                                if !has_fn("syncAccount") && !has_fn("syncSummary") {
                                    push_err(
                                        &mut issues,
                                        "MISSING_EXPORT",
                                        "oj_source capability requires exported function 'syncSummary' or 'syncAccount'".into(),
                                    );
                                }
                            }
                            PluginCapability::Tool => {
                                if !has_fn("runTool") {
                                    push_err(
                                        &mut issues,
                                        "MISSING_EXPORT",
                                        "tool capability requires exported function 'runTool'"
                                            .into(),
                                    );
                                }
                            }
                            PluginCapability::Commands => {
                                if !has_fn("runCommand") {
                                    push_err(
                                        &mut issues,
                                        "MISSING_EXPORT",
                                        "commands capability requires exported function 'runCommand'".into(),
                                    );
                                }
                            }
                            PluginCapability::Importer => {
                                if !has_fn("importPackage") {
                                    push_err(
                                        &mut issues,
                                        "MISSING_EXPORT",
                                        "importer capability requires exported function 'importPackage'".into(),
                                    );
                                }
                            }
                            PluginCapability::Exporter => {
                                if !has_fn("exportPackage") {
                                    push_err(
                                        &mut issues,
                                        "MISSING_EXPORT",
                                        "exporter capability requires exported function 'exportPackage'".into(),
                                    );
                                }
                            }
                            PluginCapability::Credentials => {}
                        }
                    }
                }
                Err(e) => {
                    push_err(&mut issues, e.code(), e.to_string());
                }
            }
        }
        Err(e) => {
            push_err(&mut issues, e.code(), e.to_string());
        }
    }

    // Check optional ui_entry syntax and render(sdk) export contract.
    if let Some(ref ui_rel) = manifest.ui_entry {
        match read_plugin_file_safe(dir, ui_rel, "ui_entry") {
            Ok(ui_source) => {
                let ui_filename = format!("xcpc-plugin://{plugin_id}/{ui_rel}");
                match PluginRuntime::check_ui_source(&ui_source, &ui_filename) {
                    Ok(true) => {}
                    Ok(false) => {
                        push_err(
                            &mut issues,
                            "MISSING_UI_RENDER",
                            format!("ui_entry '{ui_rel}' must export a render(sdk) function"),
                        );
                    }
                    Err(e) => {
                        push_err(&mut issues, e.code(), format!("ui_entry '{ui_rel}': {e}"));
                    }
                }
            }
            Err(e) => {
                push_err(&mut issues, e.code(), e.to_string());
            }
        }
    }

    let ok = !issues.iter().any(|i| i.severity == "error");
    PluginCheckReport {
        ok,
        plugin_id: Some(plugin_id),
        version: Some(version),
        exports,
        has_ui,
        issues,
    }
}

/// Validates and packs a plugin directory into a clean distributable `.zip` containing only whitelisted runtime files.
pub fn pack_plugin_dir(dir: &Path, out_zip: Option<&Path>) -> Result<PluginPackReport> {
    let report = check_plugin_dir(dir);
    if !report.ok {
        let summary = report
            .issues
            .iter()
            .map(|i| format!("[{}] {}", i.code, i.message))
            .collect::<Vec<_>>()
            .join("; ");
        return Err(PluginError::InvalidPackage(format!(
            "plugin check failed: {summary}"
        )));
    }

    let manifest = PluginManifest::from_file(&dir.join("manifest.toml"))?;
    let canonical_dir = std::fs::canonicalize(dir)?;

    let mut rel_files: Vec<String> = Vec::new();
    let mut push_if_safe = |rel: &str| -> Result<()> {
        let normalized = rel.replace('\\', "/");
        if rel_files.contains(&normalized) {
            return Ok(());
        }
        let candidate = canonical_dir.join(&normalized);
        let canonical = std::fs::canonicalize(&candidate)?;
        if !canonical.starts_with(&canonical_dir) {
            return Err(PluginError::ZipSlip(format!(
                "file '{normalized}' escapes plugin directory"
            )));
        }
        if canonical.is_file() {
            rel_files.push(normalized);
        }
        Ok(())
    };

    push_if_safe("manifest.toml")?;
    push_if_safe(&manifest.entry)?;
    if let Some(ref ui) = manifest.ui_entry {
        push_if_safe(ui)?;
    }
    for optional_doc in ["README.md", "LICENSE", "LICENSE.md", "LICENSE.txt"] {
        if canonical_dir.join(optional_doc).is_file() {
            push_if_safe(optional_doc)?;
        }
    }

    let out_path = match out_zip {
        Some(p) => p.to_path_buf(),
        None => dir.join(format!("{}-{}.zip", manifest.id, manifest.version)),
    };

    let mut zip_buf = Cursor::new(Vec::new());
    {
        let mut writer = zip::ZipWriter::new(&mut zip_buf);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(0o644);
        for rel in &rel_files {
            let bytes = std::fs::read(canonical_dir.join(rel))?;
            writer
                .start_file(rel, options)
                .map_err(|e| PluginError::InvalidPackage(format!("zip write error: {e}")))?;
            writer.write_all(&bytes)?;
        }
        writer
            .finish()
            .map_err(|e| PluginError::InvalidPackage(format!("zip finish error: {e}")))?;
    }

    let zip_bytes = zip_buf.into_inner();
    let sha256 = sha256_hex(&zip_bytes);
    let size_bytes = zip_bytes.len() as u64;
    write_atomic(&out_path, &zip_bytes)?;

    Ok(PluginPackReport {
        plugin_id: manifest.id,
        version: manifest.version,
        output_path: out_path,
        sha256,
        size_bytes,
        files: rel_files,
    })
}
