//! In-memory workspace index: startup scan, incremental path/problem/contest refresh,
//! structured filtering + normalized substring search, contest statistics, and
//! Frecency-weighted fuzzy search for `Cmd+K` (docs/architecture.md §5, docs/roadmap.md Phase 2 §4).
//!
//! No SQLite or FTS5 is used.

use std::collections::{BTreeMap, HashMap};
use std::time::{SystemTime, UNIX_EPOCH};

use xcpc_domain::{
    ContestMeta, ContestStats, FuzzyHit, FuzzyMatchKind, GroupFilter, GroupSummary, ProblemSummary,
    RatingTier, SearchQuery, check_id, normalize_group_name,
};

use crate::{Error, Result, Workspace};

/// Half-life for Frecency decay (7 days in seconds).
const FRECENCY_HALF_LIFE_SECS: f64 = 7.0 * 24.0 * 3600.0;

#[derive(Debug, Clone)]
struct IndexedProblem {
    summary: ProblemSummary,
    normalized_id: String,
    normalized_title: String,
    /// Normalized searchable corpus: id, title, author, tags, sources, statements, editorial.
    normalized_text: String,
}

#[derive(Debug, Clone)]
struct IndexedContest {
    id: String,
    meta: ContestMeta,
    normalized_id: String,
    normalized_name: String,
}

#[derive(Debug, Clone, Copy)]
struct FrecencyEntry {
    score: f64,
    last_used_secs: u64,
}

/// In-memory index of all problems and contests in a workspace.
#[derive(Debug, Clone, Default)]
pub struct WorkspaceIndex {
    problems: BTreeMap<String, IndexedProblem>,
    contests: BTreeMap<String, IndexedContest>,
    declared_groups: Vec<String>,
    frecency: HashMap<(FuzzyMatchKind, String), FrecencyEntry>,
}

impl WorkspaceIndex {
    /// Scans the entire workspace once at startup.
    ///
    /// Individual malformed `problem.toml` or `contest.toml` files are skipped so a
    /// syntax error in one file does not prevent indexing the rest of the workspace.
    pub fn scan(ws: &Workspace) -> Result<Self> {
        let mut index = Self {
            declared_groups: ws.workspace_groups(),
            ..Self::default()
        };
        for id in ws.list_problems()? {
            if let Ok(entry) = load_indexed_problem(ws, &id) {
                index.problems.insert(id, entry);
            }
        }
        for id in ws.list_contests()? {
            if let Ok(entry) = load_indexed_contest(ws, &id) {
                index.contests.insert(id, entry);
            }
        }
        Ok(index)
    }

    /// Refreshes workspace-level metadata (such as declared groups from `workspace.toml`).
    pub fn refresh_workspace_meta(&mut self, ws: &Workspace) {
        self.declared_groups = ws.workspace_groups();
    }

