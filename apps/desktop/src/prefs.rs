//! Persistent desktop preferences and Frecency history stored atomically in
//! `<data_dir>/prefs.json`, plus MCP client config snippet formatting.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use nsh_core::JsonValue;

use crate::types::AppPrefsJs;

/// Half-life for Frecency decay (7 days in seconds), matching `WorkspaceIndex`.
const FRECENCY_HALF_LIFE_SECS: f64 = 7.0 * 24.0 * 3600.0;

/// Maximum number of recent workspaces remembered.
const MAX_RECENT_WORKSPACES: usize = 10;

/// Maximum number of persisted Frecency entries.
const MAX_FRECENCY_ENTRIES: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FrecencyRecord {
    pub(crate) score: f64,
    pub(crate) last_used_secs: u64,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct StoredPrefs {
    pub(crate) app: AppPrefsJs,
    pub(crate) frecency: HashMap<String, FrecencyRecord>,
    /// Reserved Phase 2.5 window placement/state payload preserved verbatim across
    /// `prefs.json` read/write cycles (omitted from JSON when `None`).
    pub(crate) window_state: Option<JsonValue>,
}

impl StoredPrefs {
    /// Loads `<data_dir>/prefs.json`, falling back to legacy `<data_dir>/last-workspace`
    /// if `prefs.json` does not exist yet.
    pub(crate) fn load(data_dir: Option<&Path>) -> Self {
        let Some(dir) = data_dir else {
            return Self::default();
        };
        if let Ok(text) = std::fs::read_to_string(prefs_file(dir))
            && let Some(parsed) = parse_prefs_json(&text)
        {
            return parsed;
        }
        let mut prefs = Self::default();
        if let Some(last) = read_legacy_last_workspace(dir) {
            prefs.remember_workspace(&last);
        }
        prefs
    }

    /// Atomically writes `<data_dir>/prefs.json`.
    pub(crate) fn persist(&self, data_dir: Option<&Path>) {
        let Some(dir) = data_dir else {
            return;
        };
        let _ = std::fs::create_dir_all(dir);
        let json = serialize_prefs_json(self);
        let _ = xcpc_storage::write_atomic(&prefs_file(dir), json.as_bytes());
    }

    /// Prepends `root` to `recent_workspaces`, deduplicating and capping length.
    pub(crate) fn remember_workspace(&mut self, root: &Path) {
        let entry = root.display().to_string();
        if entry.is_empty() {
            return;
        }
        self.app.recent_workspaces.retain(|p| p != &entry);
        self.app.recent_workspaces.insert(0, entry);
        self.app.recent_workspaces.truncate(MAX_RECENT_WORKSPACES);
    }

    /// Returns the most recent workspace path, if any.
    pub(crate) fn latest_workspace(&self) -> Option<PathBuf> {
        self.app.recent_workspaces.first().map(PathBuf::from)
    }

    /// Updates user-visible preferences while preserving persisted Frecency records.
    pub(crate) fn apply_app_prefs(&mut self, mut next: AppPrefsJs) {
        if !next.split_ratio.is_finite() {
            next.split_ratio = 0.5;
        } else {
            next.split_ratio = next.split_ratio.clamp(0.1, 0.9);
        }
        if next.last_space.is_empty() {
            next.last_space = "problems".to_owned();
        }
        if next.last_book != "editorial" {
            next.last_book = "statements".to_owned();
        }
        next.language = normalize_language_pref(next.language.as_deref());
        next.cxx_override = normalize_cxx_override(next.cxx_override.as_deref());
        next.library_group_by = normalize_library_group_by(next.library_group_by.as_deref());
        next.library_checker_mirror_url =
            normalize_cxx_override(next.library_checker_mirror_url.as_deref());
        next.library_checker_local_repo_path =
            normalize_cxx_override(next.library_checker_local_repo_path.as_deref());
        next.recent_workspaces = dedup_recent(next.recent_workspaces);
        self.app = next;
    }

    /// Updates the persisted C++ compiler path override (empty string clears it).
    pub(crate) fn set_cxx_override(&mut self, path: Option<&str>) {
        self.app.cxx_override = normalize_cxx_override(path);
    }

    /// Updates the persisted Library Checker mirror URL and local repo path.
    pub(crate) fn set_library_checker_config(
        &mut self,
        mirror_url: Option<&str>,
        local_repo_path: Option<&str>,
    ) {
        self.app.library_checker_mirror_url = normalize_cxx_override(mirror_url);
        self.app.library_checker_local_repo_path = normalize_cxx_override(local_repo_path);
    }

    /// Records a visit to `key` at the current wall-clock time.
    pub(crate) fn record_visit(&mut self, key: &str) {
        self.record_visit_at(key, now_epoch_secs());
    }

    /// Records a visit to `key` at an explicit Unix timestamp (for deterministic tests).
    pub(crate) fn record_visit_at(&mut self, key: &str, now_secs: u64) {
        if key.is_empty() {
            return;
        }
        let decayed = self
            .frecency
            .get(key)
            .map(|r| decayed_frecency(*r, now_secs))
            .unwrap_or(0.0);
        self.frecency.insert(
            key.to_owned(),
            FrecencyRecord {
                score: decayed + 1.0,
                last_used_secs: now_secs,
            },
        );
        if self.frecency.len() > MAX_FRECENCY_ENTRIES {
            let mut ranked: Vec<(String, f64)> = self
                .frecency
                .iter()
                .map(|(k, r)| (k.clone(), decayed_frecency(*r, now_secs)))
                .collect();
            ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
            ranked.truncate(MAX_FRECENCY_ENTRIES);
            let keep: std::collections::HashSet<String> =
                ranked.into_iter().map(|(k, _)| k).collect();
            self.frecency.retain(|k, _| keep.contains(k));
        }
    }

    /// Computes the Frecency score boost for `key` at `now_secs`, using the same
    /// bounded logarithmic curve as `WorkspaceIndex`.
    pub(crate) fn frecency_boost(&self, key: &str, now_secs: u64) -> f64 {
        let Some(rec) = self.frecency.get(key) else {
            return 0.0;
        };
        let f = decayed_frecency(*rec, now_secs);
        12.0 * (1.0 + f).ln()
    }
}

pub(crate) fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn decayed_frecency(rec: FrecencyRecord, now_secs: u64) -> f64 {
    let elapsed = now_secs.saturating_sub(rec.last_used_secs) as f64;
    rec.score * 0.5_f64.powf(elapsed / FRECENCY_HALF_LIFE_SECS)
}

fn prefs_file(data_dir: &Path) -> PathBuf {
    data_dir.join("prefs.json")
}

/// Cold-start preview snapshot directory (`<dataDir>/snapshots`).
pub(crate) fn snapshots_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("snapshots")
}

