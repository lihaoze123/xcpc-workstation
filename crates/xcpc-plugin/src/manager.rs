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
    PluginCapability, PluginManifest, PluginPermissionSummary, requires_trust, validate_plugin_id,
};
use crate::runtime::{HostContext, PluginLogEntry, PluginRuntime, SandboxLimits};
use crate::workspace::WorkspaceReader;

pub const OFFICIAL_PLUGIN_REPO_URL: &str =
    "https://github.com/lihaoze123/xcpc-plugins/releases/download/official-latest/index.json";
pub const LEGACY_OFFICIAL_PLUGIN_REPO_URL: &str =
    "https://raw.githubusercontent.com/lihaoze123/xcpc-plugins/main/index.json";
pub const OFFICIAL_PLUGIN_SDK_PACKAGE: &str = "@xcpc-workstation/plugin-sdk";
pub const OFFICIAL_PLUGIN_SDK_VERSION: &str = "^0.2.0";
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
    #[serde(default)]
    pub pending_trust: bool,
    #[serde(default)]
    pub overrides_builtin: bool,
    #[serde(default)]
    pub has_network: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_repo: Option<String>,
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

#[derive(Debug, Clone)]
pub struct LoadedPluginUiHtml {
    pub plugin_id: String,
    pub html: String,
    pub manifest: PluginManifest,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct TrustedPermissions {
    #[serde(default)]
    domains: Vec<String>,
    #[serde(default)]
    credentials: Vec<String>,
    #[serde(default)]
    workspace_read: bool,
}

impl TrustedPermissions {
    fn from_manifest(manifest: &PluginManifest) -> Self {
        let mut domains: Vec<String> = manifest
            .permissions
            .domains
            .iter()
            .map(|d| d.trim().to_ascii_lowercase())
            .filter(|d| !d.is_empty())
            .collect();
        domains.sort();
        domains.dedup();

        let mut credentials: Vec<String> = manifest
            .credentials
            .iter()
            .map(|c| c.key.trim().to_string())
            .filter(|k| !k.is_empty())
            .collect();
        credentials.sort();
        credentials.dedup();

        Self {
            domains,
            credentials,
            workspace_read: manifest.permissions.workspace_read,
        }
    }

