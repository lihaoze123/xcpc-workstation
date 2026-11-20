//! Shared read-only view models over Core (`xcpc-storage`, `xcpc-convert`, `xcpc-domain`)
//! used by both `xcpc show` (CLI) and `get_problem` / `get_contest` (MCP).

use std::collections::HashMap;

use serde::Serialize;
use xcpc_convert::publish::{self, Kind, Target};
use xcpc_domain::{ContestStats, Language, Limits, ProblemMeta, Sample, SampleInventory};
use xcpc_engine::Engine;
use xcpc_storage::{Error, Workspace, WorkspaceIndex, content_hash};

/// One statement file (`statement.md`, `statement.zh.md`, `statement.en.md`) with its SHA-256 digest.
#[derive(Debug, Serialize)]
pub struct StatementView {
    /// Workspace-relative path (e.g. `problems/a-plus-b/statement.md`).
    pub path: String,
    /// Problem-relative filename (e.g. `statement.md`).
    pub file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<Language>,
    pub text: String,
    pub hash: String,
}

/// A text file (`editorial.md`) with its SHA-256 digest.
#[derive(Debug, Serialize)]
pub struct TextFileView {
    /// Workspace-relative path (e.g. `problems/a-plus-b/editorial.md`).
    pub path: String,
    /// Problem-relative filename (e.g. `editorial.md`).
    pub file: String,
    pub text: String,
    pub hash: String,
}

/// Full structured view of one problem (`xcpc show problem` / MCP `get_problem`).
#[derive(Debug, Serialize)]
pub struct ProblemDetail {
    pub ok: bool,
    pub id: String,
    pub dir: String,
    pub meta: ProblemMeta,
    pub meta_hash: String,
    pub statements: Vec<StatementView>,
    pub editorial: Option<TextFileView>,
    pub samples: Vec<Sample>,
    pub sample_inventory: SampleInventory,
    pub solutions: Vec<String>,
}

/// Summary of one contest when listing all contests (`get_contest` without `id`).
#[derive(Debug, Serialize)]
pub struct ContestSummary {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    pub language: Language,
    pub frozen: bool,
    pub problem_count: usize,
}

/// Active metadata overrides for one problem inside a contest.
#[derive(Debug, Default, Serialize)]
pub struct ContestItemOverrides {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limits: Option<Limits>,
    #[serde(rename = "show-metadata", skip_serializing_if = "Option::is_none")]
    pub show_metadata: Option<bool>,
}

/// One problem slot in a contest detail view, including compiled page range when available.
#[derive(Debug, Serialize)]
pub struct ContestProblemView {
    pub label: String,
    pub id: String,
    /// Effective title (override applied, or snapshot title when frozen).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub overrides: ContestItemOverrides,
    /// First physical page (1-based) from real compilation, when compilation succeeded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first: Option<usize>,
    /// Number of physical pages from real compilation, when compilation succeeded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<usize>,
}

/// Full structured view of one contest (`xcpc show contest` / MCP `get_contest`).
#[derive(Debug, Serialize)]
pub struct ContestDetail {
    pub ok: bool,
    pub id: String,
    pub dir: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    pub language: Language,
    pub show_metadata: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover: Option<bool>,
    pub has_cover: bool,
    pub blank_pages: bool,
    pub frozen: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pages: Option<usize>,
    pub problems: Vec<ContestProblemView>,
    pub stats: ContestStats,
}

