use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use xcpc_plugin::{
    CredentialStore, HostContext, HttpClient, OjContext, OjPluginClient, OjSample,
    PluginCapability, PluginManager, SandboxLimits, SystemKeyringStore, UreqHttpClient,
};

use crate::config::SyncConfig;
use crate::db::{CachedProblemMeta, SyncDb};
use crate::error::Result;
use crate::now_rfc3339_utc;

const CURSOR_KV_KEY: &str = "sync_cursor";

/// Options controlling a sync run.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncOptions {
    /// Sync only a single platform ID when `Some`; otherwise sync all enabled configured `oj_source` plugins.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    /// When `true`, ignore any stored incremental cursor and perform a full sync.
    #[serde(default)]
    pub full: bool,
}

/// Per-platform outcome in a [`SyncReport`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformSyncReport {
    pub platform: String,
    pub handle: String,
    pub ok: bool,
    pub account_synced: bool,
    pub summary_synced: bool,
    pub submissions_synced: usize,
    pub rating_history_synced: usize,
    pub contests_synced: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Aggregated result of a [`SyncEngine::sync`] invocation (serializable for CLI `--json` and GUI Toast).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncReport {
    pub synced_at: String,
    pub platforms: Vec<PlatformSyncReport>,
}

/// Result of resolving an external OJ problem URL via enabled `oj_source` plugins and `oj_sync.db`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedProblemUrl {
    pub platform: String,
    pub problem_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contest_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem_index: Option<String>,
    pub url: String,
    pub suggested_slug: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_rating: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normalized_rating: Option<i64>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub statement_md: Option<String>,
    #[serde(default)]
    pub samples: Vec<OjSample>,
}

/// Schedules enabled `oj_source` plugins and persists their output into `<dataDir>/oj_sync.db`.
pub struct SyncEngine {
    data_dir: PathBuf,
    http_client: Arc<dyn HttpClient>,
    credentials: Arc<dyn CredentialStore>,
}

impl SyncEngine {
    pub fn new(
        data_dir: impl Into<PathBuf>,
        http_client: Arc<dyn HttpClient>,
        credentials: Arc<dyn CredentialStore>,
    ) -> Self {
        Self {
            data_dir: data_dir.into(),
            http_client,
            credentials,
        }
    }

