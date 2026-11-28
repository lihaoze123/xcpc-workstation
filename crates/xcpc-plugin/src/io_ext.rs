use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use base64::Engine;
use serde::{Deserialize, Serialize};
use xcpc_domain::{
    ContestMetaPatch, LimitsPatch, Patch, ProblemMetaPatch, ProblemSource, check_id,
};
use xcpc_storage::{CreateProblemOptions, Workspace};

use crate::credentials::MemoryCredentialStore;
use crate::error::{PluginError, Result};
use crate::http::UreqHttpClient;
use crate::kv::{KvStore, write_atomic};
use crate::manager::PluginManager;
use crate::manifest::{ExporterSpec, ImporterSpec, PluginCapability};
use crate::runtime::{HostContext, PluginRuntime, SandboxLimits};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisteredImporter {
    pub plugin_id: String,
    pub plugin_name: String,
    pub importer: ImporterSpec,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisteredExporter {
    pub plugin_id: String,
    pub plugin_name: String,
    pub exporter: ExporterSpec,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportFileInput {
    pub name: String,
    #[serde(default)]
    pub content_base64: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftSample {
    pub input: String,
    pub output: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProblemDraft {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub title: String,
    pub statement_md: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editorial_md: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_limit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_limit: Option<String>,
    #[serde(default)]
    pub samples: Vec<DraftSample>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<ProblemSource>,
    /// Map of relative path under `assets/` -> base64-encoded file bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assets: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContestDraft {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub problems: Vec<ProblemDraft>,
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ImportDraft {
    Problem(ProblemDraft),
    Contest(ContestDraft),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportAppliedReport {
    pub kind: String,
    #[serde(default)]
    pub created_ids: Vec<String>,
    #[serde(default)]
    pub skipped_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExportContext {
    /// `"problem"` or `"contest"`.
    pub kind: String,
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub metadata: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub statement_md: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editorial_md: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub samples: Option<Vec<DraftSample>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdf_base64: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problems: Option<Vec<ExportContext>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportFileEntry {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_base64: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportOutput {
    #[serde(default)]
    pub files: Vec<ExportFileEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

impl PluginManager {
    /// Lists all importers declared by enabled plugins with the `importer` capability.
    pub fn list_importers(&self) -> Vec<RegisteredImporter> {
        let Ok(plugins) = self.list_plugins() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for p in plugins {
            if !p.enabled
                || p.pending_trust
                || !p.manifest.has_capability(PluginCapability::Importer)
            {
                continue;
            }
            for importer in p.manifest.importers {
                out.push(RegisteredImporter {
                    plugin_id: p.manifest.id.clone(),
                    plugin_name: p.manifest.name.clone(),
                    importer,
                });
            }
        }
        out
    }

    /// Lists all exporters declared by enabled plugins with the `exporter` capability.
    pub fn list_exporters(&self) -> Vec<RegisteredExporter> {
        let Ok(plugins) = self.list_plugins() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for p in plugins {
            if !p.enabled
                || p.pending_trust
                || !p.manifest.has_capability(PluginCapability::Exporter)
            {
                continue;
            }
            for exporter in p.manifest.exporters {
                out.push(RegisteredExporter {
                    plugin_id: p.manifest.id.clone(),
                    plugin_name: p.manifest.name.clone(),
                    exporter,
                });
            }
        }
        out
    }

    /// Runs a plugin importer's `importPackage(files, importerId)` in the QuickJS sandbox.
    pub fn run_importer(
        &self,
        plugin_id: &str,
        importer_id: &str,
        files: &[ImportFileInput],
    ) -> Result<ImportDraft> {
        let loaded = self.load_plugin(plugin_id)?;
        if !loaded.info.enabled {
            return Err(PluginError::Disabled(plugin_id.to_string()));
        }
        if !loaded
            .info
            .manifest
            .has_capability(PluginCapability::Importer)
        {
            return Err(PluginError::PermissionDenied {
                plugin_id: plugin_id.to_string(),
                permission: "importer".to_string(),
            });
        }

        let kv = self
            .kv_store(plugin_id)
            .unwrap_or_else(|_| KvStore::in_memory());
        let host_ctx = HostContext {
            manifest: loaded.info.manifest.clone(),
            http_client: Arc::new(UreqHttpClient::new()),
            credentials: Arc::new(MemoryCredentialStore::new()),
            kv: Arc::new(kv),
            workspace: None,
            workspace_writer: None,
            limits: SandboxLimits::default(),
        };

        let args = [serde_json::to_value(files)?, serde_json::json!(importer_id)];
        match PluginRuntime::call_function(&loaded.entry_source, "importPackage", &args, &host_ctx)
        {
            Ok(call) => {
                let draft: ImportDraft = serde_json::from_value(call.value)?;
                let _ = self.clear_error(plugin_id);
                Ok(draft)
            }
            Err(err) => {
                let _ = self.record_error(plugin_id, err.to_string());
                Err(err)
            }
        }
    }

    /// Runs a plugin exporter's `exportPackage(ctx, exporterId)` in the QuickJS sandbox.
    pub fn run_exporter(
        &self,
        plugin_id: &str,
        exporter_id: &str,
        ctx: &ExportContext,
    ) -> Result<ExportOutput> {
        let loaded = self.load_plugin(plugin_id)?;
        if !loaded.info.enabled {
            return Err(PluginError::Disabled(plugin_id.to_string()));
        }
        if !loaded
            .info
            .manifest
            .has_capability(PluginCapability::Exporter)
        {
            return Err(PluginError::PermissionDenied {
                plugin_id: plugin_id.to_string(),
                permission: "exporter".to_string(),
            });
        }

        let kv = self
            .kv_store(plugin_id)
            .unwrap_or_else(|_| KvStore::in_memory());
        let host_ctx = HostContext {
            manifest: loaded.info.manifest.clone(),
            http_client: Arc::new(UreqHttpClient::new()),
            credentials: Arc::new(MemoryCredentialStore::new()),
            kv: Arc::new(kv),
            workspace: None,
            workspace_writer: None,
            limits: SandboxLimits::default(),
        };

        let args = [serde_json::to_value(ctx)?, serde_json::json!(exporter_id)];
        match PluginRuntime::call_function(&loaded.entry_source, "exportPackage", &args, &host_ctx)
        {
            Ok(call) => {
                let output: ExportOutput = serde_json::from_value(call.value)?;
                let _ = self.clear_error(plugin_id);
                Ok(output)
            }
            Err(err) => {
                let _ = self.record_error(plugin_id, err.to_string());
                Err(err)
            }
        }
    }
}

/// Applies an `ImportDraft` returned by an importer plugin into `workspace`, enforcing:
/// - Read-only workspace rejection;
/// - Slug validation & deduplication;
/// - Strict asset relative path traversal checks (rejecting `..`, absolute paths, backslashes, illegal chars);
/// - Atomic writes through `Workspace`.
pub fn apply_import_draft(
    workspace: &Workspace,
    draft: &ImportDraft,
) -> Result<ImportAppliedReport> {
    if workspace.is_read_only() {
        return Err(PluginError::PermissionDenied {
            plugin_id: "importer".to_string(),
            permission: "workspace_write (workspace is read-only)".to_string(),
        });
    }

    match draft {
        ImportDraft::Problem(problem) => {
            validate_problem_draft(problem)?;
            let mut created_ids = Vec::new();
            let mut skipped_ids = Vec::new();
            let (id, created) = apply_single_problem(workspace, problem)?;
            if created {
                created_ids.push(id);
            } else {
                skipped_ids.push(id);
            }
            Ok(ImportAppliedReport {
                kind: "problem".to_string(),
                created_ids,
                skipped_ids,
            })
        }
        ImportDraft::Contest(contest) => {
            if let Some(ref cid) = contest.id {
                validate_explicit_slug(cid)?;
            }
            for problem in &contest.problems {
                validate_problem_draft(problem)?;
            }

            let mut created_ids = Vec::new();
            let mut skipped_ids = Vec::new();
            let mut problem_ids = Vec::with_capacity(contest.problems.len());

            for problem in &contest.problems {
                let (pid, created) = apply_single_problem(workspace, problem)?;
                if created {
                    created_ids.push(pid.clone());
                } else {
                    skipped_ids.push(pid.clone());
                }
                if !problem_ids.contains(&pid) {
                    problem_ids.push(pid);
                }
            }

            let base_slug = contest
                .id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| slugify(&contest.title, "imported-contest"));
            let contest_id = allocate_unique_contest_slug(workspace, &base_slug)?;

            let title = if contest.title.trim().is_empty() {
                contest_id.clone()
            } else {
                contest.title.trim().to_string()
            };
            let patch = ContestMetaPatch {
                name: Some(title),
                location: contest
                    .subtitle
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|s| Patch::Set(s.to_string()))
                    .unwrap_or(Patch::Keep),
                ..Default::default()
            };
            workspace
                .compose_contest(&contest_id, &problem_ids, Some(&patch))
                .map_err(map_storage_err)?;
            created_ids.push(contest_id);

            Ok(ImportAppliedReport {
                kind: "contest".to_string(),
                created_ids,
                skipped_ids,
            })
        }
    }
}

/// Safely writes an `ExportOutput` produced by an exporter plugin to `out_dir`, enforcing:
/// - Every file path is a safe relative path (rejects `..`, absolute paths, backslashes, null bytes);
/// - Read-only workspace protection when `out_dir` is inside `workspace.root()`;
/// - Symlink escape checks and atomic file writes.
pub fn write_export_output(
    workspace: &Workspace,
    output: &ExportOutput,
    out_dir: &Path,
) -> Result<Vec<PathBuf>> {
    // 1. Validate all output paths before touching the filesystem.
    for entry in &output.files {
        validate_safe_rel_path(&entry.path)?;
    }

    // 2. Enforce read-only guard if out_dir targets inside the workspace.
    if workspace.is_read_only() && path_is_within_or_resolves_into(out_dir, workspace.root()) {
        return Err(PluginError::PermissionDenied {
            plugin_id: "exporter".to_string(),
            permission: "workspace_write (workspace is read-only)".to_string(),
        });
    }

    // 3. Create and canonicalize out_dir.
    std::fs::create_dir_all(out_dir)?;
    let canonical_out = std::fs::canonicalize(out_dir)?;
    if workspace.is_read_only() && canonical_out.starts_with(workspace.root()) {
        return Err(PluginError::PermissionDenied {
            plugin_id: "exporter".to_string(),
            permission: "workspace_write (workspace is read-only)".to_string(),
        });
    }

    // 4. Write all files atomically after verifying each resolved destination stays under canonical_out.
    let mut written = Vec::with_capacity(output.files.len());
    for entry in &output.files {
        let target = canonical_out.join(&entry.path);
        if !target.starts_with(&canonical_out) {
            return Err(PluginError::ZipSlip(entry.path.clone()));
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
            let canonical_parent = std::fs::canonicalize(parent)?;
            if !canonical_parent.starts_with(&canonical_out) {
                return Err(PluginError::ZipSlip(entry.path.clone()));
            }
        }
        let bytes = if let Some(ref b64) = entry.content_base64 {
            base64::engine::general_purpose::STANDARD
                .decode(b64.trim())
                .map_err(|e| {
                    PluginError::InvalidPackage(format!(
                        "invalid base64 in export file '{}': {e}",
                        entry.path
                    ))
                })?
        } else if let Some(ref text) = entry.text {
            text.as_bytes().to_vec()
        } else {
            Vec::new()
        };
        write_atomic(&target, &bytes)?;
        written.push(target);
    }

    Ok(written)
}

fn validate_problem_draft(draft: &ProblemDraft) -> Result<()> {
    if let Some(ref id) = draft.id {
        validate_explicit_slug(id)?;
    }
    if let Some(ref assets) = draft.assets {
        for (rel_path, b64) in assets {
            validate_safe_rel_path(rel_path)?;
            base64::engine::general_purpose::STANDARD
                .decode(b64.trim())
                .map_err(|e| {
                    PluginError::InvalidPackage(format!(
                        "invalid base64 for asset '{rel_path}': {e}"
                    ))
                })?;
        }
    }
    Ok(())
}

fn validate_explicit_slug(id: &str) -> Result<()> {
    let trimmed = id.trim();
    if trimmed.contains("..") || trimmed.contains('/') || trimmed.contains('\\') {
        return Err(PluginError::ZipSlip(trimmed.to_string()));
    }
    check_id(trimmed)
        .map_err(|e| PluginError::InvalidPackage(format!("invalid slug '{trimmed}': {e}")))
}

fn validate_safe_rel_path(rel: &str) -> Result<()> {
    if rel.trim().is_empty()
        || rel.contains('\\')
        || rel.starts_with('/')
        || rel.contains('\0')
        || rel.chars().any(|c| c.is_control())
    {
        return Err(PluginError::ZipSlip(rel.to_string()));
    }
    let path = Path::new(rel);
    if path.is_absolute() {
        return Err(PluginError::ZipSlip(rel.to_string()));
    }
    let mut has_normal = false;
    for comp in path.components() {
        match comp {
            Component::Normal(seg) => {
                let s = seg.to_string_lossy();
                if s.is_empty()
                    || s == "."
                    || s == ".."
                    || s.contains(':')
                    || s.contains('*')
                    || s.contains('?')
                    || s.contains('<')
                    || s.contains('>')
                    || s.contains('|')
                    || s.contains('"')
                {
                    return Err(PluginError::ZipSlip(rel.to_string()));
                }
                has_normal = true;
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(PluginError::ZipSlip(rel.to_string()));
            }
        }
    }
    if !has_normal {
        return Err(PluginError::ZipSlip(rel.to_string()));
    }
    Ok(())
}

fn apply_single_problem(workspace: &Workspace, draft: &ProblemDraft) -> Result<(String, bool)> {
    // Source-based deduplication
    if let Some(ref src) = draft.source
        && let Some(existing_id) = workspace
            .find_problem_by_source(&src.platform, &src.id)
            .map_err(map_storage_err)?
    {
        return Ok((existing_id, false));
    }

    let base_slug = draft
        .id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| slugify(&draft.title, "imported-problem"));
    let slug = allocate_unique_problem_slug(workspace, &base_slug)?;

    let title = if draft.title.trim().is_empty() {
        slug.clone()
    } else {
        draft.title.trim().to_string()
    };

    let options = CreateProblemOptions {
        sources: draft.source.iter().cloned().collect(),
        tags: draft.tags.clone().unwrap_or_default(),
        difficulty: None,
        rating: draft.rating,
    };
    workspace
        .create_problem_with_options(&slug, &title, &options)
        .map_err(map_storage_err)?;

    if draft.time_limit.is_some() || draft.memory_limit.is_some() {
        let limits = LimitsPatch {
            time: draft
                .time_limit
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| Patch::Set(s.to_string()))
                .unwrap_or(Patch::Keep),
            memory: draft
                .memory_limit
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| Patch::Set(s.to_string()))
                .unwrap_or(Patch::Keep),
            ..Default::default()
        };
        if !limits.is_empty() {
            let patch = ProblemMetaPatch {
                limits,
                ..Default::default()
            };
            workspace
                .update_problem_meta(&slug, &patch)
                .map_err(map_storage_err)?;
        }
    }

    workspace
        .write(
            &format!("problems/{slug}/statement.md"),
            draft.statement_md.as_bytes(),
        )
        .map_err(map_storage_err)?;

    if let Some(ref editorial) = draft.editorial_md {
        workspace
            .write(
                &format!("problems/{slug}/editorial.md"),
                editorial.as_bytes(),
            )
            .map_err(map_storage_err)?;
    }

    for (idx, sample) in draft.samples.iter().enumerate() {
        let num = format!("{:02}", idx + 1);
        workspace
            .write(
                &format!("problems/{slug}/samples/{num}.in"),
                sample.input.as_bytes(),
            )
            .map_err(map_storage_err)?;
        workspace
            .write(
                &format!("problems/{slug}/samples/{num}.ans"),
                sample.output.as_bytes(),
            )
            .map_err(map_storage_err)?;
    }

    if let Some(ref assets) = draft.assets {
        for (rel_path, b64) in assets {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(b64.trim())
                .map_err(|e| {
                    PluginError::InvalidPackage(format!(
                        "invalid base64 for asset '{rel_path}': {e}"
                    ))
                })?;
            workspace
                .write(&format!("problems/{slug}/assets/{rel_path}"), &bytes)
                .map_err(map_storage_err)?;
        }
    }

    Ok((slug, true))
}

fn allocate_unique_problem_slug(workspace: &Workspace, base: &str) -> Result<String> {
    let existing = workspace.list_problems().map_err(map_storage_err)?;
    if !existing.iter().any(|p| p == base) {
        return Ok(base.to_string());
    }
    for n in 2..=9999u32 {
        let suffix = format!("-{n}");
        let max_prefix = 64usize.saturating_sub(suffix.len());
        let prefix: String = base.chars().take(max_prefix).collect();
        let candidate = format!("{prefix}{suffix}");
        if !existing.iter().any(|p| p == &candidate) {
            return Ok(candidate);
        }
    }
    Err(PluginError::InvalidPackage(format!(
        "unable to allocate unique slug for '{base}'"
    )))
}

fn allocate_unique_contest_slug(workspace: &Workspace, base: &str) -> Result<String> {
    let existing = workspace.list_contests().map_err(map_storage_err)?;
    if !existing.iter().any(|c| c == base) {
        return Ok(base.to_string());
    }
    for n in 2..=9999u32 {
        let suffix = format!("-{n}");
        let max_prefix = 64usize.saturating_sub(suffix.len());
        let prefix: String = base.chars().take(max_prefix).collect();
        let candidate = format!("{prefix}{suffix}");
        if !existing.iter().any(|c| c == &candidate) {
            return Ok(candidate);
        }
    }
    Err(PluginError::InvalidPackage(format!(
        "unable to allocate unique contest slug for '{base}'"
    )))
}

fn slugify(title: &str, fallback: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in title.chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_lowercase() || lower.is_ascii_digit() {
            out.push(lower);
            prev_dash = false;
        } else if !out.is_empty() && !prev_dash {
            out.push('-');
            prev_dash = true;
        }
        if out.len() >= 48 {
            break;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() || check_id(&trimmed).is_err() {
        fallback.to_string()
    } else {
        trimmed
    }
}

fn path_is_within_or_resolves_into(candidate: &Path, root: &Path) -> bool {
    if candidate.starts_with(root) {
        return true;
    }
    // Walk up to the nearest existing ancestor and canonicalize it to detect symlink or relative matches.
    let mut cur = Some(candidate);
    while let Some(p) = cur {
        if let Ok(canon) = std::fs::canonicalize(p) {
            return canon.starts_with(root);
        }
        cur = p.parent();
    }
    false
}

fn map_storage_err(err: xcpc_storage::Error) -> PluginError {
    match err {
        xcpc_storage::Error::PathEscape(p) => PluginError::ZipSlip(p),
        xcpc_storage::Error::ReadOnly => PluginError::PermissionDenied {
            plugin_id: "importer".to_string(),
            permission: "workspace_write (workspace is read-only)".to_string(),
        },
        other => PluginError::InvalidPackage(other.to_string()),
    }
}
