//! Phase 4A desktop bridge helpers: training status derivation & logging, OJ problem URL
//! resolution, plugin management, credentials, decentralized repo catalogs, and OJ sync.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use std::str::FromStr;

use base64::Engine as _;
use xcpc_domain::{
    ProblemSource, SyncedProblemStatus, TrainingLogEntry, TrainingLogStatus, TrainingStatus,
};
use xcpc_plugin::{
    CommandContext, CreateProblemInput, DraftSample, ExportContext, HostContext, ImportFileInput,
    OjContext, OjPluginClient, PluginCapability, PluginManager, PluginOrigin, RenderMathOptions,
    RenderTypstOptions, SandboxLimits, SetSamplesResult, StorageWorkspaceBridge,
    UpdateProblemPatch, UpsertSnippetInput, UreqHttpClient, WorkspaceProblemDetail,
    WorkspaceReader, WorkspaceSample, WorkspaceSnippetDetail, WorkspaceWriter, apply_import_draft,
    render_math_snippet, render_typst_snippet, svg_to_png_base64, write_export_output,
};
use xcpc_storage::{CreateProblemOptions, Workspace};
use xcpc_sync::{
    ContestsQuery, DB_FILENAME, EditorialQuery, IngestProblemRequest, ManualLogRequest,
    SubmissionsQuery, SyncConfig, SyncDb, SyncEngine, SyncOptions, TimeWindow,
    TrainingDashboardQuery, compute_training_dashboard, ingest_problem, log_training_record,
    now_rfc3339_utc, query_contests, query_submissions, query_training_editorials,
};

use crate::state::State;
use crate::types::{
    ActionResult, AggregatedRepoPluginJs, CreateProblemFromUrlResultJs, EditorialListResultJs,
    EditorialSummaryJs, ExportPluginResultJs, IngestProblemRequestJs, IngestProblemResultJs,
    InstallPluginResultJs, JsonReportResultJs, ManualLogResultJs, PlatformSyncReportJs,
    PluginCredentialFieldJs, PluginKvResultJs, PluginRenderPngResultJs, PluginRepoCatalogJs,
    PluginRepoConfigJs, PluginSummaryJs, PluginUiHtmlJs, PluginWatchEventJs,
    PluginWorkspaceResultJs, PluginsOverviewJs, ProblemSourceJs, RepoCatalogResultJs,
    RepoFetchErrorJs, RepoPluginEntryJs, RepoPluginPermissionJs, ResolvedProblemUrlJs,
    SyncReportJs, TrainingSubmissionsQueryJs, VerifyPluginResultJs,
};

fn origin_str(origin: PluginOrigin) -> &'static str {
    match origin {
        PluginOrigin::Builtin => "builtin",
        PluginOrigin::LocalOverride => "local_override",
        PluginOrigin::Local => "local",
    }
}

fn capability_str(cap: PluginCapability) -> &'static str {
    match cap {
        PluginCapability::OjSource => "oj_source",
        PluginCapability::Tool => "tool",
        PluginCapability::Commands => "commands",
        PluginCapability::Importer => "importer",
        PluginCapability::Exporter => "exporter",
        PluginCapability::Credentials => "credentials",
        PluginCapability::Template => "template",
    }
}

/// Batch-derives effective training status strings for indexed problems when
/// `ws.training_enabled()` is true. Returns an empty map when training is disabled.
pub(crate) fn derive_library_training_statuses(
    ws: &Workspace,
    data_dir: Option<&Path>,
    problems: &[(&str, &[ProblemSource])],
) -> BTreeMap<String, String> {
    if !ws.training_enabled() || problems.is_empty() {
        return BTreeMap::new();
    }

    let logs = ws.read_training_logs().unwrap_or_default();
    let synced_map = data_dir
        .filter(|dir| dir.join(DB_FILENAME).is_file())
        .and_then(|dir| SyncDb::open(dir).ok())
        .and_then(|db| db.get_problems_sync_statuses(problems).ok())
        .unwrap_or_default();

    let derived = Workspace::derive_problem_training_statuses(
        &logs,
        problems.iter().map(|(id, sources)| {
            let synced = synced_map
                .get(*id)
                .copied()
                .unwrap_or(SyncedProblemStatus::default());
            (*id, *sources, synced)
        }),
    );

    derived
        .into_iter()
        .map(|(id, st)| (id, st.as_str().to_owned()))
        .collect()
}

impl State {
    pub fn effective_data_dir(&self) -> Result<PathBuf, String> {
        self.data_dir
            .clone()
            .or_else(xcpc_sync::default_data_dir)
            .ok_or_else(|| "data directory is not configured".to_owned())
    }

    fn sync_engine(&self) -> Result<SyncEngine, String> {
        let dir = self.effective_data_dir()?;
        Ok(SyncEngine::new(
            dir,
            Arc::new(UreqHttpClient::new()),
            Arc::clone(&self.credentials),
        ))
    }

    // ---- Training status ---------------------------------------------------------------

    pub fn set_training_status(&self, problem_id: &str, status: &str) -> ActionResult {
        let Some(log_status) = TrainingLogStatus::parse(status) else {
            return ActionResult::err(
                "invalid",
                format!(
                    "invalid training status '{status}' (expected todo|attempted|ac|upsolved|clear)"
                ),
            );
        };
        let ws = {
            let core = self.core.lock().unwrap();
            core.workspace.clone()
        };
        let Some(ws) = ws else {
            return ActionResult::err("no_workspace", "no workspace is open");
        };
        let pid = problem_id.trim();
        if pid.is_empty() {
            return ActionResult::err("invalid_id", "problem ID cannot be empty");
        }
        let source = match ws.load_problem(pid) {
            Ok(problem) => problem.meta.sources.into_iter().next(),
            Err(e) => return ActionResult::err(e.code(), e.to_string()),
        };
        let entry = TrainingLogEntry::for_problem(
            now_rfc3339_utc(),
            Some(pid.to_owned()),
            source,
            log_status,
        );
        match ws.append_training_log(&entry) {
            Ok(()) => {
                self.revision.fetch_add(1, Ordering::AcqRel);
                ActionResult::ok()
            }
            Err(e) => ActionResult::err(e.code(), e.to_string()),
        }
    }

    // ---- Resolve & Create Problem from URL ---------------------------------------------

