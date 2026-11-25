//! Phase 5 MCP tools for Living TRD, algorithm snippets, C++ bundler, and verification.
//!
//! Security boundary:
//! - `bundle_code` is strictly sandboxed to workspace paths or snippet slugs (never arbitrary host paths).
//! - `verify_snippet` runs verification and returns structured results + timings; never edits source files.
//! - Write tools (`write_snippet`, `compose_trd`, `freeze_trd`) are strictly `--read-only` gated.
//! - Unfreezing a TRD is NEVER exposed over MCP (human-only via GUI or `xcpc trd unfreeze`).

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};
use xcpc_convert::publish;
use xcpc_domain::{
    BundleOptions, Patch, SnippetMeta, SnippetMetaPatch, SnippetVerifySpec, SnippetVerifyStatus,
    TrdChapter, TrdMetaPatch,
};
use xcpc_judge::SnippetVerifyOptions;

use crate::mcp::{
    Server, ToolErr, fail, opt_bool, opt_str, opt_str_vec, reject_unknown_keys, req_str, structured,
};

impl Server {
    pub(crate) fn tool_search_snippets(
        &mut self,
        args: &Map<String, Value>,
    ) -> Result<Value, ToolErr> {
        reject_unknown_keys(
            args,
            &["query", "tags", "status", "formula_only"],
            "search_snippets",
        )
        .map_err(|m| fail("invalid_arguments", m))?;
        let query = opt_str(args, "query")
            .map_err(|m| fail("invalid_arguments", m))?
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty());
        let tags = opt_str_vec(args, "tags").map_err(|m| fail("invalid_arguments", m))?;
        let status_filter = opt_str(args, "status").map_err(|m| fail("invalid_arguments", m))?;
        let formula_only =
            opt_bool(args, "formula_only").map_err(|m| fail("invalid_arguments", m))?;

        let want_status = match status_filter.as_deref() {
            None => None,
            Some("unverified") => Some(SnippetVerifyStatus::Unverified),
            Some("passed") => Some(SnippetVerifyStatus::Passed),
            Some("failed") => Some(SnippetVerifyStatus::Failed),
            Some("stale") => Some(SnippetVerifyStatus::Stale),
            Some(other) => {
                return Err(fail(
                    "invalid_arguments",
                    format!("unknown status {other:?} (expected unverified|passed|failed|stale)"),
                ));
            }
        };

        let summaries = self
            .ws
            .list_snippet_summaries()
            .map_err(|e| fail(e.code(), e.to_string()))?;
        let statuses = xcpc_judge::all_snippet_verify_statuses(&self.ws).unwrap_or_default();

        let items: Vec<Value> = summaries
            .into_iter()
            .filter(|s| {
                if formula_only == Some(true) && s.has_code {
                    return false;
                }
                if !tags.iter().all(|t| s.tags.iter().any(|st| st == t)) {
                    return false;
                }
                let st = statuses
                    .get(&s.slug)
                    .copied()
                    .unwrap_or(SnippetVerifyStatus::Unverified);
                if let Some(want) = want_status
                    && st != want
                {
                    return false;
                }
                if let Some(q) = &query {
                    let hay = format!(
                        "{} {} {} {}",
                        s.slug,
                        s.title,
                        s.tags.join(" "),
                        s.description.as_deref().unwrap_or("")
                    )
                    .to_lowercase();
                    if !hay.contains(q) {
                        return false;
                    }
                }
                true
            })
            .map(|s| {
                let st = statuses
                    .get(&s.slug)
                    .copied()
                    .unwrap_or(SnippetVerifyStatus::Unverified);
                let mut val = serde_json::to_value(&s).expect("serializable");
                if let Some(obj) = val.as_object_mut() {
                    obj.insert("verify_status".into(), json!(st.as_str()));
                }
                val
            })
            .collect();

