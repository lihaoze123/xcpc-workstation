use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use xcpc_domain::{ProblemSource, SyncedProblemStatus};
use xcpc_plugin::{OjAccountSync, OjContest, OjSubmission, OjSummarySync};

use crate::error::Result;

pub const DB_FILENAME: &str = "oj_sync.db";

const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS oj_accounts (
    platform TEXT PRIMARY KEY,
    handle TEXT NOT NULL,
    rating INTEGER,
    max_rating INTEGER,
    rank TEXT,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS oj_rating_history (
    platform TEXT NOT NULL,
    contest_id TEXT NOT NULL,
    contest_name TEXT NOT NULL,
    rating_before INTEGER,
    rating_after INTEGER NOT NULL,
    rank INTEGER,
    at INTEGER NOT NULL,
    PRIMARY KEY (platform, contest_id)
);

CREATE TABLE IF NOT EXISTS oj_submissions (
    platform TEXT NOT NULL,
    submission_id TEXT NOT NULL,
    handle TEXT NOT NULL,
    contest_id TEXT,
    problem_index TEXT,
    problem_key TEXT NOT NULL,
    problem_title TEXT,
    problem_url TEXT,
    raw_rating INTEGER,
    normalized_rating INTEGER,
    tags_json TEXT NOT NULL,
    verdict TEXT NOT NULL,
    language TEXT,
    submitted_at INTEGER NOT NULL,
    PRIMARY KEY (platform, submission_id)
);

CREATE INDEX IF NOT EXISTS idx_oj_submissions_problem
    ON oj_submissions (platform, problem_key);

CREATE INDEX IF NOT EXISTS idx_oj_submissions_submitted_at
    ON oj_submissions (submitted_at);

CREATE TABLE IF NOT EXISTS oj_contests (
    platform TEXT NOT NULL,
    contest_id TEXT NOT NULL,
    name TEXT NOT NULL,
    start_time INTEGER NOT NULL,
    duration_sec INTEGER NOT NULL,
    url TEXT,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (platform, contest_id)
);

CREATE TABLE IF NOT EXISTS oj_problem_meta (
    platform TEXT NOT NULL,
    problem_key TEXT NOT NULL,
    title TEXT,
    url TEXT,
    raw_rating INTEGER,
    normalized_rating INTEGER,
    tags_json TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (platform, problem_key)
);

CREATE TABLE IF NOT EXISTS oj_summaries (
    platform TEXT PRIMARY KEY,
    handle TEXT NOT NULL,
    solved_count INTEGER NOT NULL,
    submission_count INTEGER,
    solved_keys_json TEXT NOT NULL,
    rating INTEGER,
    updated_at TEXT NOT NULL
);
"#;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedAccount {
    pub platform: String,
    pub handle: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_rating: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rank: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedRatingChange {
    pub platform: String,
    pub contest_id: String,
    pub contest_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating_before: Option<i64>,
    pub rating_after: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rank: Option<i64>,
    pub at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedSubmission {
    pub platform: String,
    pub submission_id: String,
    pub handle: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contest_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem_index: Option<String>,
    pub problem_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_rating: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normalized_rating: Option<i64>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub verdict: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    pub submitted_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedContest {
    pub platform: String,
    pub contest_id: String,
    pub name: String,
    pub start_time: i64,
    pub duration_sec: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedProblemMeta {
    pub platform: String,
    pub problem_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_rating: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normalized_rating: Option<i64>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedSummary {
    pub platform: String,
    pub handle: String,
    pub solved_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub submission_count: Option<u64>,
    #[serde(default)]
    pub solved_keys: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<i64>,
    pub updated_at: String,
}

/// Rebuildable SQLite cache at `<dataDir>/oj_sync.db`.
pub struct SyncDb {
    conn: Connection,
}

impl SyncDb {
    /// Returns `<data_dir>/oj_sync.db`.
    pub fn db_path(data_dir: &Path) -> PathBuf {
        data_dir.join(DB_FILENAME)
    }

    /// Opens (creating if needed) `<data_dir>/oj_sync.db` with WAL mode and schema initialized.
    pub fn open(data_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(data_dir)?;
        Self::open_at(&Self::db_path(data_dir))
    }

    /// Opens (creating if needed) a SQLite cache at an explicit file path.
    pub fn open_at(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        let db = Self { conn };
        db.init()?;
        Ok(db)
    }

    /// Opens an in-memory SQLite cache (useful for tests).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let db = Self { conn };
        db.init()?;
        Ok(db)
    }

    fn init(&self) -> Result<()> {
        self.conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = OFF;
             PRAGMA busy_timeout = 5000;",
        )?;
        self.conn.execute_batch(SCHEMA_SQL)?;
        Ok(())
    }

    /// Upserts an account row and its rating history in a single transaction.
    /// Returns the number of rating history rows upserted.
    pub fn upsert_account(
        &mut self,
        platform: &str,
        account: &OjAccountSync,
        updated_at: &str,
    ) -> Result<usize> {
        let platform = platform.trim().to_ascii_lowercase();
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO oj_accounts (platform, handle, rating, max_rating, rank, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(platform) DO UPDATE SET
                 handle = excluded.handle,
                 rating = excluded.rating,
                 max_rating = excluded.max_rating,
                 rank = excluded.rank,
                 updated_at = excluded.updated_at",
            params![
                platform,
                account.handle,
                account.rating,
                account.max_rating,
                account.rank,
                updated_at,
            ],
        )?;

        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO oj_rating_history
                    (platform, contest_id, contest_name, rating_before, rating_after, rank, at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(platform, contest_id) DO UPDATE SET
                    contest_name = excluded.contest_name,
                    rating_before = excluded.rating_before,
                    rating_after = excluded.rating_after,
                    rank = excluded.rank,
                    at = excluded.at",
            )?;
            for item in &account.rating_history {
                stmt.execute(params![
                    platform,
                    item.contest_id,
                    item.contest_name,
                    Some(item.rating_before),
                    item.rating_after,
                    item.rank,
                    item.at,
                ])?;
            }
        }

        tx.commit()?;
        Ok(account.rating_history.len())
    }

    /// Deletes all user-specific cached rows for `platform` (`oj_accounts`, `oj_rating_history`,
    /// `oj_submissions`, and `oj_summaries`) while preserving shared `oj_contests` and
    /// `oj_problem_meta`.
    pub fn purge_platform_user_data(&self, platform: &str) -> Result<()> {
        let p = platform.trim().to_ascii_lowercase();
        if p.is_empty() {
            return Ok(());
        }
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "DELETE FROM oj_accounts WHERE LOWER(platform) = ?1",
            params![&p],
        )?;
        tx.execute(
            "DELETE FROM oj_rating_history WHERE LOWER(platform) = ?1",
            params![&p],
        )?;
        tx.execute(
            "DELETE FROM oj_submissions WHERE LOWER(platform) = ?1",
            params![&p],
        )?;
        tx.execute(
            "DELETE FROM oj_summaries WHERE LOWER(platform) = ?1",
            params![&p],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Upserts a lightweight summary row (from `syncSummary`).
    pub fn upsert_summary(
        &mut self,
        platform: &str,
        summary: &OjSummarySync,
        updated_at: &str,
    ) -> Result<()> {
        let platform = platform.trim().to_ascii_lowercase();
        let solved_keys_json = serde_json::to_string(&summary.solved_problem_keys)?;
        self.conn.execute(
            "INSERT INTO oj_summaries
                (platform, handle, solved_count, submission_count, solved_keys_json, rating, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(platform) DO UPDATE SET
                handle = excluded.handle,
                solved_count = excluded.solved_count,
                submission_count = excluded.submission_count,
                solved_keys_json = excluded.solved_keys_json,
                rating = excluded.rating,
                updated_at = excluded.updated_at",
            params![
                platform,
                summary.handle,
                summary.solved_count as i64,
                summary.submission_count.map(|v| v as i64),
                solved_keys_json,
                summary.rating,
                updated_at,
            ],
        )?;
        Ok(())
    }

    /// Upserts submissions and enriches `oj_problem_meta` in a single transaction.
    /// Returns the number of submissions processed.
    pub fn upsert_submissions(
        &mut self,
        platform: &str,
        submissions: &[OjSubmission],
        updated_at: &str,
    ) -> Result<usize> {
        if submissions.is_empty() {
            return Ok(0);
        }
        let platform = platform.trim().to_ascii_lowercase();
        let tx = self.conn.transaction()?;
        {
            let mut sub_stmt = tx.prepare_cached(
                "INSERT INTO oj_submissions
                    (platform, submission_id, handle, contest_id, problem_index, problem_key,
                     problem_title, problem_url, raw_rating, normalized_rating, tags_json,
                     verdict, language, submitted_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
                 ON CONFLICT(platform, submission_id) DO UPDATE SET
                    handle = excluded.handle,
                    contest_id = excluded.contest_id,
                    problem_index = excluded.problem_index,
                    problem_key = excluded.problem_key,
                    problem_title = excluded.problem_title,
                    problem_url = excluded.problem_url,
                    raw_rating = excluded.raw_rating,
                    normalized_rating = excluded.normalized_rating,
                    tags_json = excluded.tags_json,
                    verdict = excluded.verdict,
                    language = excluded.language,
                    submitted_at = excluded.submitted_at",
            )?;

            let mut meta_stmt = tx.prepare_cached(
                "INSERT INTO oj_problem_meta
                    (platform, problem_key, title, url, raw_rating, normalized_rating, tags_json, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(platform, problem_key) DO UPDATE SET
                    title = COALESCE(NULLIF(excluded.title, ''), oj_problem_meta.title),
                    url = COALESCE(NULLIF(excluded.url, ''), oj_problem_meta.url),
                    raw_rating = COALESCE(excluded.raw_rating, oj_problem_meta.raw_rating),
                    normalized_rating = COALESCE(excluded.normalized_rating, oj_problem_meta.normalized_rating),
                    tags_json = CASE
                        WHEN excluded.tags_json != '[]' THEN excluded.tags_json
                        ELSE oj_problem_meta.tags_json
                    END,
                    updated_at = excluded.updated_at",
            )?;

            for sub in submissions {
                let raw_rating = sub
                    .raw_rating
                    .filter(|v| v.is_finite())
                    .map(|v| v.round() as i64);
                let tags_json = serde_json::to_string(&sub.tags)?;
                let title_opt =
                    (!sub.problem_title.trim().is_empty()).then(|| sub.problem_title.clone());

                sub_stmt.execute(params![
                    platform,
                    sub.submission_id,
                    sub.handle,
                    sub.contest_id,
                    sub.problem_index,
                    sub.problem_key,
                    title_opt,
                    sub.problem_url,
                    raw_rating,
                    sub.normalized_rating,
                    tags_json,
                    sub.verdict,
                    sub.language,
                    sub.submitted_at,
                ])?;

                if !sub.problem_key.trim().is_empty() {
                    meta_stmt.execute(params![
                        platform,
                        sub.problem_key,
                        title_opt,
                        sub.problem_url,
                        raw_rating,
                        sub.normalized_rating,
                        tags_json,
                        updated_at,
                    ])?;
                }
            }
        }
        tx.commit()?;
        Ok(submissions.len())
    }

    /// Upserts upcoming/recent contests for a platform in a single transaction.
    pub fn upsert_contests(
        &mut self,
        platform: &str,
        contests: &[OjContest],
        updated_at: &str,
    ) -> Result<usize> {
        let platform = platform.trim().to_ascii_lowercase();
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO oj_contests
                    (platform, contest_id, name, start_time, duration_sec, url, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(platform, contest_id) DO UPDATE SET
                    name = excluded.name,
                    start_time = excluded.start_time,
                    duration_sec = excluded.duration_sec,
                    url = excluded.url,
                    updated_at = excluded.updated_at",
            )?;
            for c in contests {
                stmt.execute(params![
                    platform,
                    c.contest_id,
                    c.name,
                    c.start_time,
                    c.duration_sec,
                    c.url,
                    updated_at,
                ])?;
            }
        }
        tx.commit()?;
        Ok(contests.len())
    }

    /// Upserts a single `oj_problem_meta` row.
    pub fn upsert_problem_meta(&mut self, meta: &CachedProblemMeta) -> Result<()> {
        let platform = meta.platform.trim().to_ascii_lowercase();
        let tags_json = serde_json::to_string(&meta.tags)?;
        self.conn.execute(
            "INSERT INTO oj_problem_meta
                (platform, problem_key, title, url, raw_rating, normalized_rating, tags_json, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(platform, problem_key) DO UPDATE SET
                title = COALESCE(NULLIF(excluded.title, ''), oj_problem_meta.title),
                url = COALESCE(NULLIF(excluded.url, ''), oj_problem_meta.url),
                raw_rating = COALESCE(excluded.raw_rating, oj_problem_meta.raw_rating),
                normalized_rating = COALESCE(excluded.normalized_rating, oj_problem_meta.normalized_rating),
                tags_json = CASE
                    WHEN excluded.tags_json != '[]' THEN excluded.tags_json
                    ELSE oj_problem_meta.tags_json
                END,
                updated_at = excluded.updated_at",
            params![
                platform,
                meta.problem_key,
                meta.title,
                meta.url,
                meta.raw_rating,
                meta.normalized_rating,
                tags_json,
                meta.updated_at,
            ],
        )?;
        Ok(())
    }

    /// Checks both `oj_submissions` and `oj_summaries.solved_keys_json` (case-insensitive
    /// matching on platform and problem_key) for a single problem's external sources.
    pub fn get_problem_sync_status(
        &self,
        sources: &[ProblemSource],
    ) -> Result<SyncedProblemStatus> {
        if sources.is_empty() {
            return Ok(SyncedProblemStatus::default());
        }

        let mut status = SyncedProblemStatus::default();
        let mut summary_cache: HashMap<String, HashSet<String>> = HashMap::new();

        let mut sub_stmt = self.conn.prepare_cached(
            "SELECT
                MAX(CASE WHEN UPPER(verdict) IN ('AC', 'OK') THEN 1 ELSE 0 END),
                COUNT(1)
             FROM oj_submissions
             WHERE LOWER(platform) = ?1 AND LOWER(problem_key) = ?2",
        )?;

        let mut sum_stmt = self.conn.prepare_cached(
            "SELECT solved_keys_json FROM oj_summaries WHERE LOWER(platform) = ?1",
        )?;

        for src in sources {
            let platform = src.platform.trim().to_ascii_lowercase();
            let key = src.id.trim().to_ascii_lowercase();
            if platform.is_empty() || key.is_empty() {
                continue;
            }

            let (has_ac, count): (Option<i64>, i64) = sub_stmt
                .query_row(params![&platform, &key], |row| {
                    Ok((row.get(0)?, row.get(1)?))
                })?;
            if has_ac.unwrap_or(0) > 0 {
                status.ac = true;
                status.attempted = true;
                return Ok(status);
            }
            if count > 0 {
                status.attempted = true;
            }

            if !summary_cache.contains_key(&platform) {
                let mut set = HashSet::new();
                let mut rows = sum_stmt.query(params![&platform])?;
                while let Some(row) = rows.next()? {
                    let json_str: String = row.get(0)?;
                    if let Ok(keys) = serde_json::from_str::<Vec<String>>(&json_str) {
                        for k in keys {
                            let norm = k.trim().to_ascii_lowercase();
                            if !norm.is_empty() {
                                set.insert(norm);
                            }
                        }
                    }
                }
                summary_cache.insert(platform.clone(), set);
            }

            if summary_cache
                .get(&platform)
                .is_some_and(|keys| keys.contains(&key))
            {
                status.ac = true;
                status.attempted = true;
                return Ok(status);
            }
        }

        Ok(status)
    }

    /// Fast batch lookup of synced status for many problems (for the library view).
    pub fn get_problems_sync_statuses(
        &self,
        items: &[(&str, &[ProblemSource])],
    ) -> Result<BTreeMap<String, SyncedProblemStatus>> {
        let mut out = BTreeMap::new();
        if items.is_empty() {
            return Ok(out);
        }

        let any_sources = items.iter().any(|(_, srcs)| !srcs.is_empty());
        if !any_sources {
            for (id, _) in items {
                out.insert((*id).to_string(), SyncedProblemStatus::default());
            }
            return Ok(out);
        }

        // Single pass over oj_submissions grouped by (lower(platform), lower(problem_key)).
        let mut sub_index: HashMap<(String, String), SyncedProblemStatus> = HashMap::new();
        {
            let mut stmt = self.conn.prepare(
                "SELECT
                    LOWER(platform),
                    LOWER(problem_key),
                    MAX(CASE WHEN UPPER(verdict) IN ('AC', 'OK') THEN 1 ELSE 0 END)
                 FROM oj_submissions
                 GROUP BY LOWER(platform), LOWER(problem_key)",
            )?;
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                let platform: String = row.get(0)?;
                let key: String = row.get(1)?;
                let ac: i64 = row.get(2)?;
                sub_index.insert(
                    (platform, key),
                    SyncedProblemStatus {
                        ac: ac > 0,
                        attempted: true,
                    },
                );
            }
        }

        // Merge oj_summaries.solved_keys_json.
        {
            let mut stmt = self
                .conn
                .prepare("SELECT LOWER(platform), solved_keys_json FROM oj_summaries")?;
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                let platform: String = row.get(0)?;
                let json_str: String = row.get(1)?;
                if let Ok(keys) = serde_json::from_str::<Vec<String>>(&json_str) {
                    for k in keys {
                        let norm = k.trim().to_ascii_lowercase();
                        if !norm.is_empty() {
                            sub_index.insert(
                                (platform.clone(), norm),
                                SyncedProblemStatus {
                                    ac: true,
                                    attempted: true,
                                },
                            );
                        }
                    }
                }
            }
        }

        for (problem_id, sources) in items {
            let mut combined = SyncedProblemStatus::default();
            for src in *sources {
                let platform = src.platform.trim().to_ascii_lowercase();
                let key = src.id.trim().to_ascii_lowercase();
                if platform.is_empty() || key.is_empty() {
                    continue;
                }
                if let Some(st) = sub_index.get(&(platform, key)) {
                    combined.ac |= st.ac;
                    combined.attempted |= st.attempted;
                    if combined.ac {
                        break;
                    }
                }
            }
            out.insert((*problem_id).to_string(), combined);
        }

        Ok(out)
    }

    /// Looks up cached metadata for `(platform, problem_key)` in `oj_problem_meta`,
    /// falling back to the latest `oj_submissions` row if not present.
    pub fn lookup_problem_meta(
        &self,
        platform: &str,
        problem_key: &str,
    ) -> Result<Option<CachedProblemMeta>> {
        let p = platform.trim().to_ascii_lowercase();
        let k = problem_key.trim().to_ascii_lowercase();
        if p.is_empty() || k.is_empty() {
            return Ok(None);
        }

        let meta_row: Option<CachedProblemMeta> = self
            .conn
            .query_row(
                "SELECT platform, problem_key, title, url, raw_rating, normalized_rating, tags_json, updated_at
                 FROM oj_problem_meta
                 WHERE LOWER(platform) = ?1 AND LOWER(problem_key) = ?2
                 LIMIT 1",
                params![&p, &k],
                |row| {
                    let tags_json: String = row.get(6)?;
                    let tags = serde_json::from_str(&tags_json).unwrap_or_default();
                    Ok(CachedProblemMeta {
                        platform: row.get(0)?,
                        problem_key: row.get(1)?,
                        title: row.get(2)?,
                        url: row.get(3)?,
                        raw_rating: row.get(4)?,
                        normalized_rating: row.get(5)?,
                        tags,
                        updated_at: row.get(7)?,
                    })
                },
            )
            .optional()?;

        if meta_row.is_some() {
            return Ok(meta_row);
        }

        let sub_row: Option<CachedProblemMeta> = self
            .conn
            .query_row(
                "SELECT platform, problem_key, problem_title, problem_url, raw_rating, normalized_rating, tags_json, submitted_at
                 FROM oj_submissions
                 WHERE LOWER(platform) = ?1 AND LOWER(problem_key) = ?2
                 ORDER BY submitted_at DESC
                 LIMIT 1",
                params![&p, &k],
                |row| {
                    let tags_json: String = row.get(6)?;
                    let tags = serde_json::from_str(&tags_json).unwrap_or_default();
                    let submitted_at: i64 = row.get(7)?;
                    Ok(CachedProblemMeta {
                        platform: row.get(0)?,
                        problem_key: row.get(1)?,
                        title: row.get(2)?,
                        url: row.get(3)?,
                        raw_rating: row.get(4)?,
                        normalized_rating: row.get(5)?,
                        tags,
                        updated_at: crate::unix_secs_to_rfc3339_utc(submitted_at),
                    })
                },
            )
            .optional()?;

        Ok(sub_row)
    }

    pub fn list_accounts(&self) -> Result<Vec<CachedAccount>> {
        let mut stmt = self.conn.prepare(
            "SELECT platform, handle, rating, max_rating, rank, updated_at
             FROM oj_accounts
             ORDER BY platform ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(CachedAccount {
                platform: row.get(0)?,
                handle: row.get(1)?,
                rating: row.get(2)?,
                max_rating: row.get(3)?,
                rank: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn list_rating_history(&self, platform: Option<&str>) -> Result<Vec<CachedRatingChange>> {
        let mut out = Vec::new();
        if let Some(p) = platform {
            let p_lower = p.trim().to_ascii_lowercase();
            let mut stmt = self.conn.prepare(
                "SELECT platform, contest_id, contest_name, rating_before, rating_after, rank, at
                 FROM oj_rating_history
                 WHERE LOWER(platform) = ?1
                 ORDER BY at ASC, contest_id ASC",
            )?;
            let rows = stmt.query_map(params![p_lower], map_rating_row)?;
            for row in rows {
                out.push(row?);
            }
        } else {
            let mut stmt = self.conn.prepare(
                "SELECT platform, contest_id, contest_name, rating_before, rating_after, rank, at
                 FROM oj_rating_history
                 ORDER BY at ASC, platform ASC, contest_id ASC",
            )?;
            let rows = stmt.query_map([], map_rating_row)?;
            for row in rows {
                out.push(row?);
            }
        }
        Ok(out)
    }

    pub fn list_summaries(&self) -> Result<Vec<CachedSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT platform, handle, solved_count, submission_count, solved_keys_json, rating, updated_at
             FROM oj_summaries
             ORDER BY platform ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            let solved_count: i64 = row.get(2)?;
            let submission_count: Option<i64> = row.get(3)?;
            let solved_keys_json: String = row.get(4)?;
            let solved_keys = serde_json::from_str(&solved_keys_json).unwrap_or_default();
            Ok(CachedSummary {
                platform: row.get(0)?,
                handle: row.get(1)?,
                solved_count: solved_count.max(0) as u64,
                submission_count: submission_count.map(|v| v.max(0) as u64),
                solved_keys,
                rating: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn list_contests(&self) -> Result<Vec<CachedContest>> {
        let mut stmt = self.conn.prepare(
            "SELECT platform, contest_id, name, start_time, duration_sec, url, updated_at
             FROM oj_contests
             ORDER BY start_time ASC, platform ASC, contest_id ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(CachedContest {
                platform: row.get(0)?,
                contest_id: row.get(1)?,
                name: row.get(2)?,
                start_time: row.get(3)?,
                duration_sec: row.get(4)?,
                url: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn list_submissions(&self, platform: Option<&str>) -> Result<Vec<CachedSubmission>> {
        let mut out = Vec::new();
        if let Some(p) = platform {
            let p_lower = p.trim().to_ascii_lowercase();
            let mut stmt = self.conn.prepare(
                "SELECT platform, submission_id, handle, contest_id, problem_index, problem_key,
                        problem_title, problem_url, raw_rating, normalized_rating, tags_json,
                        verdict, language, submitted_at
                 FROM oj_submissions
                 WHERE LOWER(platform) = ?1
                 ORDER BY submitted_at DESC, submission_id DESC",
            )?;
            let rows = stmt.query_map(params![p_lower], map_submission_row)?;
            for row in rows {
                out.push(row?);
            }
        } else {
            let mut stmt = self.conn.prepare(
                "SELECT platform, submission_id, handle, contest_id, problem_index, problem_key,
                        problem_title, problem_url, raw_rating, normalized_rating, tags_json,
                        verdict, language, submitted_at
                 FROM oj_submissions
                 ORDER BY submitted_at DESC, platform ASC, submission_id DESC",
            )?;
            let rows = stmt.query_map([], map_submission_row)?;
            for row in rows {
                out.push(row?);
            }
        }
        Ok(out)
    }
}

fn map_rating_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CachedRatingChange> {
    Ok(CachedRatingChange {
        platform: row.get(0)?,
        contest_id: row.get(1)?,
        contest_name: row.get(2)?,
        rating_before: row.get(3)?,
        rating_after: row.get(4)?,
        rank: row.get(5)?,
        at: row.get(6)?,
    })
}

fn map_submission_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CachedSubmission> {
    let tags_json: String = row.get(10)?;
    let tags = serde_json::from_str(&tags_json).unwrap_or_default();
    Ok(CachedSubmission {
        platform: row.get(0)?,
        submission_id: row.get(1)?,
        handle: row.get(2)?,
        contest_id: row.get(3)?,
        problem_index: row.get(4)?,
        problem_key: row.get(5)?,
        problem_title: row.get(6)?,
        problem_url: row.get(7)?,
        raw_rating: row.get(8)?,
        normalized_rating: row.get(9)?,
        tags,
        verdict: row.get(11)?,
        language: row.get(12)?,
        submitted_at: row.get(13)?,
    })
}