    pub fn resolve_problem_url(&self, url: &str) -> ResolvedProblemUrlJs {
        let trimmed = url.trim();
        if trimmed.is_empty() {
            return ResolvedProblemUrlJs {
                ok: false,
                error: Some("URL cannot be empty".to_owned()),
                ..Default::default()
            };
        }

        let engine = match self.sync_engine() {
            Ok(e) => e,
            Err(err) => {
                return ResolvedProblemUrlJs {
                    ok: false,
                    error: Some(err),
                    ..Default::default()
                };
            }
        };

        let resolved = match engine.resolve_problem_url(trimmed) {
            Ok(Some(r)) => r,
            Ok(None) => {
                return ResolvedProblemUrlJs {
                    ok: false,
                    error: Some("unsupported problem URL".to_owned()),
                    ..Default::default()
                };
            }
            Err(err) => {
                return ResolvedProblemUrlJs {
                    ok: false,
                    error: Some(err.to_string()),
                    ..Default::default()
                };
            }
        };

        let ws = {
            let core = self.core.lock().unwrap();
            core.workspace.clone()
        };
        let existing_problem_id = ws.as_ref().and_then(|w| {
            w.find_problem_by_source(&resolved.platform, &resolved.problem_key)
                .ok()
                .flatten()
        });

        let rating = resolved
            .normalized_rating
            .or(resolved.raw_rating)
            .and_then(|r| u32::try_from(r).ok());

        ResolvedProblemUrlJs {
            ok: true,
            platform: resolved.platform,
            problem_key: resolved.problem_key,
            suggested_slug: resolved.suggested_slug,
            title: resolved.title,
            tags: resolved.tags,
            rating,
            url: resolved.url,
            existing_problem_id,
            error: None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_problem_from_url(
        &self,
        id: &str,
        title: &str,
        platform: &str,
        source_id: &str,
        url: &str,
        tags: Vec<String>,
        rating: Option<u32>,
    ) -> CreateProblemFromUrlResultJs {
        let ws = {
            let core = self.core.lock().unwrap();
            core.workspace.clone()
        };
        let Some(ws) = ws else {
            return CreateProblemFromUrlResultJs {
                ok: false,
                code: Some("no_workspace".to_owned()),
                error: Some("no workspace is open".to_owned()),
                ..Default::default()
            };
        };

        let plat = platform.trim();
        let src_id = source_id.trim();
        if !plat.is_empty()
            && !src_id.is_empty()
            && let Ok(Some(existing)) = ws.find_problem_by_source(plat, src_id)
        {
            return CreateProblemFromUrlResultJs {
                ok: true,
                problem_id: existing,
                existed: true,
                code: None,
                error: None,
            };
        }

        let mut sources = Vec::new();
        if !plat.is_empty() && !src_id.is_empty() {
            sources.push(ProblemSource {
                platform: plat.to_owned(),
                id: src_id.to_owned(),
                url: (!url.trim().is_empty()).then(|| url.trim().to_owned()),
            });
        }

        let options = CreateProblemOptions {
            sources,
            tags,
            rating,
            difficulty: None,
        };
        let pid = id.trim();
        let clean_title = if title.trim().is_empty() {
            pid
        } else {
            title.trim()
        };

        match ws.create_problem_with_options(pid, clean_title, &options) {
            Ok(_) => {
                let _ = self.core.lock().unwrap().index.refresh_problem(&ws, pid);
                self.invalidate_cache();
                CreateProblemFromUrlResultJs {
                    ok: true,
                    problem_id: pid.to_owned(),
                    existed: false,
                    code: None,
                    error: None,
                }
            }
            Err(e) => CreateProblemFromUrlResultJs {
                ok: false,
                problem_id: String::new(),
                existed: false,
                code: Some(e.code().to_owned()),
                error: Some(e.to_string()),
            },
        }
    }

    // ---- Plugins & Sync Overview -------------------------------------------------------

    pub fn get_plugins_overview(&self) -> PluginsOverviewJs {
        let dir = match self.effective_data_dir() {
            Ok(d) => d,
            Err(err) => {
                return PluginsOverviewJs {
                    error: Some(err),
                    ..Default::default()
                };
            }
        };
        let pm = PluginManager::new(&dir);
        let config = SyncConfig::load(&dir).unwrap_or_default();

        let installed = match pm.list_plugins() {
            Ok(list) => list,
            Err(err) => {
                return PluginsOverviewJs {
                    auto_sync_on_startup: config.auto_sync_on_startup,
                    last_synced_at: config.last_synced_at,
                    error: Some(err.to_string()),
                    ..Default::default()
                };
            }
        };

        let repos = pm
            .list_repos()
            .unwrap_or_default()
            .into_iter()
            .map(|r| PluginRepoConfigJs {
                url: r.url,
                name: r.name,
            })
            .collect();

        let plugins = installed
            .into_iter()
            .map(|p| {
                let m = p.manifest;
                let id = m.id.clone();
                let is_oj = m.capabilities.contains(&PluginCapability::OjSource);
                let handle = if is_oj {
                    config.handle_for(&id).map(str::to_owned)
                } else {
                    None
                };
                let credentials = m
                    .credentials
                    .iter()
                    .map(|spec| {
                        let has_value = self
                            .credentials
                            .get(&id, &spec.key)
                            .ok()
                            .flatten()
                            .is_some_and(|v| !v.is_empty());
                        PluginCredentialFieldJs {
                            name: spec.key.clone(),
                            label: spec.label.clone(),
                            secret: spec.secret,
                            has_value,
                            description: spec.description.clone(),
                            placeholder: spec.placeholder.clone(),
                        }
                    })
                    .collect();

                PluginSummaryJs {
                    id,
                    name: m.name,
                    version: m.version,
                    description: m.description,
                    origin: origin_str(p.origin).to_owned(),
                    enabled: p.enabled,
                    pending_trust: p.pending_trust,
                    overrides_builtin: p.overrides_builtin,
                    has_network: p.has_network,
                    capabilities: m
                        .capabilities
                        .iter()
                        .map(|c| capability_str(*c).to_owned())
                        .collect(),
                    domains: m.permissions.domains,
                    workspace_read: m.permissions.workspace_read,
                    workspace_write: m.permissions.workspace_write,
                    has_ui: m.ui_entry.is_some(),
                    is_linked: p.is_linked,
                    link_target: p.link_target.map(|t| t.to_string_lossy().into_owned()),
                    source_repo: p.source_repo,
                    credentials,
                    handle,
                    last_error: p.last_error,
                }
            })
            .collect();

        PluginsOverviewJs {
            plugins,
            repos,
            auto_sync_on_startup: config.auto_sync_on_startup,
            last_synced_at: config.last_synced_at,
            error: None,
        }
    }

    pub fn set_plugin_enabled(&self, id: &str, enabled: bool) -> ActionResult {
        let dir = match self.effective_data_dir() {
            Ok(d) => d,
            Err(err) => return ActionResult::err("data_dir", err),
        };
        let pm = PluginManager::new(dir);
        match pm.set_enabled(id, enabled) {
            Ok(()) => {
                self.sync_engine_plugin_mounts();
                ActionResult::ok()
            }
            Err(e) => ActionResult::err(e.code(), e.to_string()),
        }
    }

    pub fn trust_plugin(&self, id: &str) -> ActionResult {
        let dir = match self.effective_data_dir() {
            Ok(d) => d,
            Err(err) => return ActionResult::err("data_dir", err),
        };
        let pm = PluginManager::new(dir);
        match pm.trust_plugin(id) {
            Ok(()) => {
                self.sync_engine_plugin_mounts();
                ActionResult::ok()
            }
            Err(e) => ActionResult::err(e.code(), e.to_string()),
        }
    }

    pub fn export_builtin_plugin(&self, id: &str) -> ExportPluginResultJs {
        let dir = match self.effective_data_dir() {
            Ok(d) => d,
            Err(err) => {
                return ExportPluginResultJs {
                    ok: false,
                    path: None,
                    error: Some(err),
                };
            }
        };
        let pm = PluginManager::new(dir);
        match pm.export_builtin_copy(id) {
            Ok(path) => {
                self.refresh_plugin_watcher();
                ExportPluginResultJs {
                    ok: true,
                    path: Some(path.display().to_string()),
                    error: None,
                }
            }
            Err(e) => ExportPluginResultJs {
                ok: false,
                path: None,
                error: Some(e.to_string()),
            },
        }
    }

    pub fn install_plugin_from_path(&self, path: &Path, is_zip: bool) -> InstallPluginResultJs {
        let dir = match self.effective_data_dir() {
            Ok(d) => d,
            Err(err) => {
                return InstallPluginResultJs {
                    ok: false,
                    cancelled: false,
                    plugin_id: None,
                    overview: None,
                    error: Some(err),
                };
            }
        };
        let pm = PluginManager::new(dir);
        let installed = if is_zip {
            pm.install_from_zip(path)
        } else {
            pm.install_from_dir(path)
        };
        match installed {
            Ok(p) => {
                self.refresh_plugin_watcher();
                self.sync_engine_plugin_mounts();
                InstallPluginResultJs {
                    ok: true,
                    cancelled: false,
                    plugin_id: Some(p.manifest.id),
                    overview: Some(self.get_plugins_overview()),
                    error: None,
                }
            }
            Err(e) => InstallPluginResultJs {
                ok: false,
                cancelled: false,
                plugin_id: None,
                overview: None,
                error: Some(e.to_string()),
            },
        }
    }

    pub fn add_plugin_repo(&self, url: &str) -> ActionResult {
        let dir = match self.effective_data_dir() {
            Ok(d) => d,
            Err(err) => return ActionResult::err("data_dir", err),
        };
        let pm = PluginManager::new(dir);
        match pm.add_repo(url, None) {
            Ok(()) => ActionResult::ok(),
            Err(e) => ActionResult::err(e.code(), e.to_string()),
        }
    }

    pub fn remove_plugin_repo(&self, url: &str) -> ActionResult {
        let dir = match self.effective_data_dir() {
            Ok(d) => d,
            Err(err) => return ActionResult::err("data_dir", err),
        };
        let pm = PluginManager::new(dir);
        match pm.remove_repo(url) {
            Ok(_) => ActionResult::ok(),
            Err(e) => ActionResult::err(e.code(), e.to_string()),
        }
    }

    pub fn restore_official_plugin_repo(&self) -> ActionResult {
        let dir = match self.effective_data_dir() {
            Ok(d) => d,
            Err(err) => return ActionResult::err("data_dir", err),
        };
        let pm = PluginManager::new(dir);
        match pm.restore_official_repo() {
            Ok(_) => ActionResult::ok(),
            Err(e) => ActionResult::err(e.code(), e.to_string()),
        }
    }

    pub fn fetch_plugin_repo_catalog(&self, url: &str) -> RepoCatalogResultJs {
        let dir = match self.effective_data_dir() {
            Ok(d) => d,
            Err(err) => {
                return RepoCatalogResultJs {
                    ok: false,
                    error: Some(err),
                    ..Default::default()
                };
            }
        };
        let pm = PluginManager::new(dir);
        let http = UreqHttpClient::new();
        match pm.fetch_repo_catalog(url.trim(), &http) {
            Ok(catalog) => {
                // Remember catalog display name if we have this repo subscribed.
                if !catalog.name.trim().is_empty() {
                    let _ = pm.add_repo(url.trim(), Some(catalog.name.clone()));
                }
                let plugins = catalog
                    .plugins
                    .into_iter()
                    .map(|p| RepoPluginEntryJs {
                        id: p.id,
                        name: p.name,
                        version: p.version,
                        description: p.description,
                        download_url: p.download_url,
                        sha256: p.sha256,
                        capabilities: p
                            .capabilities
                            .iter()
                            .map(|c| capability_str(*c).to_owned())
                            .collect(),
                        has_ui: p.has_ui,
                        official: p.official,
                        permissions: p.permissions.map(|perm| RepoPluginPermissionJs {
                            domains: perm.domains,
                            workspace_read: perm.workspace_read,
                            workspace_write: perm.workspace_write,
                        }),
                    })
                    .collect();
                RepoCatalogResultJs {
                    ok: true,
                    name: catalog.name,
                    plugins,
                    error: None,
                }
            }
            Err(e) => RepoCatalogResultJs {
                ok: false,
                error: Some(e.to_string()),
                ..Default::default()
            },
        }
    }

    pub fn fetch_all_plugin_repo_catalogs(&self) -> PluginRepoCatalogJs {
        let dir = match self.effective_data_dir() {
            Ok(d) => d,
            Err(err) => {
                return PluginRepoCatalogJs {
                    ok: false,
                    error: Some(err),
                    ..Default::default()
                };
            }
        };
        let pm = PluginManager::new(dir);
        let http = UreqHttpClient::new();
        match pm.fetch_all_repo_catalogs(&http) {
            Ok(agg) => {
                let plugins = agg
                    .plugins
                    .into_iter()
                    .map(|item| {
                        let p = item.entry;
                        AggregatedRepoPluginJs {
                            repo_url: item.repo_url,
                            repo_name: item.repo_name,
                            entry: RepoPluginEntryJs {
                                id: p.id,
                                name: p.name,
                                version: p.version,
                                description: p.description,
                                download_url: p.download_url,
                                sha256: p.sha256,
                                capabilities: p
                                    .capabilities
                                    .iter()
                                    .map(|c| capability_str(*c).to_owned())
                                    .collect(),
                                has_ui: p.has_ui,
                                official: p.official,
                                permissions: p.permissions.map(|perm| RepoPluginPermissionJs {
                                    domains: perm.domains,
                                    workspace_read: perm.workspace_read,
                                    workspace_write: perm.workspace_write,
                                }),
                            },
                        }
                    })
                    .collect();
                let errors = agg
                    .errors
                    .into_iter()
                    .map(|e| RepoFetchErrorJs {
                        repo_url: e.repo_url,
                        repo_name: e.repo_name,
                        error: e.error,
                    })
                    .collect();
                PluginRepoCatalogJs {
                    ok: true,
                    plugins,
                    errors,
                    error: None,
                }
            }
            Err(e) => PluginRepoCatalogJs {
                ok: false,
                error: Some(e.to_string()),
                ..Default::default()
            },
        }
    }

    pub fn install_plugin_from_repo(
        &self,
        repo_url: &str,
        plugin_id: &str,
    ) -> InstallPluginResultJs {
        let dir = match self.effective_data_dir() {
            Ok(d) => d,
            Err(err) => {
                return InstallPluginResultJs {
                    ok: false,
                    error: Some(err),
                    ..Default::default()
                };
            }
        };
        let pm = PluginManager::new(dir);
        let http = UreqHttpClient::new();
        let catalog = match pm.fetch_repo_catalog(repo_url.trim(), &http) {
            Ok(c) => c,
            Err(e) => {
                return InstallPluginResultJs {
                    ok: false,
                    error: Some(e.to_string()),
                    ..Default::default()
                };
            }
        };
        let Some(entry) = catalog.plugins.iter().find(|p| p.id == plugin_id) else {
            return InstallPluginResultJs {
                ok: false,
                error: Some(format!("plugin '{plugin_id}' not found in repository")),
                ..Default::default()
            };
        };
        match pm.install_from_repo_entry_from_source(repo_url.trim(), entry, &http) {
            Ok(installed) => {
                self.refresh_plugin_watcher();
                self.sync_engine_plugin_mounts();
                InstallPluginResultJs {
                    ok: true,
                    cancelled: false,
                    plugin_id: Some(installed.manifest.id),
                    overview: Some(self.get_plugins_overview()),
                    error: None,
                }
            }
            Err(e) => InstallPluginResultJs {
                ok: false,
                cancelled: false,
                plugin_id: None,
                overview: None,
                error: Some(e.to_string()),
            },
        }
    }

    pub fn set_sync_handle(&self, platform: &str, handle: &str) -> ActionResult {
        let engine = match self.sync_engine() {
            Ok(e) => e,
            Err(err) => return ActionResult::err("data_dir", err),
        };
        match engine.set_handle(platform, handle) {
            Ok(()) => {
                self.revision.fetch_add(1, Ordering::AcqRel);
                ActionResult::ok()
            }
            Err(e) => ActionResult::err("sync_error", e.to_string()),
        }
    }

    pub fn set_auto_sync_on_startup(&self, enabled: bool) -> ActionResult {
        let dir = match self.effective_data_dir() {
            Ok(d) => d,
            Err(err) => return ActionResult::err("data_dir", err),
        };
        let mut config = match SyncConfig::load(&dir) {
            Ok(c) => c,
            Err(e) => return ActionResult::err("sync_error", e.to_string()),
        };
        config.auto_sync_on_startup = enabled;
        match config.save(&dir) {
            Ok(()) => ActionResult::ok(),
            Err(e) => ActionResult::err("sync_error", e.to_string()),
        }
    }

    pub fn set_plugin_credential(&self, plugin_id: &str, key: &str, value: &str) -> ActionResult {
        let pid = plugin_id.trim();
        let k = key.trim();
        if pid.is_empty() || k.is_empty() {
            return ActionResult::err("invalid", "plugin ID and credential key are required");
        }
        let res = if value.is_empty() {
            self.credentials.delete(pid, k)
        } else {
            self.credentials.set(pid, k, value)
        };
        match res {
            Ok(()) => ActionResult::ok(),
            Err(e) => ActionResult::err("credential_error", e.to_string()),
        }
    }

    pub fn verify_plugin(
        &self,
        plugin_id: &str,
        handle_override: Option<&str>,
    ) -> VerifyPluginResultJs {
        let dir = match self.effective_data_dir() {
            Ok(d) => d,
            Err(err) => {
                return VerifyPluginResultJs {
                    ok: false,
                    message: None,
                    error: Some(err),
                };
            }
        };
        let pm = PluginManager::new(&dir);
        let loaded = match pm.load_plugin(plugin_id) {
            Ok(p) => p,
            Err(e) => {
                return VerifyPluginResultJs {
                    ok: false,
                    message: None,
                    error: Some(e.to_string()),
                };
            }
        };
        let kv = match pm.kv_store(plugin_id) {
            Ok(kv) => Arc::new(kv),
            Err(e) => {
                return VerifyPluginResultJs {
                    ok: false,
                    message: None,
                    error: Some(e.to_string()),
                };
            }
        };
        let handle = match handle_override {
            Some(h) => h.trim().to_owned(),
            None => {
                let config = SyncConfig::load(&dir).unwrap_or_default();
                config.handle_for(plugin_id).unwrap_or("").to_owned()
            }
        };
        let host_ctx = HostContext {
            manifest: loaded.info.manifest.clone(),
            http_client: Arc::new(UreqHttpClient::new()),
            credentials: Arc::clone(&self.credentials),
            kv,
            workspace: None,
            workspace_writer: None,
            limits: SandboxLimits::default(),
        };
        let client = OjPluginClient::new(&loaded.entry_source, &host_ctx);
        match client.supports("verify") {
            Ok(true) => match client.verify(&OjContext { handle }) {
                Ok(res) => {
                    if res.ok {
                        let _ = pm.clear_error(plugin_id);
                    } else if handle_override.is_none()
                        && let Some(ref msg) = res.message
                    {
                        let _ = pm.record_error(plugin_id, msg);
                    }
                    VerifyPluginResultJs {
                        ok: res.ok,
                        message: res.message.clone(),
                        error: (!res.ok).then(|| {
                            res.message
                                .unwrap_or_else(|| "verification failed".to_owned())
                        }),
                    }
                }
                Err(e) => {
                    let msg = e.to_string();
                    if handle_override.is_none() {
                        let _ = pm.record_error(plugin_id, &msg);
                    }
                    VerifyPluginResultJs {
                        ok: false,
                        message: None,
                        error: Some(msg),
                    }
                }
            },
            Ok(false) => VerifyPluginResultJs {
                ok: true,
                message: Some("plugin does not implement verify()".to_owned()),
                error: None,
            },
            Err(e) => VerifyPluginResultJs {
                ok: false,
                message: None,
                error: Some(e.to_string()),
            },
        }
    }

    pub fn sync_oj(&self, platform: Option<String>, full: bool) -> SyncReportJs {
        let engine = match self.sync_engine() {
            Ok(e) => e,
            Err(err) => {
                return SyncReportJs {
                    ok: false,
                    error: Some(err),
                    ..Default::default()
                };
            }
        };
        let options = SyncOptions {
            platform: platform
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty()),
            full,
        };
        match engine.sync(&options) {
            Ok(report) => {
                self.revision.fetch_add(1, Ordering::AcqRel);
                let all_ok = report.platforms.iter().all(|p| p.ok);
                let platforms = report
                    .platforms
                    .into_iter()
                    .map(|p| PlatformSyncReportJs {
                        platform: p.platform,
                        handle: p.handle,
                        ok: p.ok,
                        account_synced: p.account_synced,
                        summary_synced: p.summary_synced,
                        submissions_synced: p.submissions_synced as u32,
                        rating_history_synced: p.rating_history_synced as u32,
                        contests_synced: p.contests_synced as u32,
                        error: p.error,
                    })
                    .collect();
                SyncReportJs {
                    ok: all_ok,
                    synced_at: report.synced_at,
                    platforms,
                    error: None,
                }
            }
            Err(e) => SyncReportJs {
                ok: false,
                error: Some(e.to_string()),
                ..Default::default()
            },
        }
    }

    /// Runs a silent startup incremental sync once per session if `auto_sync_on_startup` is
    /// enabled and at least one enabled `oj_source` plugin has a configured handle.
    /// Returns `true` if a sync was actually executed (so the UI can silently refresh).
    pub fn startup_sync_if_enabled(&self) -> bool {
        if self.startup_sync_triggered.swap(true, Ordering::AcqRel) {
            return false;
        }
        let Some(ref dir) = self.data_dir else {
            return false;
        };
        let Ok(config) = SyncConfig::load(dir) else {
            return false;
        };
        if !config.auto_sync_on_startup {
            return false;
        }
        let pm = PluginManager::new(dir);
        let Ok(plugins) = pm.list_plugins() else {
            return false;
        };
        let has_configured = plugins.iter().any(|p| {
            p.enabled
                && p.manifest
                    .capabilities
                    .contains(&PluginCapability::OjSource)
                && config.handle_for(&p.manifest.id).is_some()
        });
        if !has_configured {
            return false;
        }
        let report = self.sync_oj(None, false);
        !report.platforms.is_empty()
    }

    // ---- Phase 4B: Training Dashboard, Contests Calendar, Submissions & Ingest ---------

    pub fn training_dashboard(
        &self,
        window: Option<String>,
        heatmap_year: Option<i32>,
        tag_min_rating: Option<i32>,
    ) -> JsonReportResultJs {
        let dir = match self.effective_data_dir() {
            Ok(d) => d,
            Err(err) => return JsonReportResultJs::err("data_dir", err),
        };
        let parsed_window = match window.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(raw) => match TimeWindow::from_str(raw) {
                Ok(w) => w,
                Err(err) => return JsonReportResultJs::err("invalid", err),
            },
            None => TimeWindow::All,
        };
        let db = match SyncDb::open(&dir) {
            Ok(db) => db,
            Err(err) => return JsonReportResultJs::err("sync_error", err.to_string()),
        };
        let config = SyncConfig::load(&dir).unwrap_or_default();
        let ws = {
            let core = self.core.lock().unwrap();
            core.workspace.clone()
        };
        let query = TrainingDashboardQuery {
            window: parsed_window,
            heatmap_year,
            tag_min_rating: tag_min_rating.map(i64::from),
            now_unix: None,
        };
        match compute_training_dashboard(&db, &config, ws.as_ref(), &query) {
            Ok(dashboard) => match serde_json::to_string(&dashboard) {
                Ok(json) => JsonReportResultJs {
                    ok: true,
                    report_json: Some(json),
                    markdown: None,
                    error: None,
                    code: None,
                },
                Err(err) => JsonReportResultJs::err("serialize", err.to_string()),
            },
            Err(err) => JsonReportResultJs::err("sync_error", err.to_string()),
        }
    }

