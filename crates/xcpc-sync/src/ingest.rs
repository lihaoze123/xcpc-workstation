use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use xcpc_domain::{
    LimitsPatch, Patch, ProblemMetaPatch, ProblemSource, TrainingLogEntry, TrainingLogStatus,
    check_id,
};
use xcpc_plugin::{CredentialStore, HttpClient, PluginManager, SystemKeyringStore, UreqHttpClient};
use xcpc_storage::{CreateProblemOptions, Workspace};

use crate::analytics::parse_rfc3339_to_unix_secs;
use crate::db::SyncDb;
use crate::error::{Result, SyncError};
use crate::now_rfc3339_utc;
use crate::scheduler::{
    resolve_problem_url_offline, resolve_problem_url_with_manager, suggest_problem_slug,
};

/// Request payload for [`ingest_problem`] (from a synced submission, external URL, or manual input).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngestProblemRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_status: Option<TrainingLogStatus>,
}

/// Outcome of [`ingest_problem`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngestProblemResult {
    pub problem_id: String,
    pub created: bool,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<ProblemSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<i64>,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Request payload for [`log_training_record`] (workspace check-in or standalone manual log).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManualLogRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<i32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    pub status: TrainingLogStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default)]
    pub create_in_library: bool,
}

impl Default for ManualLogRequest {
    fn default() -> Self {
        Self {
            problem_id: None,
            title: None,
            url: None,
            platform: None,
            rating: None,
            tags: Vec::new(),
            status: TrainingLogStatus::Ac,
            at: None,
            note: None,
            create_in_library: false,
        }
    }
}

/// Outcome of [`log_training_record`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManualLogResult {
    pub entry: TrainingLogEntry,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library_problem_id: Option<String>,
    pub created_in_library: bool,
}

