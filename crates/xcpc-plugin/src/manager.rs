use std::collections::BTreeMap;
use std::io::{Cursor, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::builtin::{BUILTIN_PLUGINS, find_builtin};
use crate::credentials::CredentialStore;
use crate::error::{PluginError, Result};
use crate::http::{HttpClient, HttpRequest};
use crate::kv::{KvStore, write_atomic};
use crate::manifest::{PluginManifest, PluginPermissionSummary};
use crate::runtime::{HostContext, PluginLogEntry, PluginRuntime, SandboxLimits};
use crate::workspace::WorkspaceReader;

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

    /// Lists all discovered plugins (built-ins + local overrides/installs, local wins on ID collision).
    pub fn list_plugins(&self) -> Result<Vec<InstalledPlugin>> {
        let state = self.load_state()?;
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
                },
            );
        }

        let plugins_dir = self.plugins_dir();
        if plugins_dir.is_dir() {
            for entry in std::fs::read_dir(&plugins_dir)? {
                let entry = entry?;
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                if entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with('.'))
                {
                    continue;
                }
                let manifest_path = path.join("manifest.toml");
                if !manifest_path.is_file() {
                    continue;
                }
                let manifest = PluginManifest::from_file(&manifest_path)?;
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
                        local_dir: Some(path),
                    },
                );
            }
        }

        Ok(by_id.into_values().collect())
    }

    /// Loads a plugin's manifest and JS sources by ID.
    pub fn load_plugin(&self, id: &str) -> Result<LoadedPlugin> {
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
            return Ok(Vec::new());
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