    pub fn training_contests(
        &self,
        platform: Option<String>,
        include_finished: bool,
    ) -> JsonReportResultJs {
        let dir = match self.effective_data_dir() {
            Ok(d) => d,
            Err(err) => return JsonReportResultJs::err("data_dir", err),
        };
        let db = match SyncDb::open(&dir) {
            Ok(db) => db,
            Err(err) => return JsonReportResultJs::err("sync_error", err.to_string()),
        };
        let query = ContestsQuery {
            platform: platform
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty()),
            include_finished,
            now_unix: None,
        };
        match query_contests(&db, &query) {
            Ok(contests) => match serde_json::to_string(&contests) {
                Ok(json) => JsonReportResultJs {
                    ok: true,
                    report_json: Some(json),
                    markdown: None,
                    error: None,
                    code: None,
                },
                Err(err) => JsonReportResultJs::err("serialize", err.to_string()),
            },
            Err(err) => JsonReportResultJs::err("sync_error", err.to_string()),
        }
    }

    pub fn training_submissions(&self, query: TrainingSubmissionsQueryJs) -> JsonReportResultJs {
        let dir = match self.effective_data_dir() {
            Ok(d) => d,
            Err(err) => return JsonReportResultJs::err("data_dir", err),
        };
        let db = match SyncDb::open(&dir) {
            Ok(db) => db,
            Err(err) => return JsonReportResultJs::err("sync_error", err.to_string()),
        };
        let ws = {
            let core = self.core.lock().unwrap();
            core.workspace.clone()
        };
        let core_query = SubmissionsQuery {
            by_problem: query.by_problem,
            platform: query.platform,
            verdict: query.verdict,
            unsolved_only: query.unsolved_only,
            min_rating: query.min_rating.map(i64::from),
            max_rating: query.max_rating.map(i64::from),
            tag: query.tag,
            search: query.search,
            limit: query.limit.map(|v| v as usize),
            offset: query.offset.map(|v| v as usize),
        };
        match query_submissions(&db, ws.as_ref(), &core_query) {
            Ok(page) => match serde_json::to_string(&page) {
                Ok(json) => JsonReportResultJs {
                    ok: true,
                    report_json: Some(json),
                    markdown: None,
                    error: None,
                    code: None,
                },
                Err(err) => JsonReportResultJs::err("serialize", err.to_string()),
            },
            Err(err) => JsonReportResultJs::err("sync_error", err.to_string()),
        }
    }

    pub fn training_ingest_problem(&self, req: IngestProblemRequestJs) -> IngestProblemResultJs {
        let dir = match self.effective_data_dir() {
            Ok(d) => d,
            Err(err) => {
                return IngestProblemResultJs {
                    ok: false,
                    code: Some("data_dir".to_owned()),
                    error: Some(err),
                    ..Default::default()
                };
            }
        };
        let ws = {
            let core = self.core.lock().unwrap();
            core.workspace.clone()
        };
        let Some(ws) = ws else {
            return IngestProblemResultJs {
                ok: false,
                code: Some("no_workspace".to_owned()),
                error: Some("no workspace is open".to_owned()),
                ..Default::default()
            };
        };
        let initial_status = match req
            .initial_status
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(raw) => match TrainingLogStatus::parse(raw) {
                Some(st) => Some(st),
                None => {
                    return IngestProblemResultJs {
                        ok: false,
                        code: Some("invalid".to_owned()),
                        error: Some(format!(
                            "invalid training status '{raw}' (expected todo|attempted|ac|upsolved|clear)"
                        )),
                        ..Default::default()
                    };
                }
            },
            None => None,
        };
        let core_req = IngestProblemRequest {
            url: req.url,
            platform: req.platform,
            problem_key: req.problem_key,
            id: req.id,
            title: req.title,
            tags: req.tags,
            rating: req.rating.map(i64::from),
            initial_status,
        };
        let pm = PluginManager::new(&dir);
        match ingest_problem(&ws, &dir, Some(&pm), &core_req) {
            Ok(res) => {
                if res.created {
                    let _ = self
                        .core
                        .lock()
                        .unwrap()
                        .index
                        .refresh_problem(&ws, &res.problem_id);
                    self.invalidate_cache();
                } else if initial_status.is_some() {
                    self.revision.fetch_add(1, Ordering::AcqRel);
                }
                IngestProblemResultJs {
                    ok: true,
                    problem_id: res.problem_id,
                    created: res.created,
                    title: res.title,
                    code: None,
                    error: None,
                }
            }
            Err(err) => {
                let code = match &err {
                    xcpc_sync::SyncError::Storage(se) => se.code().to_owned(),
                    xcpc_sync::SyncError::InvalidInput(_) => "invalid".to_owned(),
                    _ => "sync_error".to_owned(),
                };
                IngestProblemResultJs {
                    ok: false,
                    code: Some(code),
                    error: Some(err.to_string()),
                    ..Default::default()
                }
            }
        }
    }

    pub fn training_log_record(&self, req_json: &str) -> ManualLogResultJs {
        let dir = match self.effective_data_dir() {
            Ok(d) => d,
            Err(err) => {
                return ManualLogResultJs {
                    ok: false,
                    error: Some(err),
                    ..Default::default()
                };
            }
        };
        let ws = {
            let core = self.core.lock().unwrap();
            core.workspace.clone()
        };
        let Some(ws) = ws else {
            return ManualLogResultJs {
                ok: false,
                error: Some("no workspace is open".to_owned()),
                ..Default::default()
            };
        };
        let req: ManualLogRequest = match serde_json::from_str(req_json) {
            Ok(r) => r,
            Err(err) => {
                return ManualLogResultJs {
                    ok: false,
                    error: Some(format!("invalid manual log request: {err}")),
                    ..Default::default()
                };
            }
        };
        let pm = PluginManager::new(&dir);
        match log_training_record(&ws, &dir, Some(&pm), &req) {
            Ok(res) => {
                if res.created_in_library
                    && let Some(ref pid) = res.library_problem_id
                {
                    let _ = self.core.lock().unwrap().index.refresh_problem(&ws, pid);
                    self.invalidate_cache();
                } else {
                    self.revision.fetch_add(1, Ordering::AcqRel);
                }
                ManualLogResultJs {
                    ok: true,
                    library_problem_id: res.library_problem_id,
                    created_in_library: res.created_in_library,
                    error: None,
                }
            }
            Err(err) => ManualLogResultJs {
                ok: false,
                library_problem_id: None,
                created_in_library: false,
                error: Some(err.to_string()),
            },
        }
    }

    pub fn training_list_editorials(&self, query_json: &str) -> EditorialListResultJs {
        let ws = {
            let core = self.core.lock().unwrap();
            core.workspace.clone()
        };
        let Some(ws) = ws else {
            return EditorialListResultJs {
                error: Some("no workspace is open".to_owned()),
                ..Default::default()
            };
        };
        let query: EditorialQuery = if query_json.trim().is_empty() {
            EditorialQuery::default()
        } else {
            match serde_json::from_str(query_json) {
                Ok(q) => q,
                Err(err) => {
                    return EditorialListResultJs {
                        error: Some(format!("invalid editorial query: {err}")),
                        ..Default::default()
                    };
                }
            }
        };
        match query_training_editorials(&ws, &query) {
            Ok(res) => EditorialListResultJs {
                items: res
                    .items
                    .into_iter()
                    .map(|item| EditorialSummaryJs {
                        problem_id: item.problem_id,
                        title: item.title,
                        rating: item.rating,
                        tags: item.tags,
                        sources: item
                            .sources
                            .into_iter()
                            .map(|s| ProblemSourceJs {
                                platform: s.platform,
                                id: s.id,
                                url: s.url,
                            })
                            .collect(),
                        excerpt: item.excerpt,
                        updated_at: item.updated_at,
                    })
                    .collect(),
                total: res.total as u32,
                available_tags: res.available_tags,
                error: None,
            },
            Err(err) => EditorialListResultJs {
                error: Some(err.to_string()),
                ..Default::default()
            },
        }
    }

    // ---- Phase 4C / Phase 7: Toolbox, UI SDK, Commands, Importers & Exporters ----------

    #[allow(clippy::type_complexity)]
    fn workspace_bridges_if_allowed(
        &self,
        pm: &PluginManager,
        plugin_id: &str,
    ) -> (
        Option<Arc<dyn WorkspaceReader>>,
        Option<Arc<dyn WorkspaceWriter>>,
        Arc<AtomicBool>,
        Option<Workspace>,
    ) {
        let mutated = Arc::new(AtomicBool::new(false));
        let Some(loaded) = pm.load_plugin(plugin_id).ok() else {
            return (None, None, mutated, None);
        };
        let perms = &loaded.info.manifest.permissions;
        if !perms.workspace_read && !perms.workspace_write {
            return (None, None, mutated, None);
        }
        let ws = {
            let core = self.core.lock().unwrap();
            core.workspace.clone()
        };
        let Some(ws) = ws else {
            return (None, None, mutated, None);
        };
        let reader: Option<Arc<dyn WorkspaceReader>> = if perms.workspace_read {
            Some(Arc::new(StorageWorkspaceBridge::new(ws.clone())))
        } else {
            None
        };
        let writer: Option<Arc<dyn WorkspaceWriter>> = if perms.workspace_write {
            Some(Arc::new(TrackingWorkspaceWriter {
                inner: StorageWorkspaceBridge::new(ws.clone()),
                mutated: Arc::clone(&mutated),
            }))
        } else {
            None
        };
        (reader, writer, mutated, Some(ws))
    }

    fn refresh_workspace_after_plugin_write(&self, ws: &Workspace) {
        if let Ok(rebuilt) = xcpc_storage::WorkspaceIndex::scan(ws) {
            let mut core = self.core.lock().unwrap();
            if core
                .workspace
                .as_ref()
                .is_some_and(|cur| cur.root() == ws.root())
            {
                core.index = rebuilt;
            }
        }
        self.invalidate_cache();
    }

    pub fn plugin_list_tools(&self) -> JsonReportResultJs {
        let dir = match self.effective_data_dir() {
            Ok(d) => d,
            Err(err) => return JsonReportResultJs::err("data_dir", err),
        };
        let pm = PluginManager::new(&dir);
        let tools = pm.list_tools();
        match serde_json::to_string(&tools) {
            Ok(json) => JsonReportResultJs::ok(Some(json), None),
            Err(err) => JsonReportResultJs::err("serialize", err.to_string()),
        }
    }

    pub fn plugin_run_tool(
        &self,
        plugin_id: &str,
        tool_id: &str,
        inputs_json: &str,
    ) -> JsonReportResultJs {
        let dir = match self.effective_data_dir() {
            Ok(d) => d,
            Err(err) => return JsonReportResultJs::err("data_dir", err),
        };
        let inputs: serde_json::Value = if inputs_json.trim().is_empty() {
            serde_json::json!({})
        } else {
            match serde_json::from_str(inputs_json) {
                Ok(v) => v,
                Err(err) => {
                    return JsonReportResultJs::err("invalid_json", err.to_string());
                }
            }
        };
        let pm = PluginManager::new(&dir);
        let (ws_reader, ws_writer, mutated, ws_opt) =
            self.workspace_bridges_if_allowed(&pm, plugin_id);
        let res = pm.run_tool_with_writer(plugin_id, tool_id, inputs, ws_reader, ws_writer);
        if mutated.load(Ordering::Acquire)
            && let Some(ref ws) = ws_opt
        {
            self.refresh_workspace_after_plugin_write(ws);
        }
        match res {
            Ok(res) => match serde_json::to_string(&res) {
                Ok(json) => JsonReportResultJs::ok(Some(json), None),
                Err(err) => JsonReportResultJs::err("serialize", err.to_string()),
            },
            Err(err) => JsonReportResultJs::err("plugin_error", err.to_string()),
        }
    }

    pub fn load_plugin_ui_html(&self, plugin_id: &str) -> PluginUiHtmlJs {
        let dir = match self.effective_data_dir() {
            Ok(d) => d,
            Err(err) => {
                return PluginUiHtmlJs {
                    ok: false,
                    error: Some(err),
                    error_code: Some("DATA_DIR".to_owned()),
                    ..Default::default()
                };
            }
        };
        let pm = PluginManager::new(&dir);
        match pm.load_plugin_ui_html(plugin_id) {
            Ok(loaded) => PluginUiHtmlJs {
                ok: true,
                html: Some(loaded.html),
                plugin_id: Some(loaded.plugin_id),
                workspace_read: Some(loaded.manifest.permissions.workspace_read),
                workspace_write: Some(loaded.manifest.permissions.workspace_write),
                error: None,
                error_code: None,
            },
            Err(err) => PluginUiHtmlJs {
                ok: false,
                html: None,
                plugin_id: Some(plugin_id.to_owned()),
                workspace_read: None,
                workspace_write: None,
                error: Some(err.to_string()),
                error_code: Some(err.code().to_owned()),
            },
        }
    }

    pub fn plugin_list_oj_accounts(&self, plugin_id: &str) -> PluginWorkspaceResultJs {
        let err_res = |code: &str, msg: String| PluginWorkspaceResultJs {
            ok: false,
            data_json: None,
            error: Some(msg),
            error_code: Some(code.to_owned()),
        };

        let dir = match self.effective_data_dir() {
            Ok(d) => d,
            Err(err) => return err_res("DATA_DIR", err),
        };
        let pm = PluginManager::new(&dir);
        let plugins = match pm.list_plugins() {
            Ok(list) => list,
            Err(err) => return err_res(err.code(), err.to_string()),
        };
        let Some(plugin) = plugins.into_iter().find(|p| p.manifest.id == plugin_id) else {
            return err_res(
                "PLUGIN_NOT_FOUND",
                format!("PLUGIN_NOT_FOUND: plugin '{plugin_id}' not found"),
            );
        };
        if !plugin.enabled {
            return err_res(
                "PLUGIN_DISABLED",
                format!("PLUGIN_DISABLED: plugin '{plugin_id}' is disabled"),
            );
        }
        if plugin.pending_trust {
            return err_res(
                "PLUGIN_UNTRUSTED",
                format!(
                    "PLUGIN_UNTRUSTED: plugin '{plugin_id}' requires user trust before execution"
                ),
            );
        }

        let config = SyncConfig::load(&dir).unwrap_or_default();
        let cached = SyncDb::open(&dir)
            .ok()
            .and_then(|db| db.list_accounts().ok())
            .unwrap_or_default();

        let mut accounts: Vec<serde_json::Value> = Vec::new();
        for (platform, handle) in &config.handles {
            let trimmed = handle.trim();
            if trimmed.is_empty() {
                continue;
            }
            let matched = cached
                .iter()
                .find(|a| a.platform == *platform && a.handle.eq_ignore_ascii_case(trimmed));
            let mut obj = serde_json::Map::new();
            obj.insert(
                "platform".into(),
                serde_json::Value::String(platform.clone()),
            );
            obj.insert(
                "handle".into(),
                serde_json::Value::String(trimmed.to_owned()),
            );
            if let Some(rating) = matched.and_then(|a| a.rating) {
                obj.insert("rating".into(), serde_json::json!(rating));
            }
            if let Some(max_rating) = matched.and_then(|a| a.max_rating) {
                obj.insert("max_rating".into(), serde_json::json!(max_rating));
            }
            if let Some(rank) = matched.and_then(|a| a.rank.as_ref()) {
                obj.insert("rank".into(), serde_json::Value::String(rank.clone()));
            }
            accounts.push(serde_json::Value::Object(obj));
        }

        match serde_json::to_string(&accounts) {
            Ok(data_json) => PluginWorkspaceResultJs {
                ok: true,
                data_json: Some(data_json),
                error: None,
                error_code: None,
            },
            Err(err) => err_res("SERIALIZE", err.to_string()),
        }
    }

    pub fn plugin_workspace_query(
        &self,
        plugin_id: &str,
        method: &str,
        args_json: Option<&str>,
    ) -> PluginWorkspaceResultJs {
        let err_res = |code: &str, msg: String| PluginWorkspaceResultJs {
            ok: false,
            data_json: None,
            error: Some(msg),
            error_code: Some(code.to_owned()),
        };

        let dir = match self.effective_data_dir() {
            Ok(d) => d,
            Err(err) => return err_res("DATA_DIR", err),
        };
        let pm = PluginManager::new(&dir);
        let plugins = match pm.list_plugins() {
            Ok(list) => list,
            Err(err) => return err_res(err.code(), err.to_string()),
        };
        let Some(plugin) = plugins.into_iter().find(|p| p.manifest.id == plugin_id) else {
            return err_res(
                "PLUGIN_NOT_FOUND",
                format!("PLUGIN_NOT_FOUND: plugin '{plugin_id}' not found"),
            );
        };
        if !plugin.enabled {
            return err_res(
                "PLUGIN_DISABLED",
                format!("PLUGIN_DISABLED: plugin '{plugin_id}' is disabled"),
            );
        }
        if plugin.pending_trust {
            return err_res(
                "PLUGIN_UNTRUSTED",
                format!(
                    "PLUGIN_UNTRUSTED: plugin '{plugin_id}' requires user trust before execution"
                ),
            );
        }
        let method_name = method.trim();
        let is_write = matches!(
            method_name,
            "createProblem"
                | "create_problem"
                | "updateProblem"
                | "update_problem"
                | "setSamples"
                | "set_samples"
                | "upsertSnippet"
                | "upsert_snippet"
        );
        if is_write {
            if !plugin.manifest.permissions.workspace_write {
                return err_res(
                    "PERMISSION_DENIED",
                    format!(
                        "PERMISSION_DENIED: plugin '{plugin_id}' lacks permission 'workspace_write'"
                    ),
                );
            }
        } else if !plugin.manifest.permissions.workspace_read {
            return err_res(
                "PERMISSION_DENIED",
                format!(
                    "PERMISSION_DENIED: plugin '{plugin_id}' lacks permission 'workspace_read'"
                ),
            );
        }

        let ws = {
            let core = self.core.lock().unwrap();
            core.workspace.clone()
        };
        let Some(ws) = ws else {
            return err_res("NO_WORKSPACE", "no workspace is open".to_owned());
        };
        if is_write && ws.is_read_only() {
            return err_res(
                "WORKSPACE_READ_ONLY",
                "WORKSPACE_READ_ONLY: workspace is open read-only".to_owned(),
            );
        }

        let parse_id_arg = |raw: Option<&str>| -> Result<String, PluginWorkspaceResultJs> {
            let text = raw.map(str::trim).unwrap_or("");
            if text.is_empty() {
                return Err(err_res(
                    "INVALID_ARGS",
                    "missing required 'id' argument".to_owned(),
                ));
            }
            let val: serde_json::Value = serde_json::from_str(text)
                .map_err(|e| err_res("INVALID_ARGS", format!("invalid argsJson: {e}")))?;
            let id = match val {
                serde_json::Value::String(s) => Some(s),
                serde_json::Value::Object(mut obj) => {
                    obj.remove("id").and_then(|v| v.as_str().map(str::to_owned))
                }
                _ => None,
            };
            match id.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()) {
                Some(s) => Ok(s),
                None => Err(err_res(
                    "INVALID_ARGS",
                    "missing required 'id' argument".to_owned(),
                )),
            }
        };

        let bridge = StorageWorkspaceBridge::new(ws.clone());
        let data_json = match method_name {
            "listProblems" | "list_problems" => bridge
                .list_problems()
                .and_then(|v| Ok(serde_json::to_string(&v)?)),
            "getProblem" | "get_problem" => {
                let id = match parse_id_arg(args_json) {
                    Ok(id) => id,
                    Err(e) => return e,
                };
                bridge
                    .get_problem(&id)
                    .and_then(|v| Ok(serde_json::to_string(&v)?))
            }
            "listContests" | "list_contests" => bridge
                .list_contests()
                .and_then(|v| Ok(serde_json::to_string(&v)?)),
            "getContest" | "get_contest" => {
                let id = match parse_id_arg(args_json) {
                    Ok(id) => id,
                    Err(e) => return e,
                };
                bridge
                    .get_contest(&id)
                    .and_then(|v| Ok(serde_json::to_string(&v)?))
            }
            "listSnippets" | "list_snippets" => bridge
                .list_snippets()
                .and_then(|v| Ok(serde_json::to_string(&v)?)),
            "getSnippet" | "get_snippet" => {
                let id = match parse_id_arg(args_json) {
                    Ok(id) => id,
                    Err(e) => return e,
                };
                bridge
                    .get_snippet(&id)
                    .and_then(|v| Ok(serde_json::to_string(&v)?))
            }
            "listTrds" | "list_trds" => bridge
                .list_trds()
                .and_then(|v| Ok(serde_json::to_string(&v)?)),
            "getTrd" | "get_trd" => {
                let id = match parse_id_arg(args_json) {
                    Ok(id) => id,
                    Err(e) => return e,
                };
                bridge
                    .get_trd(&id)
                    .and_then(|v| Ok(serde_json::to_string(&v)?))
            }
            "createProblem" | "create_problem" => {
                let raw = args_json.unwrap_or("{}");
                let input: CreateProblemInput = match serde_json::from_str(raw) {
                    Ok(v) => v,
                    Err(e) => return err_res("INVALID_ARGUMENT", format!("invalid argsJson: {e}")),
                };
                bridge
                    .create_problem(input)
                    .and_then(|v| Ok(serde_json::to_string(&v)?))
            }
            "updateProblem" | "update_problem" => {
                #[derive(serde::Deserialize)]
                struct Args {
                    id: String,
                    patch: UpdateProblemPatch,
                }
                let raw = args_json.unwrap_or("{}");
                let args: Args = match serde_json::from_str(raw) {
                    Ok(v) => v,
                    Err(e) => return err_res("INVALID_ARGUMENT", format!("invalid argsJson: {e}")),
                };
                bridge
                    .update_problem(&args.id, args.patch)
                    .and_then(|v| Ok(serde_json::to_string(&v)?))
            }
            "setSamples" | "set_samples" => {
                #[derive(serde::Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct Args {
                    problem_id: String,
                    samples: Vec<WorkspaceSample>,
                }
                let raw = args_json.unwrap_or("{}");
                let args: Args = match serde_json::from_str(raw) {
                    Ok(v) => v,
                    Err(e) => return err_res("INVALID_ARGUMENT", format!("invalid argsJson: {e}")),
                };
                bridge
                    .set_samples(&args.problem_id, args.samples)
                    .and_then(|v| Ok(serde_json::to_string(&v)?))
            }
            "upsertSnippet" | "upsert_snippet" => {
                let raw = args_json.unwrap_or("{}");
                let input: UpsertSnippetInput = match serde_json::from_str(raw) {
                    Ok(v) => v,
                    Err(e) => return err_res("INVALID_ARGUMENT", format!("invalid argsJson: {e}")),
                };
                bridge
                    .upsert_snippet(input)
                    .and_then(|v| Ok(serde_json::to_string(&v)?))
            }
            other => {
                return err_res(
                    "INVALID_METHOD",
                    format!("unknown workspace query method '{other}'"),
                );
            }
        };

        match data_json {
            Ok(data_json) => {
                if is_write {
                    self.refresh_workspace_after_plugin_write(&ws);
                }
                PluginWorkspaceResultJs {
                    ok: true,
                    data_json: Some(data_json),
                    error: None,
                    error_code: None,
                }
            }
            Err(err) => err_res(err.code(), err.to_string()),
        }
    }

    pub fn plugin_render_typst(
        &self,
        plugin_id: &str,
        source: &str,
        opts_json: Option<&str>,
    ) -> PluginWorkspaceResultJs {
        let err_res = |code: &str, msg: String| PluginWorkspaceResultJs {
            ok: false,
            data_json: None,
            error: Some(msg),
            error_code: Some(code.to_owned()),
        };

        let dir = match self.effective_data_dir() {
            Ok(d) => d,
            Err(err) => return err_res("DATA_DIR", err),
        };
        let pm = PluginManager::new(&dir);
        let plugins = match pm.list_plugins() {
            Ok(list) => list,
            Err(err) => return err_res(err.code(), err.to_string()),
        };
        let Some(plugin) = plugins.into_iter().find(|p| p.manifest.id == plugin_id) else {
            return err_res(
                "PLUGIN_NOT_FOUND",
                format!("PLUGIN_NOT_FOUND: plugin '{plugin_id}' not found"),
            );
        };
        if !plugin.enabled {
            return err_res(
                "PLUGIN_DISABLED",
                format!("PLUGIN_DISABLED: plugin '{plugin_id}' is disabled"),
            );
        }
        if plugin.pending_trust {
            return err_res(
                "PLUGIN_UNTRUSTED",
                format!(
                    "PLUGIN_UNTRUSTED: plugin '{plugin_id}' requires user trust before execution"
                ),
            );
        }

        let opts: RenderTypstOptions = match opts_json.map(str::trim).filter(|s| !s.is_empty()) {
            Some(raw) => match serde_json::from_str(raw) {
                Ok(v) => v,
                Err(e) => return err_res("INVALID_ARGUMENT", format!("invalid optsJson: {e}")),
            },
            None => RenderTypstOptions::default(),
        };
        match render_typst_snippet(source, &opts) {
            Ok(rendered) => match serde_json::to_string(&rendered) {
                Ok(data_json) => PluginWorkspaceResultJs {
                    ok: true,
                    data_json: Some(data_json),
                    error: None,
                    error_code: None,
                },
                Err(err) => err_res("SERIALIZE_ERROR", err.to_string()),
            },
            Err(err) => err_res(err.code(), err.to_string()),
        }
    }

    pub fn plugin_render_math(
        &self,
        plugin_id: &str,
        latex: &str,
        opts_json: Option<&str>,
    ) -> PluginWorkspaceResultJs {
        let err_res = |code: &str, msg: String| PluginWorkspaceResultJs {
            ok: false,
            data_json: None,
            error: Some(msg),
            error_code: Some(code.to_owned()),
        };

        let dir = match self.effective_data_dir() {
            Ok(d) => d,
            Err(err) => return err_res("DATA_DIR", err),
        };
        let pm = PluginManager::new(&dir);
        let plugins = match pm.list_plugins() {
            Ok(list) => list,
            Err(err) => return err_res(err.code(), err.to_string()),
        };
        let Some(plugin) = plugins.into_iter().find(|p| p.manifest.id == plugin_id) else {
            return err_res(
                "PLUGIN_NOT_FOUND",
                format!("PLUGIN_NOT_FOUND: plugin '{plugin_id}' not found"),
            );
        };
        if !plugin.enabled {
            return err_res(
                "PLUGIN_DISABLED",
                format!("PLUGIN_DISABLED: plugin '{plugin_id}' is disabled"),
            );
        }
        if plugin.pending_trust {
            return err_res(
                "PLUGIN_UNTRUSTED",
                format!(
                    "PLUGIN_UNTRUSTED: plugin '{plugin_id}' requires user trust before execution"
                ),
            );
        }

        let opts: RenderMathOptions = match opts_json.map(str::trim).filter(|s| !s.is_empty()) {
            Some(raw) => match serde_json::from_str(raw) {
                Ok(v) => v,
                Err(e) => return err_res("INVALID_ARGUMENT", format!("invalid optsJson: {e}")),
            },
            None => RenderMathOptions::default(),
        };
        match render_math_snippet(latex, &opts) {
            Ok(rendered) => match serde_json::to_string(&rendered) {
                Ok(data_json) => PluginWorkspaceResultJs {
                    ok: true,
                    data_json: Some(data_json),
                    error: None,
                    error_code: None,
                },
                Err(err) => err_res("SERIALIZE_ERROR", err.to_string()),
            },
            Err(err) => err_res(err.code(), err.to_string()),
        }
    }

    pub fn plugin_render_svg_png(svg: &str, scale: Option<f64>) -> PluginRenderPngResultJs {
        match svg_to_png_base64(svg, scale.map(|s| s as f32)) {
            Ok(b64) => PluginRenderPngResultJs {
                ok: true,
                png_base64: Some(b64),
                error: None,
            },
            Err(err) => PluginRenderPngResultJs {
                ok: false,
                png_base64: None,
                error: Some(err.to_string()),
            },
        }
    }

    pub fn plugin_list_commands(&self, context: Option<String>) -> JsonReportResultJs {
        let dir = match self.effective_data_dir() {
            Ok(d) => d,
            Err(err) => return JsonReportResultJs::err("data_dir", err),
        };
        let parsed_ctx = match context.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            None => None,
            Some("global") => Some(CommandContext::Global),
            Some("problem") => Some(CommandContext::Problem),
            Some("contest") => Some(CommandContext::Contest),
            Some(other) => {
                return JsonReportResultJs::err(
                    "invalid",
                    format!("unknown command context '{other}'"),
                );
            }
        };
        let pm = PluginManager::new(&dir);
        let commands = pm.list_commands(parsed_ctx);
        match serde_json::to_string(&commands) {
            Ok(json) => JsonReportResultJs::ok(Some(json), None),
            Err(err) => JsonReportResultJs::err("serialize", err.to_string()),
        }
    }

    pub fn plugin_run_command(
        &self,
        plugin_id: &str,
        command_id: &str,
        context_payload_json: &str,
    ) -> JsonReportResultJs {
        let dir = match self.effective_data_dir() {
            Ok(d) => d,
            Err(err) => return JsonReportResultJs::err("data_dir", err),
        };
        let payload: serde_json::Value = if context_payload_json.trim().is_empty() {
            serde_json::json!({})
        } else {
            match serde_json::from_str(context_payload_json) {
                Ok(v) => v,
                Err(err) => return JsonReportResultJs::err("invalid_json", err.to_string()),
            }
        };
        let pm = PluginManager::new(&dir);
        let (ws_reader, ws_writer, mutated, ws_opt) =
            self.workspace_bridges_if_allowed(&pm, plugin_id);
        let res = pm.run_command_with_writer(plugin_id, command_id, payload, ws_reader, ws_writer);
        if mutated.load(Ordering::Acquire)
            && let Some(ref ws) = ws_opt
        {
            self.refresh_workspace_after_plugin_write(ws);
        }
        match res {
            Ok(action) => match serde_json::to_string(&action) {
                Ok(json) => JsonReportResultJs::ok(Some(json), None),
                Err(err) => JsonReportResultJs::err("serialize", err.to_string()),
            },
            Err(err) => JsonReportResultJs::err("plugin_error", err.to_string()),
        }
    }

    pub fn plugin_list_importers(&self) -> JsonReportResultJs {
        let dir = match self.effective_data_dir() {
            Ok(d) => d,
            Err(err) => return JsonReportResultJs::err("data_dir", err),
        };
        let pm = PluginManager::new(&dir);
        let list = pm.list_importers();
        match serde_json::to_string(&list) {
            Ok(json) => JsonReportResultJs::ok(Some(json), None),
            Err(err) => JsonReportResultJs::err("serialize", err.to_string()),
        }
    }

    pub fn plugin_importer_extensions(&self, plugin_id: &str, importer_id: &str) -> Vec<String> {
        let Ok(dir) = self.effective_data_dir() else {
            return Vec::new();
        };
        let pm = PluginManager::new(&dir);
        pm.list_importers()
            .into_iter()
            .find(|r| r.plugin_id == plugin_id && r.importer.id == importer_id)
            .map(|r| {
                r.importer
                    .extensions
                    .into_iter()
                    .map(|ext| ext.trim().trim_start_matches('.').to_owned())
                    .filter(|ext| !ext.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn plugin_run_importer_paths(
        &self,
        plugin_id: &str,
        importer_id: &str,
        paths: &[PathBuf],
    ) -> JsonReportResultJs {
        let dir = match self.effective_data_dir() {
            Ok(d) => d,
            Err(err) => return JsonReportResultJs::err("data_dir", err),
        };
        let ws = {
            let core = self.core.lock().unwrap();
            core.workspace.clone()
        };
        let Some(ws) = ws else {
            return JsonReportResultJs::err("no_workspace", "no workspace is open");
        };
        if ws.is_read_only() {
            return JsonReportResultJs::err("read_only", "workspace is read-only");
        }

        let mut files = Vec::with_capacity(paths.len());
        for path in paths {
            let bytes = match std::fs::read(path) {
                Ok(b) => b,
                Err(err) => return JsonReportResultJs::err("io", err.to_string()),
            };
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "file".to_owned());
            let content_base64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
            let text = String::from_utf8(bytes).ok();
            files.push(ImportFileInput {
                name,
                content_base64,
                text,
            });
        }

        let pm = PluginManager::new(&dir);
        let draft = match pm.run_importer(plugin_id, importer_id, &files) {
            Ok(d) => d,
            Err(err) => return JsonReportResultJs::err("plugin_error", err.to_string()),
        };

        match apply_import_draft(&ws, &draft) {
            Ok(report) => {
                if let Ok(rebuilt) = xcpc_storage::WorkspaceIndex::scan(&ws) {
                    let mut core = self.core.lock().unwrap();
                    if core
                        .workspace
                        .as_ref()
                        .is_some_and(|cur| cur.root() == ws.root())
                    {
                        core.index = rebuilt;
                    }
                }
                self.invalidate_cache();
                match serde_json::to_string(&report) {
                    Ok(json) => JsonReportResultJs::ok(Some(json), None),
                    Err(err) => JsonReportResultJs::err("serialize", err.to_string()),
                }
            }
            Err(err) => JsonReportResultJs::err("import_error", err.to_string()),
        }
    }

    pub fn plugin_list_exporters(&self) -> JsonReportResultJs {
        let dir = match self.effective_data_dir() {
            Ok(d) => d,
            Err(err) => return JsonReportResultJs::err("data_dir", err),
        };
        let pm = PluginManager::new(&dir);
        let list = pm.list_exporters();
        match serde_json::to_string(&list) {
            Ok(json) => JsonReportResultJs::ok(Some(json), None),
            Err(err) => JsonReportResultJs::err("serialize", err.to_string()),
        }
    }

    pub fn plugin_run_exporter(
        &self,
        plugin_id: &str,
        exporter_id: &str,
        kind: &str,
        id: &str,
    ) -> JsonReportResultJs {
        let dir = match self.effective_data_dir() {
            Ok(d) => d,
            Err(err) => return JsonReportResultJs::err("data_dir", err),
        };
        let ws = {
            let core = self.core.lock().unwrap();
            core.workspace.clone()
        };
        let Some(ws) = ws else {
            return JsonReportResultJs::err("no_workspace", "no workspace is open");
        };

        let ctx = match build_export_context(&ws, kind, id) {
            Ok(c) => c,
            Err(err) => return JsonReportResultJs::err("not_found", err),
        };

        let pm = PluginManager::new(&dir);
        let output = match pm.run_exporter(plugin_id, exporter_id, &ctx) {
            Ok(o) => o,
            Err(err) => return JsonReportResultJs::err("plugin_error", err.to_string()),
        };

        let out_dir = ws
            .root()
            .join("build")
            .join("exports")
            .join(format!("{plugin_id}-{id}"));
        let written = match write_export_output(&ws, &output, &out_dir) {
            Ok(paths) => paths,
            Err(err) => return JsonReportResultJs::err("export_error", err.to_string()),
        };

        let payload = serde_json::json!({
            "summary": output.summary,
            "out_dir": out_dir.display().to_string(),
            "files": written.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
        });
        match serde_json::to_string(&payload) {
            Ok(json) => JsonReportResultJs::ok(Some(json), None),
            Err(err) => JsonReportResultJs::err("serialize", err.to_string()),
        }
    }
}

struct TrackingWorkspaceWriter {
    inner: StorageWorkspaceBridge,
    mutated: Arc<AtomicBool>,
}

impl WorkspaceWriter for TrackingWorkspaceWriter {
    fn create_problem(
        &self,
        input: CreateProblemInput,
    ) -> xcpc_plugin::Result<WorkspaceProblemDetail> {
        let out = self.inner.create_problem(input)?;
        self.mutated.store(true, Ordering::Release);
        Ok(out)
    }

    fn update_problem(
        &self,
        id: &str,
        patch: UpdateProblemPatch,
    ) -> xcpc_plugin::Result<WorkspaceProblemDetail> {
        let out = self.inner.update_problem(id, patch)?;
        self.mutated.store(true, Ordering::Release);
        Ok(out)
    }

    fn set_samples(
        &self,
        problem_id: &str,
        samples: Vec<WorkspaceSample>,
    ) -> xcpc_plugin::Result<SetSamplesResult> {
        let out = self.inner.set_samples(problem_id, samples)?;
        self.mutated.store(true, Ordering::Release);
        Ok(out)
    }

    fn upsert_snippet(
        &self,
        input: UpsertSnippetInput,
    ) -> xcpc_plugin::Result<WorkspaceSnippetDetail> {
        let out = self.inner.upsert_snippet(input)?;
        self.mutated.store(true, Ordering::Release);
        Ok(out)
    }
}

fn build_problem_export_context(ws: &Workspace, id: &str) -> Result<ExportContext, String> {
    let problem = ws.load_problem(id).map_err(|e| e.to_string())?;
    let lang = problem.default_language();
    let stmt_rel = problem
        .statement(lang)
        .map(|s| s.file.path.clone())
        .unwrap_or_else(|| format!("{}/statement.md", problem.dir));
    let statement_md = ws.read_text(&stmt_rel).ok();
    let editorial_md = ws.read_text(&format!("{}/editorial.md", problem.dir)).ok();
    let samples = problem
        .samples
        .iter()
        .map(|s| DraftSample {
            input: s.input.clone(),
            output: s.output.clone(),
        })
        .collect::<Vec<_>>();
    let pdf_path = ws.root().join("build").join(format!("{id}.pdf"));
    let pdf_base64 = std::fs::read(&pdf_path)
        .ok()
        .map(|b| base64::engine::general_purpose::STANDARD.encode(b));
    let metadata = serde_json::json!({
        "tags": problem.meta.tags,
        "rating": problem.meta.rating,
        "author": problem.meta.author,
        "time_limit": problem.meta.limits.time,
        "memory_limit": problem.meta.limits.memory,
    });
    Ok(ExportContext {
        kind: "problem".to_owned(),
        id: id.to_owned(),
        title: problem.meta.title,
        metadata,
        statement_md,
        editorial_md,
        samples: Some(samples),
        pdf_base64,
        problems: None,
    })
}

fn build_export_context(ws: &Workspace, kind: &str, id: &str) -> Result<ExportContext, String> {
    if kind.eq_ignore_ascii_case("contest") {
        let contest = ws.load_contest(id).map_err(|e| e.to_string())?;
        let mut problems = Vec::with_capacity(contest.meta.problems.len());
        for item in &contest.meta.problems {
            if let Ok(p_ctx) = build_problem_export_context(ws, &item.id) {
                problems.push(p_ctx);
            }
        }
        let pdf_path = ws.root().join("build").join(format!("{id}.pdf"));
        let pdf_base64 = std::fs::read(&pdf_path)
            .ok()
            .map(|b| base64::engine::general_purpose::STANDARD.encode(b));
        let metadata = serde_json::json!({
            "location": contest.meta.location,
            "date": contest.meta.date,
            "author": contest.meta.author,
            "frozen": contest.meta.frozen,
        });
        Ok(ExportContext {
            kind: "contest".to_owned(),
            id: id.to_owned(),
            title: contest.meta.name,
            metadata,
            statement_md: None,
            editorial_md: None,
            samples: None,
            pdf_base64,
            problems: Some(problems),
        })
    } else {
        build_problem_export_context(ws, id)
    }
}

/// Atomically writes `bytes` to `path` via a sibling temporary file + rename.
pub fn atomic_write_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    static TMP_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "output".to_owned());
    let nonce = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp_name = format!(".{file_name}.tmp.{}.{nonce}", std::process::id());
    let tmp_path = parent.join(tmp_name);
    std::fs::write(&tmp_path, bytes)?;
    if let Err(err) = std::fs::rename(&tmp_path, path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(err);
    }
    Ok(())
}