    /// Incrementally refreshes one problem from disk (or removes it if deleted).
    pub fn refresh_problem(&mut self, ws: &Workspace, id: &str) -> Result<()> {
        check_id(id)?;
        match load_indexed_problem(ws, id) {
            Ok(entry) => {
                self.problems.insert(id.to_owned(), entry);
                Ok(())
            }
            Err(Error::NotFound(_)) => {
                self.problems.remove(id);
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    /// Incrementally refreshes one contest from disk (or removes it if deleted).
    pub fn refresh_contest(&mut self, ws: &Workspace, id: &str) -> Result<()> {
        check_id(id)?;
        match load_indexed_contest(ws, id) {
            Ok(entry) => {
                self.contests.insert(id.to_owned(), entry);
                Ok(())
            }
            Err(Error::NotFound(_)) => {
                self.contests.remove(id);
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    /// Incrementally refreshes the index entry affected by a workspace-relative path
    /// (typically received from [`crate::WatchEvent::Changed`]).
    ///
    /// Ignores unrelated paths and tolerates temporarily invalid TOML mid-edit.
    pub fn refresh_path(&mut self, ws: &Workspace, rel: &str) -> Result<()> {
        let norm = rel.replace('\\', "/");
        let trimmed = norm.trim_start_matches("./").trim_start_matches('/');
        let mut parts = trimmed.split('/');
        match (parts.next(), parts.next()) {
            (Some("workspace.toml"), None) => {
                self.declared_groups = ws.workspace_groups();
                Ok(())
            }
            (Some("problems"), Some(id)) if check_id(id).is_ok() => {
                match self.refresh_problem(ws, id) {
                    Ok(()) | Err(Error::Invalid { .. }) => Ok(()),
                    Err(e) => Err(e),
                }
            }
            (Some("contests"), Some(id)) if check_id(id).is_ok() => {
                match self.refresh_contest(ws, id) {
                    Ok(()) | Err(Error::Invalid { .. }) => Ok(()),
                    Err(e) => Err(e),
                }
            }
            _ => Ok(()),
        }
    }

    /// All indexed problems, sorted by ID.
    pub fn problems(&self) -> Vec<&ProblemSummary> {
        self.problems.values().map(|p| &p.summary).collect()
    }

    /// Looks up one indexed problem summary by ID.
    pub fn problem(&self, id: &str) -> Option<&ProblemSummary> {
        self.problems.get(id).map(|p| &p.summary)
    }

    /// All indexed contests as `(id, &ContestMeta)`, sorted by ID.
    pub fn contests(&self) -> Vec<(&str, &ContestMeta)> {
        self.contests
            .values()
            .map(|c| (c.id.as_str(), &c.meta))
            .collect()
    }

    /// Looks up one indexed contest by ID.
    pub fn contest(&self, id: &str) -> Option<&ContestMeta> {
        self.contests.get(id).map(|c| &c.meta)
    }

    /// Returns persistent group summaries (declared groups in `workspace.toml` order,
    /// followed by undeclared groups from `problem.toml` in lexicographical order)
    /// along with the count of ungrouped problems.
    pub fn group_counts(&self) -> (Vec<GroupSummary>, usize) {
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        let mut ungrouped = 0usize;
        for entry in self.problems.values() {
            match &entry.summary.group {
                Some(g) => *counts.entry(g.clone()).or_insert(0) += 1,
                None => ungrouped += 1,
            }
        }
        let mut summaries = Vec::with_capacity(self.declared_groups.len() + counts.len());
        for name in &self.declared_groups {
            let count = counts.remove(name).unwrap_or(0);
            summaries.push(GroupSummary {
                name: name.clone(),
                count,
                declared: true,
            });
        }
        for (name, count) in counts {
            summaries.push(GroupSummary {
                name,
                count,
                declared: false,
            });
        }
        (summaries, ungrouped)
    }

    /// Returns the sorted IDs of all non-archived contests that include `problem_id`.
    pub fn problem_contest_ids(&self, problem_id: &str) -> Vec<String> {
        self.contests
            .values()
            .filter(|c| !c.meta.archived && c.meta.problems.iter().any(|p| p.id == problem_id))
            .map(|c| c.id.clone())
            .collect()
    }

    /// Builds a `problem_id -> Vec<contest_id>` map across all non-archived contests.
    pub fn problem_contest_map(&self) -> HashMap<String, Vec<String>> {
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        for c in self.contests.values() {
            if c.meta.archived {
                continue;
            }
            for item in &c.meta.problems {
                map.entry(item.id.clone()).or_default().push(c.id.clone());
            }
        }
        map
    }

    /// Structured filtering + normalized substring search across indexed problems.
    pub fn search_problems(&self, query: &SearchQuery) -> Vec<ProblemSummary> {
        let req_tags: Vec<String> = query
            .tags
            .iter()
            .map(|t| t.trim().to_lowercase())
            .filter(|t| !t.is_empty())
            .collect();
        let req_platform = query
            .platform
            .as_deref()
            .map(|p| p.trim().to_lowercase())
            .filter(|p| !p.is_empty());
        let text_tokens: Vec<String> = query
            .text
            .as_deref()
            .map(normalize_text)
            .filter(|s| !s.is_empty())
            .map(|s| s.split_whitespace().map(str::to_owned).collect())
            .unwrap_or_default();

        let mut ranked: Vec<(u8, &ProblemSummary)> = Vec::new();
        for entry in self.problems.values() {
            let summary = &entry.summary;

            if let Some(min) = query.rating_min
                && summary.rating.is_none_or(|r| r < min)
            {
                continue;
            }
            if let Some(max) = query.rating_max
                && summary.rating.is_none_or(|r| r > max)
            {
                continue;
            }
            if !req_tags.is_empty() {
                let has_all = req_tags
                    .iter()
                    .all(|wanted| summary.tags.iter().any(|t| t.to_lowercase() == *wanted));
                if !has_all {
                    continue;
                }
            }
            if let Some(plat) = &req_platform
                && !summary
                    .sources
                    .iter()
                    .any(|s| s.platform.to_lowercase() == *plat)
            {
                continue;
            }
            if let Some(group_filter) = &query.group {
                match group_filter {
                    GroupFilter::Named(name) => {
                        let wanted = name.trim();
                        if summary.group.as_deref() != Some(wanted) {
                            continue;
                        }
                    }
                    GroupFilter::Ungrouped => {
                        if summary.group.is_some() {
                            continue;
                        }
                    }
                }
            }
            if !text_tokens.is_empty()
                && !text_tokens
                    .iter()
                    .all(|tok| entry.normalized_text.contains(tok.as_str()))
            {
                continue;
            }

            // Rank ID/title matches ahead of body-only matches when text is given.
            let bucket = if text_tokens.is_empty()
                || text_tokens
                    .iter()
                    .all(|tok| entry.normalized_id.contains(tok.as_str()))
            {
                0
            } else if text_tokens
                .iter()
                .all(|tok| entry.normalized_title.contains(tok.as_str()))
            {
                1
            } else {
                2
            };
            ranked.push((bucket, summary));
        }

        ranked.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.id.cmp(&b.1.id)));
        ranked.into_iter().map(|(_, s)| s.clone()).collect()
    }

    /// Computes difficulty distribution (7 rating tiers + `unrated`) and tag counts for
    /// a contest's problems.
    pub fn contest_stats(&self, contest: &ContestMeta) -> ContestStats {
        let mut rating_distribution: BTreeMap<RatingTier, usize> =
            RatingTier::ALL.into_iter().map(|tier| (tier, 0)).collect();
        let mut tag_map: BTreeMap<String, usize> = BTreeMap::new();

        for item in &contest.problems {
            match self.problems.get(&item.id) {
                Some(entry) => {
                    let tier = RatingTier::from_rating(entry.summary.rating);
                    *rating_distribution.entry(tier).or_insert(0) += 1;
                    for tag in &entry.summary.tags {
                        *tag_map.entry(tag.clone()).or_insert(0) += 1;
                    }
                }
                None => {
                    *rating_distribution.entry(RatingTier::Unrated).or_insert(0) += 1;
                }
            }
        }

        let mut tag_counts: Vec<(String, usize)> = tag_map.into_iter().collect();
        tag_counts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

        ContestStats {
            problem_count: contest.problems.len(),
            rating_distribution,
            tag_counts,
        }
    }

    /// Convenience wrapper around [`Self::contest_stats`] using an indexed contest ID.
    pub fn contest_stats_by_id(&self, contest_id: &str) -> Option<ContestStats> {
        self.contests
            .get(contest_id)
            .map(|c| self.contest_stats(&c.meta))
    }

    /// Records a user visit/selection for Frecency ranking in [`Self::fuzzy_search`].
    pub fn record_visit(&mut self, kind: FuzzyMatchKind, id: &str) {
        self.record_visit_at(kind, id, now_epoch_secs());
    }

    /// Records a visit at an explicit Unix timestamp (useful for deterministic tests).
    pub fn record_visit_at(&mut self, kind: FuzzyMatchKind, id: &str, now_secs: u64) {
        let key = (kind, id.to_owned());
        let decayed = self
            .frecency
            .get(&key)
            .map(|e| decayed_frecency(*e, now_secs))
            .unwrap_or(0.0);
        self.frecency.insert(
            key,
            FrecencyEntry {
                score: decayed + 1.0,
                last_used_secs: now_secs,
            },
        );
    }

    /// Fuzzy search across problems and contests with Frecency boost (for `Cmd+K`).
    pub fn fuzzy_search(&self, query: &str, limit: usize) -> Vec<FuzzyHit> {
        self.fuzzy_search_at(query, limit, now_epoch_secs())
    }

    /// Fuzzy search at an explicit Unix timestamp (useful for deterministic tests).
    pub fn fuzzy_search_at(&self, query: &str, limit: usize, now_secs: u64) -> Vec<FuzzyHit> {
        if limit == 0 {
            return Vec::new();
        }
        let q = normalize_text(query);
        let terms: Vec<&str> = q.split_whitespace().collect();
        let mut hits = Vec::new();

        for entry in self.problems.values() {
            let frec = self.frecency_boost(FuzzyMatchKind::Problem, &entry.summary.id, now_secs);
            let base = if terms.is_empty() {
                Some(0.0)
            } else {
                score_pair(&terms, &entry.normalized_id, &entry.normalized_title)
            };
            if let Some(match_score) = base {
                hits.push(FuzzyHit {
                    kind: FuzzyMatchKind::Problem,
                    id: entry.summary.id.clone(),
                    title: entry.summary.title.clone(),
                    score: match_score + frec,
                });
            }
        }

        for entry in self.contests.values() {
            let frec = self.frecency_boost(FuzzyMatchKind::Contest, &entry.id, now_secs);
            let base = if terms.is_empty() {
                Some(0.0)
            } else {
                score_pair(&terms, &entry.normalized_id, &entry.normalized_name)
            };
            if let Some(match_score) = base {
                hits.push(FuzzyHit {
                    kind: FuzzyMatchKind::Contest,
                    id: entry.id.clone(),
                    title: entry.meta.name.clone(),
                    score: match_score + frec,
                });
            }
        }

        hits.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| a.kind.cmp(&b.kind))
                .then_with(|| a.id.cmp(&b.id))
        });
        hits.truncate(limit);
        hits
    }

    fn frecency_boost(&self, kind: FuzzyMatchKind, id: &str, now_secs: u64) -> f64 {
        let Some(entry) = self.frecency.get(&(kind, id.to_owned())) else {
            return 0.0;
        };
        let f = decayed_frecency(*entry, now_secs);
        // Smoothly bounded boost so recent/frequent items win ties and close matches.
        12.0 * (1.0 + f).ln()
    }
}

fn load_indexed_problem(ws: &Workspace, id: &str) -> Result<IndexedProblem> {
    let problem = ws.load_problem(id)?;
    let has_solutions = !ws.list_solutions(id)?.is_empty();
    let statement_path = problem
        .statement(problem.default_language())
        .map(|s| s.file.path.clone());

    let mut corpus = String::new();
    let mut push_part = |s: &str| {
        if !s.is_empty() {
            if !corpus.is_empty() {
                corpus.push(' ');
            }
            corpus.push_str(s);
        }
    };

    let group = problem
        .meta
        .group
        .as_deref()
        .and_then(|g| normalize_group_name(g).ok());

    push_part(&problem.id);
    push_part(&problem.meta.title);
    if let Some(group) = &group {
        push_part(group);
    }
    if let Some(author) = &problem.meta.author {
        push_part(author);
    }
    for tag in &problem.meta.tags {
        push_part(tag);
    }
    for src in &problem.meta.sources {
        push_part(&src.platform);
        push_part(&src.id);
        if let Some(url) = &src.url {
            push_part(url);
        }
    }
    for st in &problem.statements {
        push_part(&st.file.text);
    }
    if let Some(ed) = &problem.editorial {
        push_part(&ed.text);
    }

    let summary = ProblemSummary {
        id: problem.id.clone(),
        title: problem.meta.title.clone(),
        group,
        rating: problem.meta.rating,
        tags: problem.meta.tags,
        sources: problem.meta.sources,
        has_editorial: problem.editorial.is_some(),
        has_solutions,
        statement_path,
    };

    Ok(IndexedProblem {
        normalized_id: normalize_text(&summary.id),
        normalized_title: normalize_text(&summary.title),
        normalized_text: normalize_text(&corpus),
        summary,
    })
}

fn load_indexed_contest(ws: &Workspace, id: &str) -> Result<IndexedContest> {
    let contest = ws.load_contest(id)?;
    Ok(IndexedContest {
        normalized_id: normalize_text(&contest.id),
        normalized_name: normalize_text(&contest.meta.name),
        id: contest.id,
        meta: contest.meta,
    })
}

/// Normalizes text for case-insensitive, whitespace-collapsed matching.
pub(crate) fn normalize_text(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut prev_space = true;
    for ch in input.chars() {
        if ch.is_whitespace() {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            for lower in ch.to_lowercase() {
                out.push(lower);
            }
            prev_space = false;
        }
    }
    if out.ends_with(' ') {
        out.pop();
    }
    out
}

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn decayed_frecency(entry: FrecencyEntry, now_secs: u64) -> f64 {
    let elapsed = now_secs.saturating_sub(entry.last_used_secs) as f64;
    entry.score * 0.5_f64.powf(elapsed / FRECENCY_HALF_LIFE_SECS)
}

fn score_pair(terms: &[&str], id: &str, title: &str) -> Option<f64> {
    let mut total = 0.0;
    for term in terms {
        let best = match (fuzzy_score(term, id), fuzzy_score(term, title)) {
            (Some(a), Some(b)) => a.max(b),
            (Some(a), None) => a,
            (None, Some(b)) => b,
            (None, None) => return None,
        };
        total += best;
    }
    Some(total / terms.len() as f64)
}

/// Computes a fuzzy match score for `needle` inside `haystack` (both already normalized).
/// Returns `None` if `needle` is not a subsequence of `haystack`.
fn fuzzy_score(needle: &str, haystack: &str) -> Option<f64> {
    if needle.is_empty() {
        return Some(0.0);
    }
    if haystack == needle {
        return Some(120.0);
    }
    if haystack.starts_with(needle) {
        let len_penalty = (haystack.chars().count() - needle.chars().count()) as f64 * 0.2;
        return Some((100.0 - len_penalty).max(70.0));
    }
    if let Some(byte_pos) = haystack.find(needle) {
        let boundary = byte_pos == 0
            || haystack[..byte_pos]
                .chars()
                .next_back()
                .is_some_and(is_boundary_sep);
        let base = if boundary { 85.0 } else { 70.0 };
        let pos_penalty = haystack[..byte_pos].chars().count() as f64 * 0.3;
        return Some((base - pos_penalty).max(40.0));
    }

    // Subsequence match with consecutive + word-boundary bonuses.
    let hay: Vec<char> = haystack.chars().collect();
    let ned: Vec<char> = needle.chars().collect();
    let mut score = 20.0;
    let mut h_idx = 0usize;
    let mut prev_match: Option<usize> = None;

    for &nc in &ned {
        let mut found = None;
        for (i, &hc) in hay.iter().enumerate().skip(h_idx) {
            if hc == nc {
                found = Some(i);
                break;
            }
        }
        let pos = found?;
        if pos == 0 || is_boundary_sep(hay[pos - 1]) {
            score += 8.0;
        }
        if let Some(prev) = prev_match {
            if pos == prev + 1 {
                score += 6.0;
            } else {
                score -= (pos - prev - 1) as f64 * 0.5;
            }
        } else {
            score -= pos as f64 * 0.3;
        }
        prev_match = Some(pos);
        h_idx = pos + 1;
    }

    Some(score.max(1.0))
}

fn is_boundary_sep(c: char) -> bool {
    matches!(c, '-' | '_' | ' ' | '/' | '.' | ':')
}
