//! Phase 4A desktop bridge helpers: training status derivation & logging, OJ problem URL
//! resolution, plugin management, credentials, decentralized repo catalogs, and OJ sync.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use std::str::FromStr;

use base64::Engine as _;
use xcpc_domain::{
    ProblemSource, SyncedProblemStatus, TrainingLogEntry, TrainingLogStatus, TrainingStatus,
};
use xcpc_plugin::{
    CommandContext, DraftSample, ExportContext, HostContext, ImportFileInput, OjContext,
    OjPluginClient, PluginCapability, PluginManager, PluginOrigin, SandboxLimits, UreqHttpClient,
    WorkspaceProblemDetail, WorkspaceProblemSummary, WorkspaceReader, apply_import_draft,
    svg_to_png_base64, write_export_output,
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
    ActionResult, CreateProblemFromUrlResultJs, EditorialListResultJs, EditorialSummaryJs,
    ExportPluginResultJs, IngestProblemRequestJs, IngestProblemResultJs, InstallPluginResultJs,
    JsonReportResultJs, ManualLogResultJs, PlatformSyncReportJs, PluginCredentialFieldJs,
    PluginRenderPngResultJs, PluginRepoConfigJs, PluginSummaryJs, PluginUiSourceResultJs,
    PluginsOverviewJs, ProblemSourceJs, RepoCatalogResultJs, RepoPluginEntryJs,
    RepoPluginPermissionJs, ResolvedProblemUrlJs, SyncReportJs, TrainingSubmissionsQueryJs,
    VerifyPluginResultJs,
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
    pub(crate) fn effective_data_dir(&self) -> Result<PathBuf, String> {
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

    pub(crate) fn set_training_status(&self, problem_id: &str, status: &str) -> ActionResult {
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

    pub(crate) fn resolve_problem_url(&self, url: &str) -> ResolvedProblemUrlJs {
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
    pub(crate) fn create_problem_from_url(
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

    pub(crate) fn get_plugins_overview(&self) -> PluginsOverviewJs {
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
                    capabilities: m
                        .capabilities
                        .iter()
                        .map(|c| capability_str(*c).to_owned())
                        .collect(),
                    domains: m.permissions.domains,
                    workspace_read: m.permissions.workspace_read,
                    has_unsandboxed_ui: m.ui_entry.is_some(),
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

    pub(crate) fn set_plugin_enabled(&self, id: &str, enabled: bool) -> ActionResult {
        let dir = match self.effective_data_dir() {
            Ok(d) => d,
            Err(err) => return ActionResult::err("data_dir", err),
        };
        let pm = PluginManager::new(dir);
        match pm.set_enabled(id, enabled) {
            Ok(()) => ActionResult::ok(),
            Err(e) => ActionResult::err("plugin_error", e.to_string()),
        }
    }

    pub(crate) fn export_builtin_plugin(&self, id: &str) -> ExportPluginResultJs {
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
            Ok(path) => ExportPluginResultJs {
                ok: true,
                path: Some(path.display().to_string()),
                error: None,
            },
            Err(e) => ExportPluginResultJs {
                ok: false,
                path: None,
                error: Some(e.to_string()),
            },
        }
    }

    pub(crate) fn install_plugin_from_path(
        &self,
        path: &Path,
        is_zip: bool,
    ) -> InstallPluginResultJs {
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
            Ok(p) => InstallPluginResultJs {
                ok: true,
                cancelled: false,
                plugin_id: Some(p.manifest.id),
                overview: Some(self.get_plugins_overview()),
                error: None,
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

    pub(crate) fn add_plugin_repo(&self, url: &str) -> ActionResult {
        let dir = match self.effective_data_dir() {
            Ok(d) => d,
            Err(err) => return ActionResult::err("data_dir", err),
        };
        let pm = PluginManager::new(dir);
        match pm.add_repo(url, None) {
            Ok(()) => ActionResult::ok(),
            Err(e) => ActionResult::err("plugin_error", e.to_string()),
        }
    }

    pub(crate) fn remove_plugin_repo(&self, url: &str) -> ActionResult {
        let dir = match self.effective_data_dir() {
            Ok(d) => d,
            Err(err) => return ActionResult::err("data_dir", err),
        };
        let pm = PluginManager::new(dir);
        match pm.remove_repo(url) {
            Ok(_) => ActionResult::ok(),
            Err(e) => ActionResult::err("plugin_error", e.to_string()),
        }
    }

    pub(crate) fn fetch_plugin_repo_catalog(&self, url: &str) -> RepoCatalogResultJs {
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
                        permissions: p.permissions.map(|perm| RepoPluginPermissionJs {
                            domains: perm.domains,
                            workspace_read: perm.workspace_read,
                            has_unsandboxed_ui: perm.has_unsandboxed_ui,
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

    pub(crate) fn install_plugin_from_repo(
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
        match pm.install_from_repo_entry(entry, &http) {
            Ok(installed) => InstallPluginResultJs {
                ok: true,
                cancelled: false,
                plugin_id: Some(installed.manifest.id),
                overview: Some(self.get_plugins_overview()),
                error: None,
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

    pub(crate) fn set_sync_handle(&self, platform: &str, handle: &str) -> ActionResult {
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

    pub(crate) fn set_auto_sync_on_startup(&self, enabled: bool) -> ActionResult {
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

    pub(crate) fn set_plugin_credential(
        &self,
        plugin_id: &str,
        key: &str,
        value: &str,
    ) -> ActionResult {
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

    pub(crate) fn verify_plugin(
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

    pub(crate) fn sync_oj(&self, platform: Option<String>, full: bool) -> SyncReportJs {
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
    pub(crate) fn startup_sync_if_enabled(&self) -> bool {
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

    pub(crate) fn training_dashboard(
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

    pub(crate) fn training_contests(
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

    pub(crate) fn training_submissions(
        &self,
        query: TrainingSubmissionsQueryJs,
    ) -> JsonReportResultJs {
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

    pub(crate) fn training_ingest_problem(
        &self,
        req: IngestProblemRequestJs,
    ) -> IngestProblemResultJs {
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

    pub(crate) fn training_log_record(&self, req_json: &str) -> ManualLogResultJs {
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

    pub(crate) fn training_list_editorials(&self, query_json: &str) -> EditorialListResultJs {
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

    // ---- Phase 4C: Toolbox, UI SDK, Commands, Importers & Exporters --------------------

    fn workspace_reader_if_allowed(
        &self,
        pm: &PluginManager,
        plugin_id: &str,
    ) -> Option<Arc<dyn WorkspaceReader>> {
        let loaded = pm.load_plugin(plugin_id).ok()?;
        if !loaded.info.manifest.permissions.workspace_read {
            return None;
        }
        let ws = {
            let core = self.core.lock().unwrap();
            core.workspace.clone()
        }?;
        Some(Arc::new(DesktopWorkspaceReader(ws)))
    }

    pub(crate) fn plugin_list_tools(&self) -> JsonReportResultJs {
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

    pub(crate) fn plugin_run_tool(
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
        let ws_reader = self.workspace_reader_if_allowed(&pm, plugin_id);
        match pm.run_tool(plugin_id, tool_id, inputs, ws_reader) {
            Ok(res) => match serde_json::to_string(&res) {
                Ok(json) => JsonReportResultJs::ok(Some(json), None),
                Err(err) => JsonReportResultJs::err("serialize", err.to_string()),
            },
            Err(err) => JsonReportResultJs::err("plugin_error", err.to_string()),
        }
    }

    pub(crate) fn plugin_ui_source(&self, plugin_id: &str) -> PluginUiSourceResultJs {
        let dir = match self.effective_data_dir() {
            Ok(d) => d,
            Err(err) => {
                return PluginUiSourceResultJs {
                    ok: false,
                    source: None,
                    error: Some(err),
                };
            }
        };
        let pm = PluginManager::new(&dir);
        match pm.read_ui_source(plugin_id) {
            Ok(source) => PluginUiSourceResultJs {
                ok: true,
                source,
                error: None,
            },
            Err(err) => PluginUiSourceResultJs {
                ok: false,
                source: None,
                error: Some(err.to_string()),
            },
        }
    }

    pub(crate) fn plugin_render_svg_png(svg: &str, scale: Option<f64>) -> PluginRenderPngResultJs {
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

    pub(crate) fn plugin_list_commands(&self, context: Option<String>) -> JsonReportResultJs {
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

    pub(crate) fn plugin_run_command(
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
        let ws_reader = self.workspace_reader_if_allowed(&pm, plugin_id);
        match pm.run_command(plugin_id, command_id, payload, ws_reader) {
            Ok(action) => match serde_json::to_string(&action) {
                Ok(json) => JsonReportResultJs::ok(Some(json), None),
                Err(err) => JsonReportResultJs::err("serialize", err.to_string()),
            },
            Err(err) => JsonReportResultJs::err("plugin_error", err.to_string()),
        }
    }

    pub(crate) fn plugin_list_importers(&self) -> JsonReportResultJs {
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

    pub(crate) fn plugin_importer_extensions(
        &self,
        plugin_id: &str,
        importer_id: &str,
    ) -> Vec<String> {
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

    pub(crate) fn plugin_run_importer_paths(
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
                {
                    let mut core = self.core.lock().unwrap();
                    if let Ok(rebuilt) = xcpc_storage::WorkspaceIndex::scan(&ws) {
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

    pub(crate) fn plugin_list_exporters(&self) -> JsonReportResultJs {
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

    pub(crate) fn plugin_run_exporter(
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

struct DesktopWorkspaceReader(Workspace);

impl WorkspaceReader for DesktopWorkspaceReader {
    fn list_problems(&self) -> xcpc_plugin::Result<Vec<WorkspaceProblemSummary>> {
        let ids = self
            .0
            .list_problems()
            .map_err(|e| xcpc_plugin::PluginError::InvalidPackage(e.to_string()))?;
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            if let Ok(p) = self.0.load_problem(&id) {
                out.push(WorkspaceProblemSummary {
                    id,
                    title: p.meta.title,
                    tags: p.meta.tags,
                    difficulty: p.meta.rating.map(|r| r.to_string()),
                });
            }
        }
        Ok(out)
    }

    fn get_problem(&self, id: &str) -> xcpc_plugin::Result<Option<WorkspaceProblemDetail>> {
        let problem = match self.0.load_problem(id) {
            Ok(p) => p,
            Err(xcpc_storage::Error::NotFound(_)) => return Ok(None),
            Err(e) => return Err(xcpc_plugin::PluginError::InvalidPackage(e.to_string())),
        };
        let lang = problem.default_language();
        let stmt_rel = problem
            .statement(lang)
            .map(|s| s.file.path.clone())
            .unwrap_or_else(|| format!("{}/statement.md", problem.dir));
        let statement_md = self.0.read_text(&stmt_rel).ok();
        let editorial_md = self
            .0
            .read_text(&format!("{}/editorial.md", problem.dir))
            .ok();

        Ok(Some(WorkspaceProblemDetail {
            id: id.to_owned(),
            title: problem.meta.title,
            tags: problem.meta.tags,
            difficulty: problem.meta.rating.map(|r| r.to_string()),
            statement_md,
            editorial_md,
        }))
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
pub(crate) fn atomic_write_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
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