/// Ensures [`TrainingStatus::None`] is used as fallback string when training is disabled.
#[inline]
pub(crate) fn default_training_status_str() -> String {
    TrainingStatus::None.as_str().to_owned()
}

// ---- Phase 4D: Synchronous Plugin KV Bridge & `notify` Hot Reload Watcher --------------

impl State {
    pub fn plugin_kv_get(&self, plugin_id: &str, key: &str) -> PluginKvResultJs {
        let dir = match self.effective_data_dir() {
            Ok(d) => d,
            Err(err) => {
                return PluginKvResultJs {
                    ok: false,
                    found: false,
                    value_json: None,
                    error: Some(err),
                };
            }
        };
        let pm = PluginManager::new(dir);
        let store = match pm.kv_store(plugin_id) {
            Ok(s) => s,
            Err(e) => {
                return PluginKvResultJs {
                    ok: false,
                    found: false,
                    value_json: None,
                    error: Some(e.to_string()),
                };
            }
        };
        match store.get(key) {
            Ok(Some(val)) => PluginKvResultJs {
                ok: true,
                found: true,
                value_json: serde_json::to_string(&val).ok(),
                error: None,
            },
            Ok(None) => PluginKvResultJs {
                ok: true,
                found: false,
                value_json: None,
                error: None,
            },
            Err(e) => PluginKvResultJs {
                ok: false,
                found: false,
                value_json: None,
                error: Some(e.to_string()),
            },
        }
    }