        Ok(structured(json!({
            "ok": true,
            "total": items.len(),
            "snippets": items,
        })))
    }

    pub(crate) fn tool_get_snippet(&mut self, args: &Map<String, Value>) -> Result<Value, ToolErr> {
        reject_unknown_keys(args, &["slug"], "get_snippet")
            .map_err(|m| fail("invalid_arguments", m))?;
        let slug = req_str(args, "slug").map_err(|m| fail("invalid_arguments", m))?;
        let detail = self
            .ws
            .load_snippet(&slug)
            .map_err(|e| fail(e.code(), e.to_string()))?;
        let verify_status = xcpc_judge::snippet_verify_status(&self.ws, &slug);
        let verify_report = xcpc_judge::load_snippet_verify_report(&self.ws, &slug);
        Ok(structured(json!({
            "ok": true,
            "snippet": detail,
            "verify_status": verify_status.as_str(),
            "verify_report": verify_report,
        })))
    }

    pub(crate) fn tool_get_trd(&mut self, args: &Map<String, Value>) -> Result<Value, ToolErr> {
        reject_unknown_keys(args, &["id"], "get_trd").map_err(|m| fail("invalid_arguments", m))?;
        let id = opt_str(args, "id").map_err(|m| fail("invalid_arguments", m))?;
        let Some(id) = id else {
            let list = self
                .ws
                .list_trd_summaries()
                .map_err(|e| fail(e.code(), e.to_string()))?;
            return Ok(structured(json!({
                "ok": true,
                "trds": list,
            })));
        };
        let trd = self
            .ws
            .load_trd(&id)
            .map_err(|e| fail(e.code(), e.to_string()))?;
        let statuses = xcpc_judge::all_snippet_verify_statuses(&self.ws).unwrap_or_default();
        let preflight = if trd.meta.frozen {
            Vec::new()
        } else {
            self.ws
                .preflight_trd(&id, &statuses)
                .map_err(|e| fail(e.code(), e.to_string()))?
        };
        Ok(structured(json!({
            "ok": true,
            "trd": trd,
            "preflight": preflight,
        })))
    }

    pub(crate) fn tool_bundle_code(&mut self, args: &Map<String, Value>) -> Result<Value, ToolErr> {
        reject_unknown_keys(
            args,
            &["slug", "path", "source", "include_dirs", "compact"],
            "bundle_code",
        )
        .map_err(|m| fail("invalid_arguments", m))?;

        let slug = opt_str(args, "slug").map_err(|m| fail("invalid_arguments", m))?;
        let path = opt_str(args, "path").map_err(|m| fail("invalid_arguments", m))?;
        let source = opt_str(args, "source").map_err(|m| fail("invalid_arguments", m))?;
        let include_dirs =
            opt_str_vec(args, "include_dirs").map_err(|m| fail("invalid_arguments", m))?;
        let compact = opt_bool(args, "compact")
            .map_err(|m| fail("invalid_arguments", m))?
            .unwrap_or(false);

        let provided = slug.is_some() as u8 + path.is_some() as u8 + source.is_some() as u8;
        if provided != 1 {
            return Err(fail(
                "invalid_arguments",
                "specify exactly one of `slug`, `path`, or `source`",
            ));
        }

        let opts = BundleOptions {
            include_dirs,
            compact,
            include_boundary_comments: !compact,
        };

        let result = if let Some(s) = slug {
            self.ws
                .bundle_snippet(&s, &opts)
                .map_err(|e| fail(e.code(), e.to_string()))?
        } else {
            self.ws
                .bundle_workspace_code(path.as_deref(), source.as_deref(), &opts)
                .map_err(|e| fail(e.code(), e.to_string()))?
        };

        Ok(structured(json!({
            "ok": true,
            "result": result,
        })))
    }

    pub(crate) fn tool_verify_snippet(
        &mut self,
        args: &Map<String, Value>,
    ) -> Result<Value, ToolErr> {
        reject_unknown_keys(
            args,
            &["slug", "cascade", "all", "force", "jobs"],
            "verify_snippet",
        )
        .map_err(|m| fail("invalid_arguments", m))?;

        let slug = opt_str(args, "slug").map_err(|m| fail("invalid_arguments", m))?;
        let cascade = opt_bool(args, "cascade")
            .map_err(|m| fail("invalid_arguments", m))?
            .unwrap_or(false);
        let all = opt_bool(args, "all")
            .map_err(|m| fail("invalid_arguments", m))?
            .unwrap_or(false);
        let force = opt_bool(args, "force")
            .map_err(|m| fail("invalid_arguments", m))?
            .unwrap_or(false);
        let jobs = match args.get("jobs") {
            None | Some(Value::Null) => None,
            Some(v) => Some(
                v.as_u64()
                    .filter(|&n| n >= 1)
                    .ok_or_else(|| fail("invalid_arguments", "`jobs` must be a positive integer"))?
                    as usize,
            ),
        };

        if !all && slug.is_none() {
            return Err(fail(
                "invalid_arguments",
                "provide `slug` or set `all: true`",
            ));
        }

        let opts = SnippetVerifyOptions {
            jobs,
            force,
            ..Default::default()
        };

        let reports = if all {
            xcpc_judge::verify_all_snippets(&self.ws, &opts)
                .map_err(|e| fail(e.code(), e.to_string()))?
        } else {
            let s = slug.unwrap();
            if cascade {
                xcpc_judge::verify_snippet_cascade(&self.ws, &s, &opts)
                    .map_err(|e| fail(e.code(), e.to_string()))?
            } else {
                vec![
                    xcpc_judge::verify_snippet(&self.ws, &s, &opts)
                        .map_err(|e| fail(e.code(), e.to_string()))?,
                ]
            }
        };

        let ok = reports
            .iter()
            .all(|r| r.status != SnippetVerifyStatus::Failed);
        Ok(structured(json!({
            "ok": ok,
            "reports": reports,
        })))
    }

    pub(crate) fn tool_write_snippet(
        &mut self,
        args: &Map<String, Value>,
    ) -> Result<Value, ToolErr> {
        reject_unknown_keys(
            args,
            &[
                "slug",
                "title",
                "tags",
                "time_complexity",
                "space_complexity",
                "description",
                "source_problem",
                "verify",
                "files",
            ],
            "write_snippet",
        )
        .map_err(|m| fail("invalid_arguments", m))?;

        if self.ws.is_read_only() {
            return Err(fail("read_only", "workspace is read-only"));
        }

        let slug = req_str(args, "slug").map_err(|m| fail("invalid_arguments", m))?;
        let title = opt_str(args, "title").map_err(|m| fail("invalid_arguments", m))?;
        let tags = match args.get("tags") {
            None | Some(Value::Null) => None,
            Some(_) => Some(opt_str_vec(args, "tags").map_err(|m| fail("invalid_arguments", m))?),
        };
        let time_complexity =
            opt_str(args, "time_complexity").map_err(|m| fail("invalid_arguments", m))?;
        let space_complexity =
            opt_str(args, "space_complexity").map_err(|m| fail("invalid_arguments", m))?;
        let description = opt_str(args, "description").map_err(|m| fail("invalid_arguments", m))?;
        let source_problem =
            opt_str(args, "source_problem").map_err(|m| fail("invalid_arguments", m))?;
        let verify: Option<Vec<SnippetVerifySpec>> =
            match args.get("verify") {
                None | Some(Value::Null) => None,
                Some(v) => Some(serde_json::from_value(v.clone()).map_err(|e| {
                    fail("invalid_arguments", format!("invalid `verify` array: {e}"))
                })?),
            };

        let mut files = BTreeMap::new();
        if let Some(files_val) = args.get("files")
            && !files_val.is_null()
        {
            let Some(obj) = files_val.as_object() else {
                return Err(fail(
                    "invalid_arguments",
                    "`files` must be an object mapping filename to string content",
                ));
            };
            for (k, v) in obj {
                let Some(text) = v.as_str() else {
                    return Err(fail(
                        "invalid_arguments",
                        format!("`files.{k}` must be a string"),
                    ));
                };
                files.insert(k.clone(), text.to_owned());
            }
        }

        let exists = self.ws.load_snippet(&slug).is_ok();
        if !exists && !files.contains_key("snippet.toml") {
            let meta = SnippetMeta {
                title: title.clone().unwrap_or_else(|| slug.clone()),
                tags: tags.clone().unwrap_or_default(),
                time_complexity: time_complexity.clone(),
                space_complexity: space_complexity.clone(),
                description: description.clone(),
                source_problem: source_problem.clone(),
                verify: verify.clone().unwrap_or_default(),
            };
            self.ws
                .create_snippet(&slug, &meta, None, None)
                .map_err(|e| fail(e.code(), e.to_string()))?;
        } else if exists {
            let patch = SnippetMetaPatch {
                title,
                tags,
                time_complexity: time_complexity.map_or(Patch::Keep, Patch::Set),
                space_complexity: space_complexity.map_or(Patch::Keep, Patch::Set),
                description: description.map_or(Patch::Keep, Patch::Set),
                source_problem: source_problem.map_or(Patch::Keep, Patch::Set),
                verify,
            };
            if patch != SnippetMetaPatch::default() {
                self.ws
                    .update_snippet_meta(&slug, &patch)
                    .map_err(|e| fail(e.code(), e.to_string()))?;
            }
        }

        let detail = if !files.is_empty() {
            self.ws
                .write_snippet_files(&slug, &files)
                .map_err(|e| fail(e.code(), e.to_string()))?
        } else {
            self.ws
                .load_snippet(&slug)
                .map_err(|e| fail(e.code(), e.to_string()))?
        };

        Ok(structured(json!({
            "ok": true,
            "snippet": detail,
        })))
    }

    pub(crate) fn tool_compose_trd(&mut self, args: &Map<String, Value>) -> Result<Value, ToolErr> {
        reject_unknown_keys(
            args,
            &["id", "chapters", "name", "author", "date", "template"],
            "compose_trd",
        )
        .map_err(|m| fail("invalid_arguments", m))?;

        if self.ws.is_read_only() {
            return Err(fail("read_only", "workspace is read-only"));
        }

        let id = req_str(args, "id").map_err(|m| fail("invalid_arguments", m))?;
        let chapters_val = args
            .get("chapters")
            .ok_or_else(|| fail("invalid_arguments", "`chapters` is required"))?;
        let chapters: Vec<TrdChapter> = serde_json::from_value(chapters_val.clone())
            .map_err(|e| fail("invalid_arguments", format!("invalid `chapters`: {e}")))?;

        let patch = TrdMetaPatch {
            name: opt_str(args, "name").map_err(|m| fail("invalid_arguments", m))?,
            author: opt_str(args, "author")
                .map_err(|m| fail("invalid_arguments", m))?
                .map_or(Patch::Keep, Patch::Set),
            date: opt_str(args, "date")
                .map_err(|m| fail("invalid_arguments", m))?
                .map_or(Patch::Keep, Patch::Set),
            template: opt_str(args, "template")
                .map_err(|m| fail("invalid_arguments", m))?
                .map_or(Patch::Keep, Patch::Set),
            ..Default::default()
        };

        let detail = self
            .ws
            .compose_trd(&id, &chapters, Some(&patch))
            .map_err(|e| fail(e.code(), e.to_string()))?;
        let statuses = xcpc_judge::all_snippet_verify_statuses(&self.ws).unwrap_or_default();
        let preflight = self
            .ws
            .preflight_trd(&id, &statuses)
            .map_err(|e| fail(e.code(), e.to_string()))?;

        Ok(structured(json!({
            "ok": true,
            "trd": detail,
            "preflight": preflight,
        })))
    }

    pub(crate) fn tool_freeze_trd(&mut self, args: &Map<String, Value>) -> Result<Value, ToolErr> {
        reject_unknown_keys(args, &["id", "strict"], "freeze_trd")
            .map_err(|m| fail("invalid_arguments", m))?;

        if self.ws.is_read_only() {
            return Err(fail("read_only", "workspace is read-only"));
        }

        let id = req_str(args, "id").map_err(|m| fail("invalid_arguments", m))?;
        let strict = opt_bool(args, "strict")
            .map_err(|m| fail("invalid_arguments", m))?
            .unwrap_or(false);
        let statuses = xcpc_judge::all_snippet_verify_statuses(&self.ws).unwrap_or_default();

        let (result, report) =
            publish::freeze_trd_with_pdf(&self.ws, &mut self.engine, &id, strict, &statuses)
                .map_err(|e| fail(e.code(), e.to_string()))?;

        Ok(structured(json!({
            "ok": result.is_some() && report.ok,
            "frozen": result.is_some(),
            "result": result,
            "report": report,
        })))
    }
}