/// Loads one problem's metadata, statements, editorial (with SHA-256 hashes), samples and solutions.
pub fn problem_detail(ws: &Workspace, id: &str) -> Result<ProblemDetail, Error> {
    let problem = ws.load_problem(id)?;
    let meta_bytes = ws.read_bytes(&format!("{}/problem.toml", problem.dir))?;
    let meta_hash = content_hash(&meta_bytes);

    let statements = problem
        .statements
        .into_iter()
        .map(|s| {
            let file = s
                .file
                .path
                .rsplit('/')
                .next()
                .unwrap_or(&s.file.path)
                .to_owned();
            let hash = content_hash(s.file.text.as_bytes());
            StatementView {
                path: s.file.path,
                file,
                language: s.language,
                text: s.file.text,
                hash,
            }
        })
        .collect();

    let editorial = problem.editorial.map(|e| {
        let file = e.path.rsplit('/').next().unwrap_or(&e.path).to_owned();
        let hash = content_hash(e.text.as_bytes());
        TextFileView {
            path: e.path,
            file,
            text: e.text,
            hash,
        }
    });

    let sample_inventory = ws.inspect_samples(&format!("{}/samples", problem.dir))?;
    let solutions = ws.list_solutions(id)?;

    Ok(ProblemDetail {
        ok: true,
        id: problem.id,
        dir: problem.dir,
        meta: problem.meta,
        meta_hash,
        statements,
        editorial,
        samples: problem.samples,
        sample_inventory,
        solutions,
    })
}

/// Lists all valid contests in the workspace with summary metadata.
pub fn list_contests(ws: &Workspace) -> Result<Vec<ContestSummary>, Error> {
    let mut out = Vec::new();
    for id in ws.list_contests()? {
        if let Ok(contest) = ws.load_contest(&id) {
            out.push(ContestSummary {
                id: contest.id,
                name: contest.meta.name,
                location: contest.meta.location,
                date: contest.meta.date,
                language: contest.meta.language,
                frozen: contest.meta.frozen,
                problem_count: contest.meta.problems.len(),
            });
        }
    }
    Ok(out)
}

/// Loads one contest's metadata, frozen status, per-problem compiled page ranges and statistics.
pub fn contest_detail(
    ws: &Workspace,
    engine: &mut Engine,
    id: &str,
) -> Result<ContestDetail, Error> {
    let contest = ws.load_contest(id)?;
    let index = WorkspaceIndex::scan(ws)?;
    let stats = index.contest_stats(&contest.meta);

    let target = Target::Contest(contest.id.clone());
    let (pages, page_map) = match publish::check(ws, engine, &target, Kind::Statements, None, None)
    {
        Ok((_, _, report)) => {
            let map: HashMap<(String, String), (usize, usize)> = report
                .problems
                .into_iter()
                .map(|p| ((p.label, p.id), (p.first, p.count)))
                .collect();
            (report.pages, map)
        }
        Err(_) => (None, HashMap::new()),
    };

    let snapshot_titles: HashMap<String, String> = if contest.meta.frozen {
        ws.contest_problems(&contest)
            .map(|pairs| {
                pairs
                    .into_iter()
                    .map(|(item, prob)| (item.label, prob.meta.title))
                    .collect()
            })
            .unwrap_or_default()
    } else {
        HashMap::new()
    };

    let problems = contest
        .meta
        .problems
        .iter()
        .map(|item| {
            let title = if contest.meta.frozen {
                snapshot_titles
                    .get(&item.label)
                    .cloned()
                    .or_else(|| item.title.clone())
                    .or_else(|| index.problem(&item.id).map(|p| p.title.clone()))
            } else {
                item.title
                    .clone()
                    .or_else(|| index.problem(&item.id).map(|p| p.title.clone()))
                    .or_else(|| ws.load_problem(&item.id).ok().map(|p| p.meta.title))
            };
            let (first, count) = page_map
                .get(&(item.label.clone(), item.id.clone()))
                .copied()
                .map_or((None, None), |(f, c)| (Some(f), Some(c)));
            ContestProblemView {
                label: item.label.clone(),
                id: item.id.clone(),
                title,
                overrides: ContestItemOverrides {
                    title: item.title.clone(),
                    limits: item.limits.clone(),
                    show_metadata: item.show_metadata,
                },
                first,
                count,
            }
        })
        .collect();

    let has_cover = contest.meta.has_cover();
    Ok(ContestDetail {
        ok: true,
        id: contest.id,
        dir: contest.dir,
        name: contest.meta.name,
        location: contest.meta.location,
        date: contest.meta.date,
        author: contest.meta.author,
        language: contest.meta.language,
        show_metadata: contest.meta.show_metadata,
        cover: contest.meta.cover,
        has_cover,
        blank_pages: contest.meta.blank_pages,
        frozen: contest.meta.frozen,
        pages,
        problems,
        stats,
    })
}