    pub fn plugin_kv_set(&self, plugin_id: &str, key: &str, value_json: &str) -> ActionResult {
        let dir = match self.effective_data_dir() {
            Ok(d) => d,
            Err(err) => return ActionResult::err("data_dir", err),
        };
        let val: serde_json::Value = match serde_json::from_str(value_json) {
            Ok(v) => v,
            Err(e) => return ActionResult::err("invalid_json", e.to_string()),
        };
        let pm = PluginManager::new(dir);
        let store = match pm.kv_store(plugin_id) {
            Ok(s) => s,
            Err(e) => return ActionResult::err("plugin_error", e.to_string()),
        };
        match store.set(key, val) {
            Ok(()) => ActionResult::ok(),
            Err(e) => ActionResult::err("plugin_error", e.to_string()),
        }
    }

    pub fn plugin_kv_remove(&self, plugin_id: &str, key: &str) -> ActionResult {
        let dir = match self.effective_data_dir() {
            Ok(d) => d,
            Err(err) => return ActionResult::err("data_dir", err),
        };
        let pm = PluginManager::new(dir);
        let store = match pm.kv_store(plugin_id) {
            Ok(s) => s,
            Err(e) => return ActionResult::err("plugin_error", e.to_string()),
        };
        match store.delete(key) {
            Ok(_) => ActionResult::ok(),
            Err(e) => ActionResult::err("plugin_error", e.to_string()),
        }
    }
}