pub(crate) fn trd_tools() -> Value {
    json!([
        {
            "name": "search_snippets",
            "title": "Search algorithm snippets",
            "description": "Search workspace algorithm snippets (`snippets/<slug>/`) by optional text, tags, verification status, or pure formula card filter.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Case-insensitive substring matched against slug, title, tags, and description." },
                    "tags": { "type": "array", "items": { "type": "string" }, "description": "Required tags (snippet must contain all listed tags)." },
                    "status": { "type": "string", "enum": ["unverified", "passed", "failed", "stale"], "description": "Filter by derived verification status." },
                    "formula_only": { "type": "boolean", "description": "When true, only return pure formula/reference cards without `code.hpp`." },
                },
                "additionalProperties": false,
            },
            "annotations": { "readOnlyHint": true },
        },
        {
            "name": "get_snippet",
            "title": "Get algorithm snippet detail",
            "description": "Return full snippet metadata, `code.hpp`, `notes.md`, `test.cpp`, verify adapters, dependency DAG & reverse references (`deps`, `transitive_deps`, `reverse_deps`, `related_problems`), KACTL++ cumulative hashes, and verification status/report.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "slug": { "type": "string", "description": "Snippet slug under `snippets/<slug>/`." },
                },
                "required": ["slug"],
                "additionalProperties": false,
            },
            "annotations": { "readOnlyHint": true },
        },
        {
            "name": "get_trd",
            "title": "Get TRD detail or list",
            "description": "List all Team Reference Documents (`trds/`) when `id` is omitted, or return one TRD's metadata, chapters, frozen state, and preflight issues.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Optional TRD ID under `trds/<id>/`." },
                },
                "additionalProperties": false,
            },
            "annotations": { "readOnlyHint": true },
        },
        {
            "name": "bundle_code",
            "title": "Bundle C++ code into a single file",
            "description": "Recursively inline local `#include \"...\"` directives into a single self-contained C++ file (topologically ordered, deduplicating system headers). Strictly sandboxed to workspace paths or snippet slugs.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "slug": { "type": "string", "description": "Workspace snippet slug to expand with all transitive dependencies." },
                    "path": { "type": "string", "description": "Workspace-relative C++ source file path to expand (sandboxed inside workspace)." },
                    "source": { "type": "string", "description": "Inline C++ source text to expand against workspace `snippets/` and sandboxed `include_dirs`." },
                    "include_dirs": { "type": "array", "items": { "type": "string" }, "description": "Additional workspace-relative `#include` search directories." },
                    "compact": { "type": "boolean", "description": "When true, strip C/C++ comments and collapse extra blank lines." },
                },
                "additionalProperties": false,
            },
            "annotations": { "readOnlyHint": true },
        },
        {
            "name": "verify_snippet",
            "title": "Verify algorithm snippet(s)",
            "description": "Compile and run snippet verification specs (`self`, `problem`, `library_checker`) for one snippet, its downstream dependents (`cascade: true`), or all snippets (`all: true`). Returns structured results and timings; never edits source files.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "slug": { "type": "string", "description": "Snippet slug to verify (required unless `all: true`)." },
                    "cascade": { "type": "boolean", "description": "Also verify all downstream snippets depending on `slug` in topological order." },
                    "all": { "type": "boolean", "description": "Verify all snippets in the workspace." },
                    "force": { "type": "boolean", "description": "Ignore cached verification reports and re-run." },
                    "jobs": { "type": "integer", "minimum": 1, "description": "Optional parallel worker count." },
                },
                "additionalProperties": false,
            },
            "annotations": { "readOnlyHint": true },
        },
        {
            "name": "write_snippet",
            "title": "Create or update algorithm snippet",
            "description": "Atomically create or update a snippet under `snippets/<slug>/` (`snippet.toml` metadata and whitelisted files `code.hpp`, `notes.md`, `test.cpp`, `verify_*.cpp`). Refused under `--read-only`.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "slug": { "type": "string", "description": "Immutable snippet slug under `snippets/<slug>/`." },
                    "title": { "type": "string", "description": "Display title." },
                    "tags": { "type": "array", "items": { "type": "string" }, "description": "Snippet tags." },
                    "time_complexity": { "type": "string", "description": "Time complexity (e.g. `O(n log n)`)." },
                    "space_complexity": { "type": "string", "description": "Space complexity (e.g. `O(n)`)." },
                    "description": { "type": "string", "description": "Short description." },
                    "source_problem": { "type": "string", "description": "Optional workspace problem slug this snippet was extracted from." },
                    "verify": { "type": "array", "items": { "type": "object" }, "description": "Verification specs (`[[verify]]`: `self`, `problem`, or `library_checker`)." },
                    "files": { "type": "object", "additionalProperties": { "type": "string" }, "description": "Map of filename (`code.hpp`, `notes.md`, `test.cpp`, `verify_*.cpp`, `snippet.toml`) to UTF-8 text." },
                },
                "required": ["slug"],
                "additionalProperties": false,
            },
            "annotations": { "readOnlyHint": false, "destructiveHint": false, "idempotentHint": true },
        },
        {
            "name": "compose_trd",
            "title": "Compose or update a Living TRD",
            "description": "Create or replace a TRD's chapters and optional metadata (`trds/<id>/trd.toml`) and return preflight findings. Refused if the TRD is frozen or under `--read-only`.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "TRD ID under `trds/<id>/`." },
                    "chapters": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "title": { "type": "string" },
                                "items": { "type": "array", "items": { "type": "string" } },
                            },
                            "required": ["title", "items"],
                        },
                        "description": "Ordered chapters referencing snippet slugs.",
                    },
                    "name": { "type": "string", "description": "TRD title." },
                    "author": { "type": "string", "description": "Team or author name." },
                    "date": { "type": "string", "description": "Date string." },
                    "template": { "type": "string", "description": "TRD template ID (`trd-compact`, `trd-book`, or custom)." },
                },
                "required": ["id", "chapters"],
                "additionalProperties": false,
            },
            "annotations": { "readOnlyHint": false, "destructiveHint": false, "idempotentHint": true },
        },
        {
            "name": "freeze_trd",
            "title": "Freeze a TRD snapshot",
            "description": "Validate, compile PDF, archive snippet sources & effective template into `trds/<id>/snapshot/`, and mark the TRD read-only (`frozen = true`). Soft warnings allow freeze by default; pass `strict: true` to require all code snippets verified. Unfreezing is human-only and never exposed via MCP.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "TRD ID to freeze." },
                    "strict": { "type": "boolean", "description": "When true, block freeze if any code snippet is unverified, stale, or failed." },
                },
                "required": ["id"],
                "additionalProperties": false,
            },
            "annotations": { "readOnlyHint": false, "destructiveHint": false, "idempotentHint": false },
        },
    ])
}
