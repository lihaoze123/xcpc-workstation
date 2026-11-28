use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use xcpc_domain::{LimitsPatch, Patch, ProblemMetaPatch, SnippetMeta, SnippetMetaPatch, check_id};
use xcpc_storage::{CreateProblemOptions, Error as StorageError, Workspace};

use crate::error::{PluginError, Result};

/// Maximum number of samples accepted by `workspace.setSamples`.
pub const MAX_PLUGIN_SAMPLES: usize = 64;
/// Maximum byte length of a single sample `.in` or `.ans` file accepted by `workspace.setSamples` (2 MiB).
pub const MAX_PLUGIN_SAMPLE_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceSample {
    pub input: String,
    pub output: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceProblemSource {
    pub platform: String,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceProblemSummary {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub difficulty: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_limit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_limit: Option<String>,
    #[serde(default)]
    pub sample_count: usize,
    #[serde(default)]
    pub has_editorial: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceProblemDetail {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub difficulty: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_limit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_limit: Option<String>,
    #[serde(default)]
    pub source: Vec<WorkspaceProblemSource>,
    #[serde(default)]
    pub samples: Vec<WorkspaceSample>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub statement_md: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editorial_md: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceContestItem {
    pub label: String,
    pub id: String,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceContestSummary {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub problem_count: usize,
    #[serde(default)]
    pub frozen: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceContestDetail {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub frozen: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    #[serde(default)]
    pub problem_count: usize,
    #[serde(default)]
    pub problems: Vec<WorkspaceContestItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceSnippetSummary {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default = "default_cpp_lang")]
    pub language: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceSnippetDetail {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default = "default_cpp_lang")]
    pub language: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_complexity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub space_complexity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes_md: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceTrdSection {
    pub title: String,
    #[serde(default)]
    pub items: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceTrdSummary {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    #[serde(default)]
    pub section_count: usize,
    #[serde(default)]
    pub frozen: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceTrdDetail {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    #[serde(default)]
    pub section_count: usize,
    #[serde(default)]
    pub frozen: bool,
    #[serde(default)]
    pub sections: Vec<WorkspaceTrdSection>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateProblemInput {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub difficulty: Option<String>,
    #[serde(default)]
    pub rating: Option<u32>,
    #[serde(default, alias = "timeLimit")]
    pub time_limit: Option<String>,
    #[serde(default, alias = "memoryLimit")]
    pub memory_limit: Option<String>,
    #[serde(default, alias = "statementMd")]
    pub statement_md: Option<String>,
    #[serde(default, alias = "editorialMd")]
    pub editorial_md: Option<String>,
    #[serde(default)]
    pub samples: Vec<WorkspaceSample>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateProblemPatch {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub difficulty: Option<String>,
    #[serde(default)]
    pub rating: Option<u32>,
    #[serde(default, alias = "timeLimit")]
    pub time_limit: Option<String>,
    #[serde(default, alias = "memoryLimit")]
    pub memory_limit: Option<String>,
    #[serde(default, alias = "statementMd")]
    pub statement_md: Option<String>,
    #[serde(default, alias = "editorialMd")]
    pub editorial_md: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetSamplesResult {
    pub count: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpsertSnippetInput {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default, alias = "notes_md", alias = "notesMd")]
    pub notes: Option<String>,
    #[serde(default, alias = "timeComplexity")]
    pub time_complexity: Option<String>,
    #[serde(default, alias = "spaceComplexity")]
    pub space_complexity: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

fn default_cpp_lang() -> String {
    "cpp".to_string()
}

/// Strictly read-only workspace projection exposed to plugins with `workspace_read = true`.
/// Never exposes host filesystem paths or destructive mutation methods.
pub trait WorkspaceReader: Send + Sync {
    fn list_problems(&self) -> Result<Vec<WorkspaceProblemSummary>>;
    fn get_problem(&self, id: &str) -> Result<Option<WorkspaceProblemDetail>>;

    fn list_contests(&self) -> Result<Vec<WorkspaceContestSummary>> {
        Ok(Vec::new())
    }

    fn get_contest(&self, _id: &str) -> Result<Option<WorkspaceContestDetail>> {
        Ok(None)
    }

    fn list_snippets(&self) -> Result<Vec<WorkspaceSnippetSummary>> {
        Ok(Vec::new())
    }

    fn get_snippet(&self, _id: &str) -> Result<Option<WorkspaceSnippetDetail>> {
        Ok(None)
    }

    fn list_trds(&self) -> Result<Vec<WorkspaceTrdSummary>> {
        Ok(Vec::new())
    }

    fn get_trd(&self, _id: &str) -> Result<Option<WorkspaceTrdDetail>> {
        Ok(None)
    }
}

/// Domain-scoped workspace write capability exposed only to trusted plugins declaring
/// `permissions.workspace_write = true`. Never permits deleting problems/snippets,
/// modifying frozen contest snapshots, or unfreezing contests.
pub trait WorkspaceWriter: Send + Sync {
    fn create_problem(&self, input: CreateProblemInput) -> Result<WorkspaceProblemDetail>;
    fn update_problem(&self, id: &str, patch: UpdateProblemPatch)
    -> Result<WorkspaceProblemDetail>;
    fn set_samples(
        &self,
        problem_id: &str,
        samples: Vec<WorkspaceSample>,
    ) -> Result<SetSamplesResult>;
    fn upsert_snippet(&self, input: UpsertSnippetInput) -> Result<WorkspaceSnippetDetail>;
}

/// Core implementation of [`WorkspaceReader`] and [`WorkspaceWriter`] backed directly by
/// [`xcpc_storage::Workspace`]. Shared across QuickJS, WebView, CLI, and tests.
pub struct StorageWorkspaceBridge {
    workspace: Workspace,
}

impl StorageWorkspaceBridge {
    pub fn new(workspace: Workspace) -> Self {
        Self { workspace }
    }

    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }
}

fn map_storage_err(err: StorageError) -> PluginError {
    match err {
        StorageError::ReadOnly => {
            PluginError::WorkspaceReadOnly("workspace is open read-only".into())
        }
        StorageError::Frozen(id) | StorageError::TrdFrozen(id) => {
            PluginError::ContestFrozen(format!("'{id}' is frozen and read-only"))
        }
        StorageError::Conflict { path, .. } => {
            PluginError::AlreadyExists(format!("'{path}' already exists"))
        }
        StorageError::SnippetAlreadyExists(slug) => {
            PluginError::AlreadyExists(format!("snippet '{slug}' already exists"))
        }
        StorageError::InvalidId(e) => PluginError::InvalidArgument(e.to_string()),
        StorageError::Invalid { message, .. } => PluginError::InvalidArgument(message),
        other => PluginError::InvalidPackage(other.to_string()),
    }
}

fn validate_samples_limits(samples: &[WorkspaceSample]) -> Result<()> {
    if samples.len() > MAX_PLUGIN_SAMPLES {
        return Err(PluginError::InvalidArgument(format!(
            "too many samples ({}, maximum is {MAX_PLUGIN_SAMPLES})",
            samples.len()
        )));
    }
    for (idx, s) in samples.iter().enumerate() {
        if s.input.len() > MAX_PLUGIN_SAMPLE_BYTES {
            return Err(PluginError::InvalidArgument(format!(
                "sample {} input exceeds 2 MiB limit",
                idx + 1
            )));
        }
        if s.output.len() > MAX_PLUGIN_SAMPLE_BYTES {
            return Err(PluginError::InvalidArgument(format!(
                "sample {} output exceeds 2 MiB limit",
                idx + 1
            )));
        }
    }
    Ok(())
}

fn project_problem_detail(ws: &Workspace, id: &str) -> Result<Option<WorkspaceProblemDetail>> {
    let problem = match ws.load_problem(id) {
        Ok(p) => p,
        Err(StorageError::NotFound(_)) => return Ok(None),
        Err(e) => return Err(map_storage_err(e)),
    };
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
        .map(|s| WorkspaceSample {
            input: s.input.clone(),
            output: s.output.clone(),
        })
        .collect();
    let source = problem
        .meta
        .sources
        .iter()
        .map(|s| WorkspaceProblemSource {
            platform: s.platform.clone(),
            id: s.id.clone(),
            url: s.url.clone(),
        })
        .collect();
    let difficulty = problem.meta.rating.map(|r| r.to_string());
    Ok(Some(WorkspaceProblemDetail {
        id: problem.id,
        title: problem.meta.title,
        tags: problem.meta.tags,
        difficulty,
        rating: problem.meta.rating,
        time_limit: problem.meta.limits.time,
        memory_limit: problem.meta.limits.memory,
        source,
        samples,
        statement_md,
        editorial_md,
    }))
}

fn project_snippet_detail(ws: &Workspace, id: &str) -> Result<Option<WorkspaceSnippetDetail>> {
    let detail = match ws.load_snippet(id) {
        Ok(s) => s,
        Err(StorageError::NotFound(_)) => return Ok(None),
        Err(e) => return Err(map_storage_err(e)),
    };
    let category = detail.meta.tags.first().cloned();
    Ok(Some(WorkspaceSnippetDetail {
        id: detail.slug,
        title: detail.meta.title,
        category,
        tags: detail.meta.tags,
        language: "cpp".into(),
        code: detail.code.map(|c| c.text),
        time_complexity: detail.meta.time_complexity,
        space_complexity: detail.meta.space_complexity,
        description: detail.meta.description,
        notes_md: detail.notes.map(|n| n.text),
    }))
}

impl WorkspaceReader for StorageWorkspaceBridge {
    fn list_problems(&self) -> Result<Vec<WorkspaceProblemSummary>> {
        let ids = self.workspace.list_problems().map_err(map_storage_err)?;
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            if let Ok(p) = self.workspace.load_problem(&id) {
                let difficulty = p.meta.rating.map(|r| r.to_string());
                out.push(WorkspaceProblemSummary {
                    id,
                    title: p.meta.title,
                    tags: p.meta.tags,
                    difficulty,
                    rating: p.meta.rating,
                    time_limit: p.meta.limits.time,
                    memory_limit: p.meta.limits.memory,
                    sample_count: p.samples.len(),
                    has_editorial: p.editorial.is_some(),
                });
            }
        }
        Ok(out)
    }

    fn get_problem(&self, id: &str) -> Result<Option<WorkspaceProblemDetail>> {
        project_problem_detail(&self.workspace, id)
    }

    fn list_contests(&self) -> Result<Vec<WorkspaceContestSummary>> {
        let ids = self.workspace.list_contests().map_err(map_storage_err)?;
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            if let Ok(c) = self.workspace.load_contest(&id) {
                out.push(WorkspaceContestSummary {
                    id,
                    title: c.meta.name,
                    subtitle: c.meta.location.or(c.meta.date),
                    problem_count: c.meta.problems.len(),
                    frozen: c.meta.frozen,
                    template: c.meta.template,
                });
            }
        }
        Ok(out)
    }

    fn get_contest(&self, id: &str) -> Result<Option<WorkspaceContestDetail>> {
        let contest = match self.workspace.load_contest(id) {
            Ok(c) => c,
            Err(StorageError::NotFound(_)) => return Ok(None),
            Err(e) => return Err(map_storage_err(e)),
        };
        let mut problems = Vec::with_capacity(contest.meta.problems.len());
        for item in &contest.meta.problems {
            let title = item
                .title
                .clone()
                .or_else(|| {
                    self.workspace
                        .load_problem_meta(&item.id)
                        .ok()
                        .map(|m| m.title)
                })
                .unwrap_or_else(|| item.id.clone());
            problems.push(WorkspaceContestItem {
                label: item.label.clone(),
                id: item.id.clone(),
                title,
            });
        }
        let subtitle = contest
            .meta
            .location
            .clone()
            .or_else(|| contest.meta.date.clone());
        Ok(Some(WorkspaceContestDetail {
            id: contest.id,
            title: contest.meta.name,
            subtitle,
            location: contest.meta.location,
            date: contest.meta.date,
            author: contest.meta.author,
            language: contest.meta.language.name().to_owned(),
            frozen: contest.meta.frozen,
            template: contest.meta.template,
            problem_count: problems.len(),
            problems,
        }))
    }

    fn list_snippets(&self) -> Result<Vec<WorkspaceSnippetSummary>> {
        let summaries = self
            .workspace
            .list_snippet_summaries()
            .map_err(map_storage_err)?;
        Ok(summaries
            .into_iter()
            .map(|s| {
                let category = s.tags.first().cloned();
                WorkspaceSnippetSummary {
                    id: s.slug,
                    title: s.title,
                    category,
                    tags: s.tags,
                    language: "cpp".into(),
                }
            })
            .collect())
    }

    fn get_snippet(&self, id: &str) -> Result<Option<WorkspaceSnippetDetail>> {
        project_snippet_detail(&self.workspace, id)
    }

    fn list_trds(&self) -> Result<Vec<WorkspaceTrdSummary>> {
        let summaries = self
            .workspace
            .list_trd_summaries()
            .map_err(map_storage_err)?;
        Ok(summaries
            .into_iter()
            .map(|t| WorkspaceTrdSummary {
                id: t.id,
                title: t.name,
                template: t.template,
                section_count: t.chapter_count,
                frozen: t.frozen,
            })
            .collect())
    }

    fn get_trd(&self, id: &str) -> Result<Option<WorkspaceTrdDetail>> {
        let trd = match self.workspace.load_trd(id) {
            Ok(t) => t,
            Err(StorageError::NotFound(_)) => return Ok(None),
            Err(e) => return Err(map_storage_err(e)),
        };
        let sections: Vec<WorkspaceTrdSection> = trd
            .meta
            .chapters
            .into_iter()
            .map(|ch| WorkspaceTrdSection {
                title: ch.title,
                items: ch.items,
            })
            .collect();
        Ok(Some(WorkspaceTrdDetail {
            id: trd.id,
            title: trd.meta.name,
            author: trd.meta.author,
            date: trd.meta.date,
            template: trd.meta.template,
            section_count: sections.len(),
            frozen: trd.meta.frozen,
            sections,
        }))
    }
}

impl WorkspaceWriter for StorageWorkspaceBridge {
    fn create_problem(&self, input: CreateProblemInput) -> Result<WorkspaceProblemDetail> {
        if self.workspace.is_read_only() {
            return Err(PluginError::WorkspaceReadOnly(
                "workspace is open read-only".into(),
            ));
        }
        let id = input.id.trim().to_owned();
        check_id(&id).map_err(|e| PluginError::InvalidArgument(e.to_string()))?;
        validate_samples_limits(&input.samples)?;
        if self
            .workspace
            .resolve(&format!("problems/{id}/problem.toml"))
            .is_ok_and(|p| p.exists())
        {
            return Err(PluginError::AlreadyExists(format!(
                "problem '{id}' already exists"
            )));
        }

        let title = if input.title.trim().is_empty() {
            id.clone()
        } else {
            input.title.trim().to_owned()
        };
        let opts = CreateProblemOptions {
            sources: Vec::new(),
            tags: input.tags,
            rating: input.rating,
            difficulty: input.difficulty,
        };
        self.workspace
            .create_problem_with_options(&id, &title, &opts)
            .map_err(map_storage_err)?;

        if input.time_limit.is_some() || input.memory_limit.is_some() {
            let patch = ProblemMetaPatch {
                limits: LimitsPatch {
                    time: match input.time_limit {
                        Some(v) => Patch::Set(v),
                        None => Patch::Keep,
                    },
                    memory: match input.memory_limit {
                        Some(v) => Patch::Set(v),
                        None => Patch::Keep,
                    },
                    ..Default::default()
                },
                ..Default::default()
            };
            self.workspace
                .update_problem_meta(&id, &patch)
                .map_err(map_storage_err)?;
        }

        if let Some(stmt) = input.statement_md {
            self.workspace
                .write(&format!("problems/{id}/statement.md"), stmt.as_bytes())
                .map_err(map_storage_err)?;
        }
        if let Some(ed) = input.editorial_md {
            self.workspace
                .write(&format!("problems/{id}/editorial.md"), ed.as_bytes())
                .map_err(map_storage_err)?;
        }
        if !input.samples.is_empty() {
            let pairs: Vec<(String, String)> = input
                .samples
                .into_iter()
                .map(|s| (s.input, s.output))
                .collect();
            self.workspace
                .save_samples(&id, &pairs)
                .map_err(map_storage_err)?;
        }

        project_problem_detail(&self.workspace, &id)?.ok_or_else(|| {
            PluginError::InvalidPackage(format!("failed to load newly created problem '{id}'"))
        })
    }

    fn update_problem(
        &self,
        id: &str,
        patch: UpdateProblemPatch,
    ) -> Result<WorkspaceProblemDetail> {
        if self.workspace.is_read_only() {
            return Err(PluginError::WorkspaceReadOnly(
                "workspace is open read-only".into(),
            ));
        }
        let id = id.trim();
        check_id(id).map_err(|e| PluginError::InvalidArgument(e.to_string()))?;
        let existing = self.workspace.load_problem(id).map_err(map_storage_err)?;

        let rating_patch = match (patch.rating, patch.difficulty) {
            (Some(r), _) => Patch::Set(r),
            (None, Some(v)) if v.trim().is_empty() => Patch::Clear,
            (None, Some(v)) => match v.trim().parse::<u32>() {
                Ok(r) => Patch::Set(r),
                Err(_) => Patch::Keep,
            },
            (None, None) => Patch::Keep,
        };
        let meta_patch = ProblemMetaPatch {
            title: patch.title,
            tags: patch.tags,
            rating: rating_patch,
            limits: LimitsPatch {
                time: match patch.time_limit {
                    Some(v) if !v.trim().is_empty() => Patch::Set(v),
                    Some(_) => Patch::Clear,
                    None => Patch::Keep,
                },
                memory: match patch.memory_limit {
                    Some(v) if !v.trim().is_empty() => Patch::Set(v),
                    Some(_) => Patch::Clear,
                    None => Patch::Keep,
                },
                ..Default::default()
            },
            ..Default::default()
        };
        self.workspace
            .update_problem_meta(id, &meta_patch)
            .map_err(map_storage_err)?;

        if let Some(stmt) = patch.statement_md {
            let lang = existing.default_language();
            let stmt_rel = existing
                .statement(lang)
                .map(|s| s.file.path.clone())
                .unwrap_or_else(|| format!("problems/{id}/statement.md"));
            self.workspace
                .write(&stmt_rel, stmt.as_bytes())
                .map_err(map_storage_err)?;
        }
        if let Some(ed) = patch.editorial_md {
            self.workspace
                .write(&format!("problems/{id}/editorial.md"), ed.as_bytes())
                .map_err(map_storage_err)?;
        }

        project_problem_detail(&self.workspace, id)?.ok_or_else(|| {
            PluginError::InvalidPackage(format!("failed to load updated problem '{id}'"))
        })
    }

    fn set_samples(
        &self,
        problem_id: &str,
        samples: Vec<WorkspaceSample>,
    ) -> Result<SetSamplesResult> {
        if self.workspace.is_read_only() {
            return Err(PluginError::WorkspaceReadOnly(
                "workspace is open read-only".into(),
            ));
        }
        let problem_id = problem_id.trim();
        check_id(problem_id).map_err(|e| PluginError::InvalidArgument(e.to_string()))?;
        validate_samples_limits(&samples)?;
        // Verify problem exists before replacing samples.
        let _ = self
            .workspace
            .load_problem_meta(problem_id)
            .map_err(map_storage_err)?;
        let count = samples.len();
        let pairs: Vec<(String, String)> =
            samples.into_iter().map(|s| (s.input, s.output)).collect();
        self.workspace
            .save_samples(problem_id, &pairs)
            .map_err(map_storage_err)?;
        Ok(SetSamplesResult { count })
    }

    fn upsert_snippet(&self, input: UpsertSnippetInput) -> Result<WorkspaceSnippetDetail> {
        if self.workspace.is_read_only() {
            return Err(PluginError::WorkspaceReadOnly(
                "workspace is open read-only".into(),
            ));
        }
        let slug = input.id.trim().to_owned();
        check_id(&slug).map_err(|e| PluginError::InvalidArgument(e.to_string()))?;

        let mut tags = input.tags.unwrap_or_default();
        if let Some(cat) = input
            .category
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            && !tags.iter().any(|t| t.eq_ignore_ascii_case(cat))
        {
            tags.insert(0, cat.to_owned());
        }

        let exists = self
            .workspace
            .resolve(&format!("snippets/{slug}/snippet.toml"))
            .is_ok_and(|p| p.exists());

        if exists {
            let patch = SnippetMetaPatch {
                title: (!input.title.trim().is_empty()).then(|| input.title.trim().to_owned()),
                tags: (!tags.is_empty()).then_some(tags),
                time_complexity: match input.time_complexity {
                    Some(v) => Patch::Set(v),
                    None => Patch::Keep,
                },
                space_complexity: match input.space_complexity {
                    Some(v) => Patch::Set(v),
                    None => Patch::Keep,
                },
                description: match input.description {
                    Some(v) => Patch::Set(v),
                    None => Patch::Keep,
                },
                ..Default::default()
            };
            self.workspace
                .update_snippet_meta(&slug, &patch)
                .map_err(map_storage_err)?;
            let mut files = BTreeMap::new();
            if let Some(code) = input.code {
                files.insert("code.hpp".to_owned(), code);
            }
            if let Some(notes) = input.notes {
                files.insert("notes.md".to_owned(), notes);
            }
            if !files.is_empty() {
                self.workspace
                    .write_snippet_files(&slug, &files)
                    .map_err(map_storage_err)?;
            }
        } else {
            let meta = SnippetMeta {
                title: if input.title.trim().is_empty() {
                    slug.clone()
                } else {
                    input.title.trim().to_owned()
                },
                tags,
                time_complexity: input.time_complexity,
                space_complexity: input.space_complexity,
                description: input.description,
                ..Default::default()
            };
            self.workspace
                .create_snippet(&slug, &meta, input.code.as_deref(), input.notes.as_deref())
                .map_err(map_storage_err)?;
        }

        project_snippet_detail(&self.workspace, &slug)?.ok_or_else(|| {
            PluginError::InvalidPackage(format!("failed to load upserted snippet '{slug}'"))
        })
    }
}