/// Records a manual training entry into `training/logs.jsonl`, optionally enriching from
/// offline OJ URL parsing / local `oj_sync.db` cache and optionally creating a linked
/// problem in the workspace library (`create_in_library = true`).
pub fn log_training_record(
    workspace: &Workspace,
    data_dir: &Path,
    plugins: Option<&PluginManager>,
    req: &ManualLogRequest,
) -> Result<ManualLogResult> {
    if workspace.is_read_only() {
        return Err(xcpc_storage::Error::ReadOnly.into());
    }

    let req_problem_id = req
        .problem_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let req_title = req
        .title
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let req_url = req.url.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let req_platform = req
        .platform
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_ascii_lowercase);
    let req_note = req.note.as_deref().map(str::trim).filter(|s| !s.is_empty());

    let at = match req.at.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(raw) => {
            if parse_rfc3339_to_unix_secs(raw).is_none() {
                return Err(SyncError::InvalidInput(format!(
                    "invalid RFC3339 timestamp {raw:?}"
                )));
            }
            raw.to_string()
        }
        None => now_rfc3339_utc(),
    };

    // Deduplicate and clean explicit tags while preserving order.
    let mut cleaned_tags: Vec<String> = Vec::new();
    let mut seen_tags: HashSet<String> = HashSet::new();
    for raw_tag in &req.tags {
        let t = raw_tag.trim();
        if !t.is_empty() && seen_tags.insert(t.to_ascii_lowercase()) {
            cleaned_tags.push(t.to_string());
        }
    }

    // Offline-safe URL resolution (never blocks on network) when a URL is provided.
    let resolved = if let Some(url) = req_url {
        if let Some(pm) = plugins {
            resolve_problem_url_offline(pm, data_dir, url)
                .ok()
                .flatten()
        } else {
            let owned_pm = PluginManager::new(data_dir);
            resolve_problem_url_offline(&owned_pm, data_dir, url)
                .ok()
                .flatten()
        }
    } else {
        None
    };

    let mut source = resolved.as_ref().map(|r| ProblemSource {
        platform: r.platform.trim().to_ascii_lowercase(),
        id: r.problem_key.trim().to_string(),
        url: Some(r.url.clone()),
    });

    let mut final_platform = req_platform
        .or_else(|| {
            resolved
                .as_ref()
                .map(|r| r.platform.trim().to_ascii_lowercase())
        })
        .filter(|s| !s.is_empty());
    let final_url = req_url
        .map(str::to_owned)
        .or_else(|| resolved.as_ref().map(|r| r.url.clone()));
    let mut final_title = req_title
        .map(str::to_owned)
        .or_else(|| resolved.as_ref().and_then(|r| r.title.clone()));
    let final_rating = req.rating.or_else(|| {
        resolved
            .as_ref()
            .and_then(|r| r.normalized_rating.or(r.raw_rating))
            .and_then(|v| i32::try_from(v).ok())
    });
    let final_tags = if !cleaned_tags.is_empty() {
        cleaned_tags
    } else {
        resolved
            .as_ref()
            .map(|r| r.tags.clone())
            .unwrap_or_default()
    };

    let mut entry_problem_id: Option<String> = None;
    let mut library_problem_id: Option<String> = None;
    let mut created_in_library = false;

    if let Some(pid) = req_problem_id {
        let problem = workspace.load_problem(pid)?;
        if source.is_none() {
            source = problem.meta.sources.first().cloned();
        }
        if final_platform.is_none() {
            final_platform = source
                .as_ref()
                .map(|s| s.platform.trim().to_ascii_lowercase())
                .filter(|s| !s.is_empty());
        }
        entry_problem_id = Some(problem.id.clone());
        library_problem_id = Some(problem.id);
    } else if req.create_in_library {
        if source.is_none() && final_title.is_none() {
            return Err(SyncError::InvalidInput(
                "must provide `problem_id`, recognizable `url`, or `title` to create a library problem"
                    .into(),
            ));
        }
        let ingest_req = IngestProblemRequest {
            url: final_url.clone(),
            platform: source
                .as_ref()
                .map(|s| s.platform.clone())
                .or_else(|| final_platform.clone()),
            problem_key: source.as_ref().map(|s| s.id.clone()),
            id: None,
            title: final_title.clone(),
            tags: (!final_tags.is_empty()).then_some(final_tags.clone()),
            rating: final_rating.map(i64::from),
            initial_status: None,
        };
        let res = ingest_problem_inner(workspace, data_dir, plugins, &ingest_req, false)?;
        if source.is_none() {
            source = res.source;
        }
        if final_title.is_none() {
            final_title = Some(res.title);
        }
        entry_problem_id = Some(res.problem_id.clone());
        library_problem_id = Some(res.problem_id);
        created_in_library = res.created;
    } else if let Some(ref src) = source {
        library_problem_id = workspace.find_problem_by_source(&src.platform, &src.id)?;
    }

    let entry = TrainingLogEntry {
        at,
        problem_id: entry_problem_id,
        source,
        status: req.status,
        title: final_title,
        url: final_url,
        platform: final_platform,
        rating: final_rating,
        tags: final_tags,
        note: req_note.map(str::to_owned),
    };

    entry.validate().map_err(SyncError::InvalidInput)?;
    workspace.append_training_log(&entry)?;

    Ok(ManualLogResult {
        entry,
        library_problem_id,
        created_in_library,
    })
}

/// Ingests an external OJ problem (by URL or `(platform, problem_key)`) into `workspace`.
///
/// - Enforces workspace read-only boundaries (`xcpc_storage::Error::ReadOnly`).
/// - Resolves `req.url` via enabled `oj_source` plugins when provided.
/// - Enriches metadata (`title`, `url`, `normalized_rating`, `tags`) from `<data_dir>/oj_sync.db`.
/// - Deduplicates by `[[source]]`: if a matching problem already exists in `workspace`, returns
///   `created: false` (still appending `initial_status` to `training/logs.jsonl` if provided).
/// - Otherwise creates the problem atomically via [`Workspace::create_problem_with_options`],
///   never writing training status into `problem.toml`.
pub fn ingest_problem(
    workspace: &Workspace,
    data_dir: &Path,
    plugins: Option<&PluginManager>,
    req: &IngestProblemRequest,
) -> Result<IngestProblemResult> {
    ingest_problem_inner(workspace, data_dir, plugins, req, true)
}