/// Returns `true` only for filesystem mutation events that can actually change plugin
/// files or directory entries. Explicitly ignores `Access` (OPEN/READ/CLOSE_NOWRITE)
/// and `Modify(Metadata)` events to prevent self-triggering reload loops on Linux inotify,
/// while accepting `Modify(Any)` so Windows `FILE_ACTION_MODIFIED` triggers hot reload.
pub(crate) fn is_relevant_plugin_watch_event(kind: &notify::EventKind) -> bool {
    use notify::EventKind;
    use notify::event::ModifyKind;

    matches!(
        kind,
        EventKind::Create(_)
            | EventKind::Modify(ModifyKind::Data(_) | ModifyKind::Name(_) | ModifyKind::Any)
            | EventKind::Remove(_)
    )
}

/// Extracts affected plugin IDs from watcher `changed_paths` by matching `<dataDir>/plugins/<id>/...`
/// and registered dev-link target roots.
pub(crate) fn extract_changed_plugin_ids(
    data_dir: &Path,
    links: &[(String, PathBuf)],
    changed_paths: &[String],
) -> Vec<String> {
    use std::collections::BTreeSet;
    use std::path::Component;

    let plugins_dir = data_dir.join("plugins");
    let canon_plugins_dir = std::fs::canonicalize(&plugins_dir).ok();
    let link_roots: Vec<(&str, &Path, Option<PathBuf>)> = links
        .iter()
        .map(|(id, target)| {
            (
                id.as_str(),
                target.as_path(),
                std::fs::canonicalize(target).ok(),
            )
        })
        .collect();

    let mut ids = BTreeSet::new();
    for raw in changed_paths {
        let path = Path::new(raw);
        let canon_path = std::fs::canonicalize(path).ok();

        let rel_under_plugins = path.strip_prefix(&plugins_dir).ok().or_else(|| {
            canon_plugins_dir.as_deref().and_then(|cp| {
                path.strip_prefix(cp)
                    .ok()
                    .or_else(|| canon_path.as_deref().and_then(|p| p.strip_prefix(cp).ok()))
            })
        });
        if let Some(rel) = rel_under_plugins
            && let Some(Component::Normal(first)) = rel.components().next()
        {
            let name = first.to_string_lossy();
            if !name.is_empty() {
                ids.insert(name.into_owned());
            }
        }

        for (id, root, canon_root) in &link_roots {
            let matched = path.starts_with(root)
                || canon_root.as_deref().is_some_and(|cr| {
                    path.starts_with(cr)
                        || canon_path.as_deref().is_some_and(|cp| cp.starts_with(cr))
                });
            if matched {
                ids.insert((*id).to_owned());
            }
        }
    }

    ids.into_iter().collect()
}