    pub fn default_for_data_dir(data_dir: impl Into<PathBuf>) -> Self {
        Self::new(
            data_dir,
            Arc::new(UreqHttpClient::new()),
            Arc::new(SystemKeyringStore::new()),
        )
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn plugin_manager(&self) -> PluginManager {
        PluginManager::new(&self.data_dir)
    }

    pub fn open_db(&self) -> Result<SyncDb> {
        SyncDb::open(&self.data_dir)
    }

    pub fn load_config(&self) -> Result<SyncConfig> {
        SyncConfig::load(&self.data_dir)
    }

    pub fn save_config(&self, config: &SyncConfig) -> Result<()> {
        config.save(&self.data_dir)
    }

    /// Runs incremental (or full) sync across enabled `oj_source` plugins with configured handles.
    ///
    /// Plugin errors are isolated per platform: a failing plugin records its error via
    /// [`PluginManager::record_error`] and is reported in [`SyncReport::platforms`] without
    /// aborting other platforms.
    pub fn sync(&self, options: &SyncOptions) -> Result<SyncReport> {
        let pm = self.plugin_manager();
        let mut config = SyncConfig::load(&self.data_dir)?;
        let mut db = SyncDb::open(&self.data_dir)?;
        let synced_at = now_rfc3339_utc();

        let plugins = pm.list_plugins()?;
        let mut reports = Vec::new();

        for plugin in plugins {
            if !plugin.enabled
                || !plugin
                    .manifest
                    .capabilities
                    .contains(&PluginCapability::OjSource)
            {
                continue;
            }
            let platform_id = plugin.manifest.id.clone();
            if let Some(ref target) = options.platform
                && !platform_id.eq_ignore_ascii_case(target.trim())
            {
                continue;
            }

            let Some(handle) = config.handle_for(&platform_id).map(str::to_owned) else {
                if options.platform.is_some() {
                    let msg = format!("no handle configured for platform '{platform_id}'");
                    reports.push(PlatformSyncReport {
                        platform: platform_id,
                        handle: String::new(),
                        ok: false,
                        account_synced: false,
                        summary_synced: false,
                        submissions_synced: 0,
                        rating_history_synced: 0,
                        contests_synced: 0,
                        error: Some(msg),
                    });
                }
                continue;
            };

            let outcome = self.sync_platform(
                &pm,
                &mut db,
                &platform_id,
                &handle,
                options.full,
                &synced_at,
            );

            match outcome {
                Ok(report) => {
                    let _ = pm.clear_error(&platform_id);
                    reports.push(report);
                }
                Err(err) => {
                    let msg = err.to_string();
                    let _ = pm.record_error(&platform_id, &msg);
                    reports.push(PlatformSyncReport {
                        platform: platform_id,
                        handle,
                        ok: false,
                        account_synced: false,
                        summary_synced: false,
                        submissions_synced: 0,
                        rating_history_synced: 0,
                        contests_synced: 0,
                        error: Some(msg),
                    });
                }
            }
        }

        config.last_synced_at = Some(synced_at.clone());
        config.save(&self.data_dir)?;

        Ok(SyncReport {
            synced_at,
            platforms: reports,
        })
    }

    fn sync_platform(
        &self,
        pm: &PluginManager,
        db: &mut SyncDb,
        platform_id: &str,
        handle: &str,
        full: bool,
        synced_at: &str,
    ) -> Result<PlatformSyncReport> {
        let loaded = pm.load_plugin(platform_id)?;
        let kv = Arc::new(pm.kv_store(platform_id)?);
        let host_ctx = HostContext {
            manifest: loaded.info.manifest.clone(),
            http_client: Arc::clone(&self.http_client),
            credentials: Arc::clone(&self.credentials),
            kv: Arc::clone(&kv),
            workspace: None,
            limits: SandboxLimits::default(),
        };
        let client = OjPluginClient::new(&loaded.entry_source, &host_ctx);
        let oj_ctx = OjContext {
            handle: handle.to_string(),
        };

        let cursor = if full {
            None
        } else {
            kv.get(CURSOR_KV_KEY)?
                .and_then(|v| v.as_str().map(str::to_owned))
                .filter(|s| !s.is_empty())
        };

        let mut account_synced = false;
        let mut rating_history_synced = 0;
        let mut summary_synced = false;
        let mut submissions_synced = 0;
        let mut contests_synced = 0;

        if client.supports("syncAccount")? {
            let account = client.sync_account(&oj_ctx)?;
            rating_history_synced = db.upsert_account(platform_id, &account, synced_at)?;
            account_synced = true;
        }

        if client.supports("syncSummary")? {
            let summary = client.sync_summary(&oj_ctx)?;
            db.upsert_summary(platform_id, &summary, synced_at)?;
            summary_synced = true;
        }

        if client.supports("syncSubmissions")? {
            let mut batch = client.sync_submissions(&oj_ctx, cursor.as_deref())?;
            let supports_normalize = client.supports("normalizeRating")?;
            if supports_normalize {
                for sub in &mut batch.submissions {
                    if sub.normalized_rating.is_none()
                        && let Some(raw) = sub.raw_rating
                    {
                        sub.normalized_rating = client.normalize_rating(raw)?;
                    }
                }
            }
            submissions_synced =
                db.upsert_submissions(platform_id, &batch.submissions, synced_at)?;
            if let Some(next_cursor) = batch.next_cursor.filter(|c| !c.trim().is_empty()) {
                kv.set(CURSOR_KV_KEY, serde_json::Value::String(next_cursor))?;
            }
        }

        if client.supports("fetchContests")? {
            let contests = client.fetch_contests(&oj_ctx)?;
            contests_synced = db.upsert_contests(platform_id, &contests, synced_at)?;
        }

        Ok(PlatformSyncReport {
            platform: platform_id.to_string(),
            handle: handle.to_string(),
            ok: true,
            account_synced,
            summary_synced,
            submissions_synced,
            rating_history_synced,
            contests_synced,
            error: None,
        })
    }

    /// Parses an external OJ problem URL using enabled `oj_source` plugins, enriches it with
    /// cached metadata from `oj_sync.db` (and optionally `fetchProblem` if title is missing),
    /// and suggests a clean valid problem slug.
    pub fn resolve_problem_url(&self, url: &str) -> Result<Option<ResolvedProblemUrl>> {
        let pm = self.plugin_manager();
        resolve_problem_url_with_manager(
            &pm,
            &self.data_dir,
            &self.http_client,
            &self.credentials,
            url,
        )
    }
}

pub(crate) fn resolve_problem_url_with_manager(
    pm: &PluginManager,
    data_dir: &Path,
    http_client: &Arc<dyn HttpClient>,
    credentials: &Arc<dyn CredentialStore>,
    url: &str,
) -> Result<Option<ResolvedProblemUrl>> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let config = SyncConfig::load(data_dir)?;
    let plugins = pm.list_plugins()?;