    fn covers(&self, manifest: &PluginManifest) -> bool {
        if manifest.permissions.workspace_read && !self.workspace_read {
            return false;
        }
        for d in &manifest.permissions.domains {
            let norm = d.trim().to_ascii_lowercase();
            if !norm.is_empty() && !self.domains.iter().any(|td| td == &norm) {
                return false;
            }
        }
        for c in &manifest.credentials {
            let key = c.key.trim();
            if !key.is_empty() && !self.credentials.iter().any(|tk| tk == key) {
                return false;
            }
        }
        true
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PluginStateEntry {
    #[serde(default = "default_true")]
    enabled: bool,
    #[serde(default)]
    pending_trust: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    trusted_permissions: Option<TrustedPermissions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_repo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_error: Option<String>,
}

impl Default for PluginStateEntry {
    fn default() -> Self {
        Self {
            enabled: true,
            pending_trust: false,
            trusted_permissions: None,
            source_repo: None,
            last_error: None,
        }
    }
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoFetchError {
    pub repo_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_name: Option<String>,
    pub error: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AggregatedRepoPlugin {
    pub repo_url: String,
    pub repo_name: String,
    pub entry: RepoPluginEntry,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AggregatedRepoCatalog {
    #[serde(default)]
    pub plugins: Vec<AggregatedRepoPlugin>,
    #[serde(default)]
    pub errors: Vec<RepoFetchError>,
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
                return Err(PluginError::SymlinkEscape(format!(
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
                return Err(PluginError::SymlinkEscape(format!(
                    "symlink '{}' does not match registered target '{}'",
                    entry_path.display(),
                    reg.target
                )));
            }
            Ok(Some((canonical_entry.clone(), true, Some(canonical_entry))))
        } else if meta.is_dir() {
            let canonical_entry = std::fs::canonicalize(entry_path)?;
            if !canonical_entry.starts_with(canonical_plugins_dir) {
                return Err(PluginError::SymlinkEscape(format!(
                    "plugin directory '{}' escapes plugins root",
                    entry_path.display()
                )));
            }
            Ok(Some((canonical_entry, false, None)))
        } else {
            Ok(None)
        }
    }

    fn compute_pending_trust(
        manifest: &PluginManifest,
        origin: PluginOrigin,
        st: Option<&PluginStateEntry>,
    ) -> bool {
        if matches!(origin, PluginOrigin::Builtin) || !requires_trust(manifest) {
            return false;
        }
        match st {
            Some(entry) if !entry.pending_trust => !entry
                .trusted_permissions
                .as_ref()
                .is_some_and(|tp| tp.covers(manifest)),
            _ => true,
        }
    }

    fn record_install_trust_state(
        &self,
        manifest: &PluginManifest,
        source_repo: Option<&str>,
    ) -> Result<()> {
        let mut state = self.load_state()?;
        let entry = state.plugins.entry(manifest.id.clone()).or_default();
        let source_changed = entry.source_repo.as_deref() != source_repo;
        entry.source_repo = source_repo.map(str::to_string);
        if requires_trust(manifest) {
            let already_trusted = !source_changed
                && !entry.pending_trust
                && entry
                    .trusted_permissions
                    .as_ref()
                    .is_some_and(|tp| tp.covers(manifest));
            if !already_trusted {
                entry.pending_trust = true;
                entry.trusted_permissions = None;
            }
        } else {
            entry.pending_trust = false;
            if source_changed {
                entry.trusted_permissions = None;
            }
        }
        self.save_state(&state)
    }

    fn clear_trust_state(&self, id: &str) {
        if let Ok(mut state) = self.load_state()
            && let Some(entry) = state.plugins.get_mut(id)
        {
            entry.pending_trust = false;
            entry.trusted_permissions = None;
            entry.source_repo = None;
            let _ = self.save_state(&state);
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
            let has_network = !manifest.permissions.domains.is_empty();
            by_id.insert(
                manifest.id.clone(),
                InstalledPlugin {
                    manifest,
                    origin: PluginOrigin::Builtin,
                    enabled: st.map(|s| s.enabled).unwrap_or(true),
                    pending_trust: false,
                    overrides_builtin: false,
                    has_network,
                    source_repo: None,
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
                let pending_trust = Self::compute_pending_trust(&manifest, origin, st);
                let overrides_builtin = matches!(origin, PluginOrigin::LocalOverride);
                let has_network = !manifest.permissions.domains.is_empty();
                by_id.insert(
                    manifest.id.clone(),
                    InstalledPlugin {
                        manifest,
                        origin,
                        enabled: st.map(|s| s.enabled).unwrap_or(true),
                        pending_trust,
                        overrides_builtin,
                        has_network,
                        source_repo: st.and_then(|s| s.source_repo.clone()),
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
    /// Refuses to load any plugin that is pending user trust (`PLUGIN_UNTRUSTED`).
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

        if info.pending_trust {
            return Err(PluginError::Untrusted(info.manifest.id));
        }

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

    /// Loads a plugin's WebView HTML entry (`manifest.ui_entry`) safely from disk.
    /// Enforces `enabled == true` and `pending_trust == false`, and rejects path traversal or symlink escapes.
    pub fn load_plugin_ui_html(&self, id: &str) -> Result<LoadedPluginUiHtml> {
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

        if !info.enabled {
            return Err(PluginError::Disabled(info.manifest.id));
        }
        if info.pending_trust {
            return Err(PluginError::Untrusted(info.manifest.id));
        }

        let ui_rel = info.manifest.ui_entry.as_deref().ok_or_else(|| {
            PluginError::InvalidPackage(format!(
                "plugin '{}' does not declare ui_entry",
                info.manifest.id
            ))
        })?;
        let ui_lower = ui_rel.to_ascii_lowercase();
        if !ui_lower.ends_with(".html") && !ui_lower.ends_with(".htm") {
            return Err(PluginError::InvalidPackage(format!(
                "UNSUPPORTED_UI_ENTRY: plugin '{}' uses legacy ui_entry '{}' (expected .html)",
                info.manifest.id, ui_rel
            )));
        }
        let dir = info.local_dir.as_deref().ok_or_else(|| {
            PluginError::InvalidPackage(format!(
                "plugin '{}' has no local directory for ui_entry",
                info.manifest.id
            ))
        })?;

        let html = read_plugin_file_safe(dir, ui_rel, "ui_entry")?;
        Ok(LoadedPluginUiHtml {
            plugin_id: info.manifest.id.clone(),
            html,
            manifest: info.manifest,
        })
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
        self.record_install_trust_state(&manifest, None)?;

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
        if removed_reg || removed_link {
            self.clear_trust_state(id);
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
            self.clear_trust_state(id);
            return Ok(true);
        }
        Ok(false)
    }

    pub fn kv_store(&self, plugin_id: &str) -> Result<KvStore> {
        KvStore::open(&self.data_dir, plugin_id)
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<()> {
        // Ensure plugin exists (allow toggling even when pending_trust is true).
        let plugins = self.list_plugins()?;
        if !plugins.iter().any(|p| p.manifest.id == id) {
            return Err(PluginError::NotFound(id.to_string()));
        }
        let mut state = self.load_state()?;
        let entry = state.plugins.entry(id.to_string()).or_default();
        entry.enabled = enabled;
        self.save_state(&state)
    }

    /// Explicitly marks an installed plugin (and its currently declared sensitive permissions) as trusted.
    pub fn trust_plugin(&self, id: &str) -> Result<()> {
        let plugins = self.list_plugins()?;
        let plugin = plugins
            .into_iter()
            .find(|p| p.manifest.id == id)
            .ok_or_else(|| PluginError::NotFound(id.to_string()))?;
        let mut state = self.load_state()?;
        let entry = state.plugins.entry(id.to_string()).or_default();
        entry.pending_trust = false;
        entry.trusted_permissions = Some(TrustedPermissions::from_manifest(&plugin.manifest));
        self.save_state(&state)
    }

    pub fn record_error(&self, id: &str, err_msg: impl Into<String>) -> Result<()> {
        let mut state = self.load_state()?;
        let entry = state.plugins.entry(id.to_string()).or_default();
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
        // Trust the initial exported copy's built-in permissions; any newly added permissions later will re-trigger trust.
        let _ = self.trust_plugin(builtin.id);
        Ok(dest)
    }

    /// Installs a plugin from a local directory into `<dataDir>/plugins/<id>/`.
    pub fn install_from_dir(&self, src: &Path) -> Result<InstalledPlugin> {
        self.install_from_dir_with_source(src, None)
    }

    fn install_from_dir_with_source(
        &self,
        src: &Path,
        source_repo: Option<&str>,
    ) -> Result<InstalledPlugin> {
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
        self.record_install_trust_state(&manifest, source_repo)?;

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
        self.install_from_zip_bytes_inner(zip_bytes, expected_sha256, None, None)
    }

    fn install_from_zip_bytes_inner(
        &self,
        zip_bytes: &[u8],
        expected_sha256: Option<&str>,
        expected_id: Option<&str>,
        source_repo: Option<&str>,
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
        self.install_from_dir_with_source(&plugin_root, source_repo)
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
        let raw = std::fs::read_to_string(&path)?;
        if raw.trim().is_empty() {
            return Ok(Vec::new());
        }
        let mut file: PluginReposFile = serde_json::from_str(&raw)?;
        let mut migrated = false;
        let current_official = official_repo_url();
        for repo in &mut file.repos {
            if repo.url == LEGACY_OFFICIAL_PLUGIN_REPO_URL {
                repo.url = current_official.clone();
                if repo.name.is_none() {
                    repo.name = Some(OFFICIAL_PLUGIN_REPO_NAME.to_string());
                }
                migrated = true;
            }
        }
        if migrated {
            // Deduplicate URLs in case both legacy and new official URL were present.
            let mut deduped: Vec<PluginRepoConfig> = Vec::with_capacity(file.repos.len());
            for repo in file.repos {
                if !deduped.iter().any(|r| r.url == repo.url) {
                    deduped.push(repo);
                }
            }
            file.repos = deduped;
            let bytes = serde_json::to_vec_pretty(&file)?;
            let _ = write_atomic(&path, &bytes);
        }
        Ok(file.repos)
    }

    pub fn add_repo(&self, url: impl Into<String>, name: Option<String>) -> Result<()> {
        let url = url.into().trim().to_string();
        validate_repo_url(&url)?;
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

    /// Ensures the official plugin repository is subscribed (prepended at index 0 if missing).
    /// Returns `true` if the official repo was restored, `false` if it was already present.
    pub fn restore_official_repo(&self) -> Result<bool> {
        let mut repos = self.list_repos()?;
        let url = official_repo_url();
        if repos.iter().any(|r| r.url == url) {
            return Ok(false);
        }
        repos.insert(
            0,
            PluginRepoConfig {
                url,
                name: Some(OFFICIAL_PLUGIN_REPO_NAME.to_string()),
            },
        );
        let bytes = serde_json::to_vec_pretty(&PluginReposFile { repos })?;
        write_atomic(&self.repos_path(), &bytes)?;
        Ok(true)
    }

    pub fn fetch_repo_catalog(&self, url: &str, http: &dyn HttpClient) -> Result<RepoCatalog> {
        const MAX_REPO_REDIRECTS: usize = 5;
        let mut current_url = url.to_string();
        for hop in 0..=MAX_REPO_REDIRECTS {
            validate_repo_url(&current_url)?;
            let resp = http.send(&HttpRequest {
                url: current_url.clone(),
                method: "GET".to_string(),
                headers: BTreeMap::new(),
                body: None,
                timeout_ms: Some(15_000),
                response_type: crate::http::HttpResponseType::Text,
            })?;
            if matches!(resp.status, 301 | 302 | 303 | 307 | 308) {
                let Some(loc) = resp
                    .headers
                    .get("location")
                    .filter(|s| !s.trim().is_empty())
                else {
                    return Err(PluginError::Http(format!(
                        "HTTP {} redirect without Location fetching repo catalog {current_url}",
                        resp.status
                    )));
                };
                if hop == MAX_REPO_REDIRECTS {
                    return Err(PluginError::Http(format!(
                        "too many redirects fetching repo catalog {url}"
                    )));
                }
                let base = url::Url::parse(&current_url).map_err(|e| {
                    PluginError::Http(format!("invalid repo catalog url '{current_url}': {e}"))
                })?;
                let next = base.join(loc.trim()).map_err(|e| {
                    PluginError::Http(format!("invalid redirect location '{loc}': {e}"))
                })?;
                current_url = next.to_string();
                continue;
            }
            if !(200..300).contains(&resp.status) {
                return Err(PluginError::Http(format!(
                    "HTTP {} fetching repo catalog {current_url}",
                    resp.status
                )));
            }
            let catalog: RepoCatalog = serde_json::from_str(&resp.body)?;
            return Ok(catalog);
        }
        Err(PluginError::Http(format!(
            "too many redirects fetching repo catalog {url}"
        )))
    }

    /// Fetches all subscribed plugin repositories, returning both aggregated plugins and per-repo fetch errors.
    /// Backfills missing/URL-default repo display names from fetched catalog names and persists `plugin_repos.json`.
    pub fn fetch_all_repo_catalogs(&self, http: &dyn HttpClient) -> Result<AggregatedRepoCatalog> {
        let mut repos = self.list_repos()?;
        let mut repos_updated = false;
        let mut plugins = Vec::new();
        let mut errors = Vec::new();
        for repo in &mut repos {
            match self.fetch_repo_catalog(&repo.url, http) {
                Ok(catalog) => {
                    let catalog_name = catalog.name.trim();
                    let repo_name = if !catalog_name.is_empty() {
                        if repo.name.is_none() || repo.name.as_deref() == Some(repo.url.as_str()) {
                            repo.name = Some(catalog_name.to_string());
                            repos_updated = true;
                        }
                        catalog_name.to_string()
                    } else {
                        repo.name.clone().unwrap_or_else(|| repo.url.clone())
                    };
                    for entry in catalog.plugins {
                        plugins.push(AggregatedRepoPlugin {
                            repo_url: repo.url.clone(),
                            repo_name: repo_name.clone(),
                            entry,
                        });
                    }
                }
                Err(err) => {
                    errors.push(RepoFetchError {
                        repo_url: repo.url.clone(),
                        repo_name: repo.name.clone(),
                        error: err.to_string(),
                    });
                }
            }
        }
        if repos_updated {
            let bytes = serde_json::to_vec_pretty(&PluginReposFile { repos })?;
            let _ = write_atomic(&self.repos_path(), &bytes);
        }
        Ok(AggregatedRepoCatalog { plugins, errors })
    }

    pub fn install_from_repo_entry(
        &self,
        entry: &RepoPluginEntry,
        http: &dyn HttpClient,
    ) -> Result<InstalledPlugin> {
        validate_repo_url(&entry.download_url)?;
        let zip_bytes = http.get_bytes(&entry.download_url)?;
        self.install_from_zip_bytes_inner(&zip_bytes, Some(&entry.sha256), Some(&entry.id), None)
    }

    /// Installs a plugin entry from a specific repository URL, recording `source_repo` for cross-repo trust isolation.
    pub fn install_from_repo_entry_from_source(
        &self,
        repo_url: &str,
        entry: &RepoPluginEntry,
        http: &dyn HttpClient,
    ) -> Result<InstalledPlugin> {
        validate_repo_url(repo_url)?;
        validate_repo_url(&entry.download_url)?;
        let zip_bytes = http.get_bytes(&entry.download_url)?;
        self.install_from_zip_bytes_inner(
            &zip_bytes,
            Some(&entry.sha256),
            Some(&entry.id),
            Some(repo_url),
        )
    }

    /// Looks up `plugin_id` across all subscribed repositories and installs the matching entry.
    pub fn install_from_repos(
        &self,
        plugin_id: &str,
        http: &dyn HttpClient,
    ) -> Result<InstalledPlugin> {
        validate_plugin_id(plugin_id)?;
        let aggregated = self.fetch_all_repo_catalogs(http)?;
        if let Some(found) = aggregated.plugins.iter().find(|p| p.entry.id == plugin_id) {
            return self.install_from_repo_entry_from_source(&found.repo_url, &found.entry, http);
        }
        if aggregated.plugins.is_empty() && !aggregated.errors.is_empty() {
            let summary = aggregated
                .errors
                .iter()
                .map(|e| format!("{}: {}", e.repo_url, e.error))
                .collect::<Vec<_>>()
                .join("; ");
            return Err(PluginError::Http(format!(
                "failed to fetch plugin repositories: {summary}"
            )));
        }
        Err(PluginError::NotFound(plugin_id.to_string()))
    }
}

/// Validates that a plugin repository or download URL uses HTTPS,
/// allowing plaintext HTTP only for loopback hosts (`localhost`, `127.0.0.1`, `[::1]`).
pub fn validate_repo_url(raw: &str) -> Result<()> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(PluginError::InvalidPackage(
            "repo URL cannot be empty".into(),
        ));
    }
    let parsed = url::Url::parse(trimmed)
        .map_err(|e| PluginError::InvalidPackage(format!("invalid repo URL '{trimmed}': {e}")))?;
    match parsed.scheme() {
        "https" => Ok(()),
        "http" => {
            let is_loopback = match parsed.host() {
                Some(url::Host::Domain(host)) => host.eq_ignore_ascii_case("localhost"),
                Some(url::Host::Ipv4(ip)) => ip == std::net::Ipv4Addr::LOCALHOST,
                Some(url::Host::Ipv6(ip)) => ip == std::net::Ipv6Addr::LOCALHOST,
                None => false,
            };
            if is_loopback {
                Ok(())
            } else {
                Err(PluginError::HttpNotAllowed(trimmed.to_string()))
            }
        }
        other => Err(PluginError::InvalidPackage(format!(
            "unsupported repo URL scheme '{other}' (expected https://)"
        ))),
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

/// Reads a plugin-relative file after rejecting path traversal and verifying its canonical path stays within `dir`.
fn read_plugin_file_safe(dir: &Path, rel_path: &str, field_name: &str) -> Result<String> {
    if rel_path.trim().is_empty() || rel_path.contains('\\') || rel_path.starts_with('/') {
        return Err(PluginError::ZipSlip(rel_path.to_string()));
    }
    let rel = Path::new(rel_path);
    if rel.is_absolute() {
        return Err(PluginError::ZipSlip(rel_path.to_string()));
    }
    for comp in rel.components() {
        if matches!(
            comp,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        ) {
            return Err(PluginError::ZipSlip(rel_path.to_string()));
        }
    }

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
        return Err(PluginError::SymlinkEscape(format!(
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
            return Err(PluginError::SymlinkEscape(format!(
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

/// Scaffolds a new plain-JS plugin directory with `manifest.toml`, `index.js`, and (for Tool plugins) a zero-build `ui.html`.
pub fn init_plugin_scaffold(dir: &Path, kind: ScaffoldKind) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    let id = derive_scaffold_id(dir);
    let display_name = title_case_id(&id);

    let (manifest, index_js, ui_html) = match kind {
        ScaffoldKind::Oj => (
            format!(
                r#"id = "{id}"
name = "{display_name}"
version = "0.1.0"
description = "Custom OJ data source plugin."
entry = "index.js"
capabilities = ["oj_source"]

[permissions]
domains = ["example.com"]
workspace_read = false
"#
            ),
            r#"function verify(ctx) {
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
            None,
        ),
        ScaffoldKind::Tool => (
            format!(
                r#"id = "{id}"
name = "{display_name}"
version = "0.1.0"
description = "本地离线工具插件"
entry = "index.js"
ui_entry = "ui.html"
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
            r#"function runTool(toolId, inputs) {
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
            Some(format!(
                r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>{display_name}</title>
  <style>
    :root {{
      color-scheme: light dark;
      font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
    }}
    body {{
      margin: 0;
      padding: 16px;
      background: var(--xcpc-bg, transparent);
      color: var(--xcpc-fg, inherit);
    }}
    .card {{
      border: 1px solid var(--xcpc-border, rgba(128, 128, 128, 0.25));
      border-radius: 10px;
      padding: 14px;
      display: flex;
      flex-direction: column;
      gap: 10px;
    }}
    input {{
      padding: 8px 10px;
      border-radius: 8px;
      border: 1px solid var(--xcpc-border, rgba(128, 128, 128, 0.35));
      background: var(--xcpc-card, transparent);
      color: inherit;
      font: inherit;
    }}
    pre {{
      margin: 0;
      padding: 10px;
      border-radius: 8px;
      background: var(--xcpc-card, rgba(128, 128, 128, 0.1));
      white-space: pre-wrap;
    }}
  </style>
</head>
<body>
  <div class="card">
    <strong>{display_name}</strong>
    <input id="text-input" type="text" placeholder="Type something..." />
    <pre id="output">Loading...</pre>
  </div>
  <script>
    (async function () {{
      const input = document.getElementById("text-input");
      const output = document.getElementById("output");
      const xcpc = window.xcpc;

      async function update(text) {{
        if (!xcpc) {{
          output.textContent = "Hello, " + (text || "world") + "!";
          return;
        }}
        try {{
          const res = await xcpc.runTool("hello", {{ text }});
          const first = res && res.blocks && res.blocks[0];
          output.textContent = first && first.content ? first.content : JSON.stringify(res, null, 2);
        }} catch (err) {{
          output.textContent = "Error: " + (err && err.message ? err.message : String(err));
        }}
      }}

      const saved = xcpc ? await xcpc.storage.get("name") : null;
      input.value = typeof saved === "string" ? saved : "XCPC";
      await update(input.value);

      input.addEventListener("input", () => {{
        const next = input.value;
        if (xcpc) xcpc.storage.set("name", next);
        update(next);
      }});
    }})();
  </script>
</body>
</html>
"#
            )),
        ),
    };

    write_atomic(&dir.join("manifest.toml"), manifest.as_bytes())?;
    write_atomic(&dir.join("index.js"), index_js.as_bytes())?;
    if let Some(html) = ui_html {
        write_atomic(&dir.join("ui.html"), html.as_bytes())?;
    }
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

fn title_case_id(id: &str) -> String {
    let words: Vec<String> = id
        .split(['-', '_'])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect();
    if words.is_empty() {
        "My Plugin".to_string()
    } else {
        words.join(" ")
    }
}

/// Scaffolds a Vite + TypeScript WebView plugin project using `@xcpc-workstation/plugin-sdk`.
pub fn init_plugin_scaffold_vite(dir: &Path, kind: ScaffoldKind) -> Result<()> {
    std::fs::create_dir_all(dir.join("src"))?;
    let id = derive_scaffold_id(dir);
    let display_name = title_case_id(&id);

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
    "{OFFICIAL_PLUGIN_SDK_PACKAGE}": "{OFFICIAL_PLUGIN_SDK_VERSION}",
    "typescript": "^5.7.0",
    "vite": "^6.0.0"
  }}
}}
"#
    );

    let vite_config_ts = r#"import { defineConfig } from "vite";
import { xcpcPlugin } from "@xcpc-workstation/plugin-sdk/vite";

export default defineConfig({
  plugins: [xcpcPlugin()],
});
"#;

    let tsconfig_json = r#"{
  "compilerOptions": {
    "target": "ES2022",
    "lib": ["ES2022", "DOM", "DOM.Iterable"],
    "module": "ESNext",
    "moduleResolution": "Bundler",
    "strict": true,
    "noEmit": true,
    "skipLibCheck": true
  },
  "include": ["src", "vite.config.ts"]
}
"#;

    let gitignore = "node_modules/\ndist/\n*.zip\n";

    let (manifest_toml, index_ts, web_files) = match kind {
        ScaffoldKind::Oj => (
            format!(
                r#"# yaml-language-server: $schema=./node_modules/@xcpc-workstation/plugin-sdk/manifest.schema.json
id = "{id}"
name = "{display_name}"
version = "0.1.0"
description = "Custom OJ data source plugin."
entry = "index.js"
capabilities = ["oj_source"]

[permissions]
domains = ["example.com"]
workspace_read = false
"#
            ),
            r#"import type { OjContext, OjSummarySync, OjVerifyResult, ParsedProblemUrl } from "@xcpc-workstation/plugin-sdk/host";

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
                r#"# yaml-language-server: $schema=./node_modules/@xcpc-workstation/plugin-sdk/manifest.schema.json
id = "{id}"
name = "{display_name}"
version = "0.1.0"
description = "本地离线工具插件"
entry = "index.js"
ui_entry = "ui.html"
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
            r#"import type { ToolExecutionResult } from "@xcpc-workstation/plugin-sdk/host";

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
            Some((
                format!(
                    r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>{display_name}</title>
  </head>
  <body>
    <div id="app"></div>
    <script type="module" src="/src/main.ts"></script>
  </body>
</html>
"#
                ),
                format!(
                    r#"import "./style.css";
import {{ getXcpc }} from "@xcpc-workstation/plugin-sdk/ui";

const xcpc = getXcpc();
const root = document.getElementById("app")!;

root.innerHTML = `
  <main class="card">
    <h1>{display_name}</h1>
    <input id="name-input" type="text" placeholder="Type something..." />
    <pre id="result">Loading...</pre>
  </main>
`;

const input = document.getElementById("name-input") as HTMLInputElement;
const result = document.getElementById("result") as HTMLPreElement;

async function run(text: string) {{
  try {{
    const out = await xcpc.runTool("hello", {{ text }});
    const first = out.blocks[0];
    result.textContent =
      first && first.type === "text" ? first.content : JSON.stringify(out, null, 2);
  }} catch (err) {{
    result.textContent = err instanceof Error ? err.message : String(err);
  }}
}}

async function init() {{
  const saved = await xcpc.storage.get<string>("name");
  input.value = saved ?? "XCPC";
  await run(input.value);

  input.addEventListener("input", () => {{
    const next = input.value;
    void xcpc.storage.set("name", next);
    void run(next);
  }});
}}

void init();
"#
                ),
                r#":root {
  color-scheme: light dark;
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
}

body {
  margin: 0;
  padding: 16px;
  background: var(--xcpc-bg, transparent);
  color: var(--xcpc-fg, inherit);
}

.card {
  border: 1px solid var(--xcpc-border, rgba(128, 128, 128, 0.25));
  border-radius: 10px;
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

h1 {
  margin: 0;
  font-size: 14px;
  font-weight: 600;
}

input {
  padding: 8px 10px;
  border-radius: 8px;
  border: 1px solid var(--xcpc-border, rgba(128, 128, 128, 0.35));
  background: var(--xcpc-card, transparent);
  color: inherit;
  font: inherit;
}

pre {
  margin: 0;
  padding: 10px;
  border-radius: 8px;
  background: var(--xcpc-card, rgba(128, 128, 128, 0.1));
  white-space: pre-wrap;
}
"#
                .to_string(),
            )),
        ),
    };

    write_atomic(&dir.join("package.json"), package_json.as_bytes())?;
    write_atomic(&dir.join("vite.config.ts"), vite_config_ts.as_bytes())?;
    write_atomic(&dir.join("tsconfig.json"), tsconfig_json.as_bytes())?;
    write_atomic(&dir.join(".gitignore"), gitignore.as_bytes())?;
    write_atomic(&dir.join("manifest.toml"), manifest_toml.as_bytes())?;
    write_atomic(&dir.join("src/index.ts"), index_ts.as_bytes())?;
    if let Some((index_html, main_ts, style_css)) = web_files {
        write_atomic(&dir.join("index.html"), index_html.as_bytes())?;
        write_atomic(&dir.join("src/main.ts"), main_ts.as_bytes())?;
        write_atomic(&dir.join("src/style.css"), style_css.as_bytes())?;
    }
    Ok(())
}

/// Performs static + runtime dry-run validation of a plugin directory.
pub fn check_plugin_dir(dir: &Path) -> PluginCheckReport {
    let mut issues: Vec<PluginCheckIssue> = Vec::new();
    let push_err = |issues: &mut Vec<PluginCheckIssue>, code: &str, msg: String| {
        let prefix = format!("{code}: ");
        let clean_msg = msg.strip_prefix(&prefix).unwrap_or(&msg).to_string();
        issues.push(PluginCheckIssue {
            severity: "error".to_string(),
            code: code.to_string(),
            message: clean_msg,
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
            push_err(&mut issues, e.code(), e.detail());
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
            push_err(&mut issues, e.code(), e.detail());
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
                    push_err(&mut issues, e.code(), e.detail());
                }
            }
        }
        Err(e) => {
            let mut detail = e.detail();
            if dir.join("package.json").is_file()
                && (dir.join("vite.config.ts").is_file() || dir.join("vite.config.js").is_file())
            {
                detail.push_str(" (hint: Vite source directory detected — run `npm run build` and check `dist/` instead)");
            }
            push_err(&mut issues, e.code(), detail);
        }
    }

    // Check optional WebView HTML ui_entry existence, path safety, and non-empty UTF-8 content.
    if let Some(ref ui_rel) = manifest.ui_entry {
        match read_plugin_file_safe(dir, ui_rel, "ui_entry") {
            Ok(html) => {
                if html.trim().is_empty() {
                    push_err(
                        &mut issues,
                        "EMPTY_UI_HTML",
                        format!("ui_entry '{ui_rel}' must be non-empty UTF-8 HTML"),
                    );
                }
            }
            Err(e) => {
                push_err(&mut issues, e.code(), e.detail());
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
    let canonical_dir = xcpc_storage::canonicalize_clean(dir)?;

    let mut rel_files: Vec<String> = Vec::new();
    let mut push_if_safe = |rel: &str| -> Result<()> {
        let normalized = rel.replace('\\', "/");
        if rel_files.contains(&normalized) {
            return Ok(());
        }
        let candidate = canonical_dir.join(&normalized);
        let canonical = xcpc_storage::canonicalize_clean(&candidate)?;
        if !canonical.starts_with(&canonical_dir) {
            return Err(PluginError::SymlinkEscape(format!(
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
        None => {
            let filename = format!("{}-{}.zip", manifest.id, manifest.version);
            canonical_dir
                .parent()
                .map(|p| p.join(&filename))
                .unwrap_or_else(|| PathBuf::from(&filename))
        }
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