fn read_legacy_last_workspace(data_dir: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(data_dir.join("last-workspace")).ok()?;
    let trimmed = text.trim_end_matches(['\r', '\n']);
    (!trimmed.is_empty()).then(|| PathBuf::from(trimmed))
}

fn normalize_language_pref(raw: Option<&str>) -> Option<String> {
    match raw.map(str::trim) {
        Some("system") => Some("system".to_owned()),
        Some("zh-CN") => Some("zh-CN".to_owned()),
        Some("en") => Some("en".to_owned()),
        _ => None,
    }
}

fn normalize_cxx_override(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn normalize_library_group_by(raw: Option<&str>) -> Option<String> {
    match raw.map(str::trim) {
        Some("none") => Some("none".to_owned()),
        Some("group") => Some("group".to_owned()),
        Some("rating") => Some("rating".to_owned()),
        Some("platform") => Some("platform".to_owned()),
        Some("contest") => Some("contest".to_owned()),
        Some("package_status") => Some("package_status".to_owned()),
        _ => None,
    }
}

fn dedup_recent(list: Vec<String>) -> Vec<String> {
    let mut out = Vec::with_capacity(list.len().min(MAX_RECENT_WORKSPACES));
    for item in list {
        if !item.is_empty() && !out.contains(&item) {
            out.push(item);
            if out.len() >= MAX_RECENT_WORKSPACES {
                break;
            }
        }
    }
    out
}

fn parse_prefs_json(text: &str) -> Option<StoredPrefs> {
    let val: JsonValue = text.parse().ok()?;
    let obj = val.as_object()?;
    let mut app = AppPrefsJs::default();

    if let Some(arr) = obj.get("recentWorkspaces").and_then(JsonValue::as_array) {
        let list = arr
            .iter()
            .filter_map(JsonValue::as_str)
            .map(str::to_owned)
            .collect();
        app.recent_workspaces = dedup_recent(list);
    }
    if let Some(ratio) = obj.get("splitRatio").and_then(JsonValue::as_f64)
        && ratio.is_finite()
    {
        app.split_ratio = ratio.clamp(0.1, 0.9);
    }
    if let Some(space) = obj.get("lastSpace").and_then(JsonValue::as_str)
        && !space.is_empty()
    {
        app.last_space = space.to_owned();
    }
    app.last_target = obj
        .get("lastTarget")
        .and_then(JsonValue::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    app.last_id = obj
        .get("lastId")
        .and_then(JsonValue::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    app.last_label = obj
        .get("lastLabel")
        .and_then(JsonValue::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    if let Some(book) = obj.get("lastBook").and_then(JsonValue::as_str) {
        app.last_book = if book == "editorial" {
            "editorial".to_owned()
        } else {
            "statements".to_owned()
        };
    }
    if let Some(props_open) = obj.get("propsOpen").and_then(JsonValue::as_bool) {
        app.props_open = props_open;
    }
    app.language = normalize_language_pref(obj.get("language").and_then(JsonValue::as_str));
    app.cxx_override = normalize_cxx_override(obj.get("cxxOverride").and_then(JsonValue::as_str));
    if let Some(open) = obj.get("librarySidebarOpen").and_then(JsonValue::as_bool) {
        app.library_sidebar_open = open;
    }
    app.library_group_by =
        normalize_library_group_by(obj.get("libraryGroupBy").and_then(JsonValue::as_str));
    app.library_checker_mirror_url = normalize_cxx_override(
        obj.get("libraryCheckerMirrorUrl")
            .and_then(JsonValue::as_str),
    );
    app.library_checker_local_repo_path = normalize_cxx_override(
        obj.get("libraryCheckerLocalRepoPath")
            .and_then(JsonValue::as_str),
    );

    let mut frecency = HashMap::new();
    if let Some(frec_obj) = obj.get("frecency").and_then(JsonValue::as_object) {
        for (k, entry) in frec_obj {
            let Some(score) = entry.get("score").and_then(JsonValue::as_f64) else {
                continue;
            };
            let Some(last_used_secs) = entry.get("lastUsedSecs").and_then(JsonValue::as_u64) else {
                continue;
            };
            if score.is_finite() && score > 0.0 {
                frecency.insert(
                    k.clone(),
                    FrecencyRecord {
                        score,
                        last_used_secs,
                    },
                );
            }
        }
    }

    let window_state = obj.get("windowState").filter(|v| !v.is_null()).cloned();

    Some(StoredPrefs {
        app,
        frecency,
        window_state,
    })
}

fn empty_json_object() -> JsonValue {
    "{}".parse().expect("valid empty JSON object")
}

fn serialize_prefs_json(prefs: &StoredPrefs) -> String {
    let mut root = empty_json_object();
    root["recentWorkspaces"] = JsonValue::Array(
        prefs
            .app
            .recent_workspaces
            .iter()
            .cloned()
            .map(JsonValue::String)
            .collect(),
    );
    root["splitRatio"] = JsonValue::from(prefs.app.split_ratio);
    root["lastSpace"] = JsonValue::from(prefs.app.last_space.as_str());
    if let Some(ref target) = prefs.app.last_target {
        root["lastTarget"] = JsonValue::from(target.as_str());
    }
    if let Some(ref id) = prefs.app.last_id {
        root["lastId"] = JsonValue::from(id.as_str());
    }
    if let Some(ref label) = prefs.app.last_label {
        root["lastLabel"] = JsonValue::from(label.as_str());
    }
    root["lastBook"] = JsonValue::from(prefs.app.last_book.as_str());
    root["propsOpen"] = JsonValue::from(prefs.app.props_open);
    if let Some(ref lang) = prefs.app.language {
        root["language"] = JsonValue::from(lang.as_str());
    }
    if let Some(ref cxx) = prefs.app.cxx_override {
        root["cxxOverride"] = JsonValue::from(cxx.as_str());
    }
    root["librarySidebarOpen"] = JsonValue::from(prefs.app.library_sidebar_open);
    if let Some(ref group_by) = prefs.app.library_group_by {
        root["libraryGroupBy"] = JsonValue::from(group_by.as_str());
    }
    if let Some(ref mirror) = prefs.app.library_checker_mirror_url {
        root["libraryCheckerMirrorUrl"] = JsonValue::from(mirror.as_str());
    }
    if let Some(ref repo) = prefs.app.library_checker_local_repo_path {
        root["libraryCheckerLocalRepoPath"] = JsonValue::from(repo.as_str());
    }
    if let Some(ref window_state) = prefs.window_state {
        root["windowState"] = window_state.clone();
    }

    let mut frec = empty_json_object();
    let mut keys: Vec<&String> = prefs.frecency.keys().collect();
    keys.sort();
    for k in keys {
        let rec = prefs.frecency[k];
        let mut entry = empty_json_object();
        entry["score"] = JsonValue::from(rec.score);
        entry["lastUsedSecs"] = JsonValue::from(rec.last_used_secs);
        frec[k.as_str()] = entry;
    }
    root["frecency"] = frec;

    format!("{root:#}\n")
}

/// Generates a ready-to-paste MCP client JSON configuration snippet pointing at `root`.
pub(crate) fn mcp_config_snippet(root: Option<&Path>) -> String {
    let mut server = empty_json_object();
    server["command"] = JsonValue::from("xcpc");
    let args = match root {
        Some(dir) => vec![
            JsonValue::from("-w"),
            JsonValue::from(dir.display().to_string()),
            JsonValue::from("mcp"),
        ],
        None => vec![JsonValue::from("mcp")],
    };
    server["args"] = JsonValue::Array(args);

    let mut servers = empty_json_object();
    servers["xcpc"] = server;

    let mut root_obj = empty_json_object();
    root_obj["mcpServers"] = servers;
    format!("{root_obj:#}")
}