    for plugin in plugins {
        if !plugin.enabled
            || !plugin
                .manifest
                .capabilities
                .contains(&PluginCapability::OjSource)
        {
            continue;
        }
        let platform_id = plugin.manifest.id.clone();
        let loaded = pm.load_plugin(&platform_id)?;
        let kv = Arc::new(pm.kv_store(&platform_id)?);
        let host_ctx = HostContext {
            manifest: loaded.info.manifest.clone(),
            http_client: Arc::clone(http_client),
            credentials: Arc::clone(credentials),
            kv,
            workspace: None,
            limits: SandboxLimits::default(),
        };
        let client = OjPluginClient::new(&loaded.entry_source, &host_ctx);
        if !client.supports("parseProblemUrl")? {
            continue;
        }
        let Some(parsed) = client.parse_problem_url(trimmed)? else {
            continue;
        };

        let mut db = SyncDb::open(data_dir)?;
        let cached = db.lookup_problem_meta(&parsed.platform, &parsed.problem_id)?;

        let mut title = cached.as_ref().and_then(|m| m.title.clone());
        let mut raw_rating = cached.as_ref().and_then(|m| m.raw_rating);
        let mut normalized_rating = cached.as_ref().and_then(|m| m.normalized_rating);
        let mut tags = cached.as_ref().map(|m| m.tags.clone()).unwrap_or_default();
        let mut statement_md = None;
        let mut samples = Vec::new();

        if title.is_none() && client.supports("fetchProblem")? {
            let oj_ctx = OjContext {
                handle: config
                    .handle_for(&parsed.platform)
                    .unwrap_or("")
                    .to_string(),
            };
            if let Ok(Some(detail)) = client.fetch_problem(&oj_ctx, &parsed.problem_id) {
                if !detail.title.trim().is_empty() {
                    title = Some(detail.title);
                }
                if raw_rating.is_none() {
                    raw_rating = detail
                        .raw_rating
                        .filter(|v| v.is_finite())
                        .map(|v| v.round() as i64);
                }
                if normalized_rating.is_none() {
                    normalized_rating = detail.normalized_rating;
                }
                if normalized_rating.is_none()
                    && let Some(raw) = detail.raw_rating
                    && client.supports("normalizeRating")?
                {
                    normalized_rating = client.normalize_rating(raw).ok().flatten();
                }
                if tags.is_empty() && !detail.tags.is_empty() {
                    tags = detail.tags;
                }
                statement_md = detail.statement_md;
                samples = detail.samples;

                // Cache newly fetched metadata for subsequent lookups.
                let _ = db.upsert_problem_meta(&CachedProblemMeta {
                    platform: parsed.platform.clone(),
                    problem_key: parsed.problem_id.clone(),
                    title: title.clone(),
                    url: detail.url.or_else(|| Some(parsed.url.clone())),
                    raw_rating,
                    normalized_rating,
                    tags: tags.clone(),
                    updated_at: now_rfc3339_utc(),
                });
            }
        }

        let suggested_slug = suggest_problem_slug(&parsed.platform, &parsed.problem_id);

        return Ok(Some(ResolvedProblemUrl {
            platform: parsed.platform,
            problem_key: parsed.problem_id,
            contest_id: parsed.contest_id,
            problem_index: parsed.problem_index,
            url: parsed.url,
            suggested_slug,
            title,
            raw_rating,
            normalized_rating,
            tags,
            statement_md,
            samples,
        }));
    }

    Ok(None)
}

/// Suggests a clean problem directory slug (always satisfying [`xcpc_domain::check_id`]).
///
/// Examples:
/// - `("codeforces", "1999A")` -> `"cf-1999a"`
/// - `("atcoder", "abc350_a")` -> `"atcoder-abc350-a"`
pub fn suggest_problem_slug(platform: &str, problem_key: &str) -> String {
    let prefix_raw = match platform.trim().to_ascii_lowercase().as_str() {
        "codeforces" => "cf".to_string(),
        other => sanitize_slug_segment(other),
    };
    let key_clean = sanitize_slug_segment(problem_key);

    let combined = match (prefix_raw.is_empty(), key_clean.is_empty()) {
        (true, true) => "problem".to_string(),
        (true, false) => key_clean,
        (false, true) => prefix_raw,
        (false, false) => {
            if key_clean == prefix_raw || key_clean.starts_with(&format!("{prefix_raw}-")) {
                key_clean
            } else {
                format!("{prefix_raw}-{key_clean}")
            }
        }
    };

    let mut slug = combined;
    if !slug
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    {
        slug = format!("p-{slug}");
    }
    if slug.len() > 64 {
        slug.truncate(64);
        while slug.ends_with('-') || slug.ends_with('_') {
            slug.pop();
        }
    }
    if slug.is_empty() || xcpc_domain::check_id(&slug).is_err() {
        "problem".to_string()
    } else {
        slug
    }
}

fn sanitize_slug_segment(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut prev_dash = false;
    for ch in raw.trim().chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_lowercase() || lower.is_ascii_digit() {
            out.push(lower);
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}