enum PluginWatchControl {
    Refresh,
    Stop,
}

/// Watches `<dataDir>/plugins/`, `<dataDir>/plugin_links.json`, and all registered symlinked
/// development plugin target directories for `manifest.toml` / `index.js` / `ui.html` / `ui.js` changes.
pub(crate) struct PluginFileWatcher {
    ctrl_tx: std::sync::mpsc::Sender<PluginWatchControl>,
    event_rx: Arc<std::sync::Mutex<std::sync::mpsc::Receiver<PluginWatchEventJs>>>,
}

impl PluginFileWatcher {
    pub fn start(data_dir: PathBuf) -> Option<Self> {
        use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
        use std::collections::BTreeSet;
        use std::sync::mpsc;
        use std::time::Duration;

        let (raw_tx, raw_rx) = mpsc::channel::<notify::Result<Event>>();
        let mut watcher: RecommendedWatcher = notify::recommended_watcher(move |res| {
            let _ = raw_tx.send(res);
        })
        .ok()?;

        let (ctrl_tx, ctrl_rx) = mpsc::channel::<PluginWatchControl>();
        let (event_tx, event_rx) = mpsc::channel::<PluginWatchEventJs>();

        let sync_watches = |w: &mut RecommendedWatcher,
                            watched: &mut BTreeSet<PathBuf>,
                            active_links: &mut Vec<(String, PathBuf)>,
                            data_dir: &Path| {
            let mut desired: BTreeSet<PathBuf> = BTreeSet::new();
            if data_dir.is_dir() {
                desired.insert(data_dir.to_path_buf());
            }
            let plugins_dir = data_dir.join("plugins");
            if plugins_dir.is_dir() {
                desired.insert(plugins_dir.clone());
            }
            active_links.clear();
            let pm = PluginManager::new(data_dir);
            if let Ok(links) = pm.list_links() {
                for link in links {
                    let p = PathBuf::from(&link.target);
                    active_links.push((link.id, p.clone()));
                    if p.is_dir() {
                        desired.insert(p);
                    }
                }
            }
            // Unwatch removed targets.
            for old in watched.difference(&desired) {
                let _ = w.unwatch(old);
            }
            // Watch newly added targets.
            for new_path in desired.difference(watched) {
                let mode = if new_path == data_dir {
                    RecursiveMode::NonRecursive
                } else {
                    RecursiveMode::Recursive
                };
                let _ = w.watch(new_path, mode);
            }
            *watched = desired;
        };

        std::thread::Builder::new()
            .name("xcpc-plugin-watcher".into())
            .spawn(move || {
                let mut watched = BTreeSet::new();
                let mut active_links: Vec<(String, PathBuf)> = Vec::new();
                sync_watches(&mut watcher, &mut watched, &mut active_links, &data_dir);

                let is_relevant_path = |p: &Path| -> bool {
                    let Some(name) = p.file_name().and_then(|n| n.to_str()) else {
                        return false;
                    };
                    if matches!(
                        name,
                        "manifest.toml" | "index.js" | "ui.html" | "ui.js" | "plugin_links.json"
                    ) {
                        return true;
                    }
                    let lower = name.to_ascii_lowercase();
                    lower.ends_with(".typ")
                        || lower.ends_with(".ttf")
                        || lower.ends_with(".otf")
                        || lower.ends_with(".ttc")
                        || lower.ends_with(".svg")
                        || lower.ends_with(".png")
                };

                loop {
                    // Check control messages non-blockingly.
                    while let Ok(cmd) = ctrl_rx.try_recv() {
                        match cmd {
                            PluginWatchControl::Refresh => {
                                sync_watches(
                                    &mut watcher,
                                    &mut watched,
                                    &mut active_links,
                                    &data_dir,
                                );
                            }
                            PluginWatchControl::Stop => {
                                return;
                            }
                        }
                    }

                    match raw_rx.recv_timeout(Duration::from_millis(200)) {
                        Ok(Ok(ev)) => {
                            if !is_relevant_plugin_watch_event(&ev.kind) {
                                continue;
                            }
                            let mut changed_paths: BTreeSet<String> = BTreeSet::new();
                            let mut links_changed = false;
                            for p in &ev.paths {
                                if is_relevant_path(p) {
                                    if p.file_name().and_then(|n| n.to_str())
                                        == Some("plugin_links.json")
                                    {
                                        links_changed = true;
                                    }
                                    changed_paths.insert(p.to_string_lossy().into_owned());
                                } else if p.parent() == Some(data_dir.join("plugins").as_path()) {
                                    // New plugin directory or symlink created/removed directly in plugins/
                                    links_changed = true;
                                    changed_paths.insert(p.to_string_lossy().into_owned());
                                }
                            }
                            if changed_paths.is_empty() {
                                continue;
                            }
                            // 150ms debounce window to coalesce `vite build --watch` multi-file writes.
                            let debounce = Duration::from_millis(150);
                            while let Ok(next_res) = raw_rx.recv_timeout(debounce) {
                                if let Ok(next_ev) = next_res
                                    && is_relevant_plugin_watch_event(&next_ev.kind)
                                {
                                    for p in &next_ev.paths {
                                        if is_relevant_path(p) {
                                            if p.file_name().and_then(|n| n.to_str())
                                                == Some("plugin_links.json")
                                            {
                                                links_changed = true;
                                            }
                                            changed_paths.insert(p.to_string_lossy().into_owned());
                                        } else if p.parent()
                                            == Some(data_dir.join("plugins").as_path())
                                        {
                                            links_changed = true;
                                            changed_paths.insert(p.to_string_lossy().into_owned());
                                        }
                                    }
                                }
                            }
                            if links_changed {
                                sync_watches(
                                    &mut watcher,
                                    &mut watched,
                                    &mut active_links,
                                    &data_dir,
                                );
                            }
                            let paths: Vec<String> = changed_paths.into_iter().collect();
                            let changed_plugin_ids =
                                extract_changed_plugin_ids(&data_dir, &active_links, &paths);
                            if event_tx
                                .send(PluginWatchEventJs {
                                    kind: "changed".to_owned(),
                                    paths,
                                    changed_plugin_ids,
                                })
                                .is_err()
                            {
                                return;
                            }
                        }
                        Ok(Err(_)) => {}
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        Err(mpsc::RecvTimeoutError::Disconnected) => return,
                    }
                }
            })
            .ok()?;

        Some(Self {
            ctrl_tx,
            event_rx: Arc::new(std::sync::Mutex::new(event_rx)),
        })
    }