fn ingest_problem_inner(
    workspace: &Workspace,
    data_dir: &Path,
    plugins: Option<&PluginManager>,
    req: &IngestProblemRequest,
    allow_network: bool,
) -> Result<IngestProblemResult> {
    if workspace.is_read_only() {
        return Err(xcpc_storage::Error::ReadOnly.into());
    }

    let req_url = req.url.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let req_platform = req
        .platform
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let req_key = req
        .problem_key
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let req_title = req
        .title
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let req_id = req.id.as_deref().map(str::trim).filter(|s| !s.is_empty());

    // 1. Resolve URL via enabled `oj_source` plugins if provided.
    let resolved = if let Some(url) = req_url {
        if allow_network {
            let http: Arc<dyn HttpClient> = Arc::new(UreqHttpClient::new());
            let creds: Arc<dyn CredentialStore> = Arc::new(SystemKeyringStore::new());
            if let Some(pm) = plugins {
                resolve_problem_url_with_manager(pm, data_dir, &http, &creds, url)?
            } else {
                let owned_pm = PluginManager::new(data_dir);
                resolve_problem_url_with_manager(&owned_pm, data_dir, &http, &creds, url)?
            }
        } else if let Some(pm) = plugins {
            resolve_problem_url_offline(pm, data_dir, url)?
        } else {
            let owned_pm = PluginManager::new(data_dir);
            resolve_problem_url_offline(&owned_pm, data_dir, url)?
        }
    } else {
        None
    };

    let platform = resolved
        .as_ref()
        .map(|r| r.platform.trim().to_ascii_lowercase())
        .or_else(|| req_platform.map(str::to_ascii_lowercase));
    let problem_key = resolved
        .as_ref()
        .map(|r| r.problem_key.trim().to_string())
        .or_else(|| req_key.map(str::to_owned));

    // 2. Enrich from `<data_dir>/oj_sync.db` (`oj_problem_meta` + `oj_submissions` fallback).
    let cached = if let (Some(p), Some(k)) = (platform.as_deref(), problem_key.as_deref())
        && SyncDb::db_path(data_dir).exists()
    {
        let db = SyncDb::open(data_dir)?;
        db.lookup_problem_meta(p, k)?
    } else {
        None
    };

    let final_url = resolved
        .as_ref()
        .map(|r| r.url.clone())
        .or_else(|| req_url.map(str::to_owned))
        .or_else(|| cached.as_ref().and_then(|c| c.url.clone()));

    let source = match (platform.as_deref(), problem_key.as_deref()) {
        (Some(p), Some(k)) if !p.is_empty() && !k.is_empty() => Some(ProblemSource {
            platform: p.to_string(),
            id: k.to_string(),
            url: final_url,
        }),
        _ => None,
    };

    if source.is_none() && req_title.is_none() && req_id.is_none() {
        return Err(SyncError::InvalidInput(
            "must provide a recognizable problem URL, (platform, problem_key), or title/id".into(),
        ));
    }

    // 3. Deduplicate against existing workspace problems by `[[source]]`.
    if let Some(ref src) = source
        && let Some(existing_id) = workspace.find_problem_by_source(&src.platform, &src.id)?
    {
        if let Some(status) = req.initial_status {
            workspace.append_training_log(&TrainingLogEntry::for_problem(
                now_rfc3339_utc(),
                Some(existing_id.clone()),
                Some(src.clone()),
                status,
            ))?;
        }
        let existing = workspace.load_problem(&existing_id)?;
        let existing_source = existing
            .meta
            .sources
            .iter()
            .find(|s| s.matches(&src.platform, &src.id))
            .cloned()
            .or_else(|| Some(src.clone()));
        return Ok(IngestProblemResult {
            problem_id: existing.id,
            created: false,
            title: existing.meta.title,
            source: existing_source,
            rating: existing.meta.rating.map(i64::from),
            tags: existing.meta.tags,
        });
    }

    // 4. Merge metadata (explicit request > URL resolution > oj_sync.db cache).
    let rating = req
        .rating
        .or_else(|| resolved.as_ref().and_then(|r| r.normalized_rating))
        .or_else(|| cached.as_ref().and_then(|c| c.normalized_rating))
        .or_else(|| resolved.as_ref().and_then(|r| r.raw_rating))
        .or_else(|| cached.as_ref().and_then(|c| c.raw_rating));

    let tags = if let Some(ref explicit_tags) = req.tags {
        explicit_tags.clone()
    } else if let Some(ref r) = resolved
        && !r.tags.is_empty()
    {
        r.tags.clone()
    } else if let Some(ref c) = cached {
        c.tags.clone()
    } else {
        Vec::new()
    };

    // 5. Choose a valid, non-colliding slug.
    let slug = if let Some(explicit_id) = req_id {
        check_id(explicit_id).map_err(|e| {
            SyncError::InvalidInput(format!("invalid problem id {explicit_id:?}: {e}"))
        })?;
        explicit_id.to_string()
    } else {
        let base = if let Some(ref src) = source {
            suggest_problem_slug(&src.platform, &src.id)
        } else {
            suggest_problem_slug("", req_title.unwrap_or("problem"))
        };
        allocate_unique_slug(workspace, &base)?
    };

    let title = req_title
        .map(str::to_owned)
        .or_else(|| resolved.as_ref().and_then(|r| r.title.clone()))
        .or_else(|| cached.as_ref().and_then(|c| c.title.clone()))
        .or_else(|| problem_key.clone())
        .unwrap_or_else(|| slug.clone());

    let options = CreateProblemOptions {
        sources: source.clone().into_iter().collect(),
        tags,
        difficulty: None,
        rating: rating.and_then(|r| u32::try_from(r).ok()),
    };

    // 6. Atomically create the problem (never writing training status into `problem.toml`).
    // Clear the placeholder default limits (`1s` / `256MB`) written by `create_problem_with_options`
    // because OJ submission metadata does not provide true time/memory limits.
    let created = workspace.create_problem_with_options(&slug, &title, &options)?;
    workspace.update_problem_meta(
        &created.id,
        &ProblemMetaPatch {
            limits: LimitsPatch {
                time: Patch::Clear,
                memory: Patch::Clear,
                ..Default::default()
            },
            ..Default::default()
        },
    )?;

    if let Some(status) = req.initial_status {
        workspace.append_training_log(&TrainingLogEntry::for_problem(
            now_rfc3339_utc(),
            Some(created.id.clone()),
            source.clone(),
            status,
        ))?;
    }

    Ok(IngestProblemResult {
        problem_id: created.id,
        created: true,
        title: created.meta.title,
        source,
        rating: created.meta.rating.map(i64::from).or(rating),
        tags: created.meta.tags,
    })
}

fn allocate_unique_slug(workspace: &Workspace, base: &str) -> Result<String> {
    let existing: HashSet<String> = workspace.list_problems()?.into_iter().collect();
    if !existing.contains(base) {
        return Ok(base.to_string());
    }
    for suffix_num in 2u32..=10_000 {
        let suffix = format!("-{suffix_num}");
        let max_base_len = 64usize.saturating_sub(suffix.len());
        let mut prefix = base.chars().take(max_base_len).collect::<String>();
        while prefix.ends_with('-') || prefix.ends_with('_') {
            prefix.pop();
        }
        let candidate = if prefix.is_empty() {
            format!("p{suffix}")
        } else {
            format!("{prefix}{suffix}")
        };
        if check_id(&candidate).is_ok() && !existing.contains(&candidate) {
            return Ok(candidate);
        }
    }
    Err(SyncError::InvalidInput(format!(
        "could not allocate unique slug derived from {base:?}"
    )))
}