    pub fn refresh(&self) {
        let _ = self.ctrl_tx.send(PluginWatchControl::Refresh);
    }

    pub fn receiver(&self) -> Arc<std::sync::Mutex<std::sync::mpsc::Receiver<PluginWatchEventJs>>> {
        Arc::clone(&self.event_rx)
    }

    pub fn stop(&self) {
        let _ = self.ctrl_tx.send(PluginWatchControl::Stop);
    }
}

impl Drop for PluginFileWatcher {
    fn drop(&mut self) {
        self.stop();
    }
}

// ---- Phase 4D+: Uninstall --------------------------------------------------------------

impl State {
    pub fn uninstall_plugin(&self, id: &str) -> InstallPluginResultJs {
        let dir = match self.effective_data_dir() {
            Ok(d) => d,
            Err(err) => {
                return InstallPluginResultJs {
                    ok: false,
                    cancelled: false,
                    plugin_id: None,
                    overview: None,
                    error: Some(err),
                };
            }
        };
        let pm = PluginManager::new(dir);
        match pm.uninstall_plugin(id.trim()) {
            Ok(true) => {
                self.refresh_plugin_watcher();
                self.sync_engine_plugin_mounts();
                InstallPluginResultJs {
                    ok: true,
                    cancelled: false,
                    plugin_id: Some(id.trim().to_owned()),
                    overview: Some(self.get_plugins_overview()),
                    error: None,
                }
            }
            Ok(false) => InstallPluginResultJs {
                ok: false,
                cancelled: false,
                plugin_id: None,
                overview: None,
                error: Some(format!(
                    "plugin '{}' cannot be uninstalled (not locally installed)",
                    id.trim()
                )),
            },
            Err(e) => InstallPluginResultJs {
                ok: false,
                cancelled: false,
                plugin_id: None,
                overview: None,
                error: Some(e.to_string()),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_watch_event_filter_ignores_reads_and_metadata_and_accepts_mutations() {
        use notify::EventKind;
        use notify::event::{
            AccessKind, AccessMode, CreateKind, DataChange, MetadataKind, ModifyKind, RemoveKind,
            RenameMode,
        };

        // Access events (Linux inotify OPEN / ACCESS / CLOSE_NOWRITE) MUST be ignored
        assert!(!is_relevant_plugin_watch_event(&EventKind::Access(
            AccessKind::Any
        )));
        assert!(!is_relevant_plugin_watch_event(&EventKind::Access(
            AccessKind::Read
        )));
        assert!(!is_relevant_plugin_watch_event(&EventKind::Access(
            AccessKind::Open(AccessMode::Any)
        )));
        assert!(!is_relevant_plugin_watch_event(&EventKind::Access(
            AccessKind::Open(AccessMode::Read)
        )));
        assert!(!is_relevant_plugin_watch_event(&EventKind::Access(
            AccessKind::Close(AccessMode::Read)
        )));
        assert!(!is_relevant_plugin_watch_event(&EventKind::Access(
            AccessKind::Close(AccessMode::Write)
        )));
        assert!(!is_relevant_plugin_watch_event(&EventKind::Access(
            AccessKind::Close(AccessMode::Any)
        )));

        // Metadata-only and non-mutation events MUST be ignored
        assert!(!is_relevant_plugin_watch_event(&EventKind::Modify(
            ModifyKind::Metadata(MetadataKind::Any)
        )));
        assert!(!is_relevant_plugin_watch_event(&EventKind::Modify(
            ModifyKind::Metadata(MetadataKind::AccessTime)
        )));
        assert!(!is_relevant_plugin_watch_event(&EventKind::Modify(
            ModifyKind::Metadata(MetadataKind::WriteTime)
        )));
        assert!(!is_relevant_plugin_watch_event(&EventKind::Modify(
            ModifyKind::Metadata(MetadataKind::Permissions)
        )));
        assert!(!is_relevant_plugin_watch_event(&EventKind::Modify(
            ModifyKind::Other
        )));
        assert!(!is_relevant_plugin_watch_event(&EventKind::Any));
        assert!(!is_relevant_plugin_watch_event(&EventKind::Other));

        // Real mutations MUST be accepted (including ModifyKind::Any for Windows FILE_ACTION_MODIFIED)
        assert!(is_relevant_plugin_watch_event(&EventKind::Create(
            CreateKind::File
        )));
        assert!(is_relevant_plugin_watch_event(&EventKind::Create(
            CreateKind::Folder
        )));
        assert!(is_relevant_plugin_watch_event(&EventKind::Create(
            CreateKind::Any
        )));
        assert!(is_relevant_plugin_watch_event(&EventKind::Modify(
            ModifyKind::Any
        )));
        assert!(is_relevant_plugin_watch_event(&EventKind::Modify(
            ModifyKind::Data(DataChange::Content)
        )));
        assert!(is_relevant_plugin_watch_event(&EventKind::Modify(
            ModifyKind::Data(DataChange::Size)
        )));
        assert!(is_relevant_plugin_watch_event(&EventKind::Modify(
            ModifyKind::Data(DataChange::Any)
        )));
        assert!(is_relevant_plugin_watch_event(&EventKind::Modify(
            ModifyKind::Name(RenameMode::Any)
        )));
        assert!(is_relevant_plugin_watch_event(&EventKind::Modify(
            ModifyKind::Name(RenameMode::To)
        )));
        assert!(is_relevant_plugin_watch_event(&EventKind::Modify(
            ModifyKind::Name(RenameMode::From)
        )));
        assert!(is_relevant_plugin_watch_event(&EventKind::Modify(
            ModifyKind::Name(RenameMode::Both)
        )));
        assert!(is_relevant_plugin_watch_event(&EventKind::Remove(
            RemoveKind::File
        )));
        assert!(is_relevant_plugin_watch_event(&EventKind::Remove(
            RemoveKind::Folder
        )));
        assert!(is_relevant_plugin_watch_event(&EventKind::Remove(
            RemoveKind::Any
        )));
    }

    #[test]
    fn extract_changed_plugin_ids_matches_plugins_dir_and_dev_links() {
        let data_dir = PathBuf::from("/tmp/xcpc-data-test");
        let links = vec![(
            "dev-tool".to_owned(),
            PathBuf::from("/home/dev/my-plugin/dist"),
        )];
        let changed_paths = vec![
            "/tmp/xcpc-data-test/plugins/math-bitwise/ui.html".to_owned(),
            "/tmp/xcpc-data-test/plugins/graph-editor".to_owned(),
            "/home/dev/my-plugin/dist/ui.html".to_owned(),
            "/tmp/xcpc-data-test/plugin_links.json".to_owned(),
        ];

        let ids = extract_changed_plugin_ids(&data_dir, &links, &changed_paths);
        assert_eq!(ids, vec!["dev-tool", "graph-editor", "math-bitwise"]);
    }
}
