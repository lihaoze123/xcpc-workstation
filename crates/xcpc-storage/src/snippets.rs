//! Snippets & TRD (Team Reference Document) persistence, reverse index, preflight,
//! freeze/unfreeze snapshots, and workspace settings.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::Path;

use toml_edit::{Array, ArrayOfTables, DocumentMut, Item, Table, Value};
use xcpc_domain::{
    BundleOptions, BundleResult, Patch, SnippetDetail, SnippetMeta, SnippetMetaPatch,
    SnippetReverseRefs, SnippetSummary, SnippetVerifySpec, SnippetVerifyStatus,
    TemplateOptionValue, TemplateOptionsPatch, TextFile, TrdChapter, TrdDetail, TrdFreezeResult,
    TrdMeta, TrdMetaPatch, TrdPreflightIssue, TrdPreflightKind, TrdPreflightSeverity,
    TrdSnippetDiffKind, TrdSummary, TrdUnfreezeDiff, TrdUnfreezeSnippetDiff, WorkspaceMeta,
    check_id,
};

use crate::bundler::{
    SnippetDag, bundle_local_file, bundle_local_source, bundle_snippet, bundle_workspace_code,
    parse_snippet_deps_from_code,
};
use crate::code_hash::preprocess_printed_code;
use crate::{Error, Result, SnapshotTemplateArchive, WORKSPACE_FILE, Workspace, io, parse_toml};

/// Default cumulative hash interval (lines) used when generating [`PrintedSnippetCode`]
/// for snippet previews.
pub const DEFAULT_HASH_INTERVAL: usize = 10;

/// Allowed files inside `snippets/<slug>/` for [`Workspace::write_snippet_files`].
fn is_allowed_snippet_file(name: &str) -> bool {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.starts_with('.')
        || name.contains(['/', '\\', '\0'])
        || name.chars().any(char::is_control)
    {
        return false;
    }
    matches!(name, "snippet.toml" | "code.hpp" | "notes.md" | "test.cpp")
        || name.ends_with(".cpp")
        || name.ends_with(".hpp")
        || name.ends_with(".h")
}

impl Workspace {
    // ---- Workspace-level snippets setting ----------------------------------------------

    /// Loads `workspace.toml` as [`WorkspaceMeta`] (defaulting all fields if missing).
    pub fn load_workspace_meta(&self) -> Result<WorkspaceMeta> {
        match self.read_text(WORKSPACE_FILE) {
            Ok(raw) => parse_toml(WORKSPACE_FILE, &raw),
            Err(Error::NotFound(_)) => Ok(WorkspaceMeta::default()),
            Err(e) => Err(e),
        }
    }

    /// Reads the optional `snippets = <bool>` setting from `workspace.toml`, defaulting
    /// to `true` when absent.
    pub fn snippets_enabled(&self) -> bool {
        let Ok(raw) = fs::read_to_string(self.root.join(WORKSPACE_FILE)) else {
            return true;
        };
        let Ok(table) = toml::from_str::<toml::Table>(&raw) else {
            return true;
        };
        table
            .get("snippets")
            .and_then(toml::Value::as_bool)
            .unwrap_or(true)
    }

    /// Updates `snippets = <bool>` in `workspace.toml` via `toml_edit`, preserving comments,
    /// key order, and unknown fields.
    pub fn set_snippets_enabled(&self, enabled: bool) -> Result<()> {
        self.check_writable(WORKSPACE_FILE)?;
        let raw = match self.read_text(WORKSPACE_FILE) {
            Ok(s) => s,
            Err(Error::NotFound(_)) => String::new(),
            Err(e) => return Err(e),
        };
        let mut doc = parse_doc(WORKSPACE_FILE, &raw)?;
        set_scalar(doc.as_table_mut(), "snippets", Value::from(enabled));
        let rendered = doc.to_string();
        let _: toml::Table = parse_toml(WORKSPACE_FILE, &rendered)?;
        self.write(WORKSPACE_FILE, rendered.as_bytes())
    }

    // ---- Snippets CRUD -----------------------------------------------------------------

    /// Lists all snippet slugs (`snippets/<slug>/snippet.toml`), sorted.
    pub fn list_snippets(&self) -> Result<Vec<String>> {
        self.list_dirs("snippets", "snippet.toml")
    }

    /// Returns lightweight summaries of all workspace snippets, sorted by slug.
    pub fn list_snippet_summaries(&self) -> Result<Vec<SnippetSummary>> {
        let slugs = self.list_snippets()?;
        let mut out = Vec::with_capacity(slugs.len());
        for slug in slugs {
            let meta_rel = format!("snippets/{slug}/snippet.toml");
            let meta_raw = self.read_text(&meta_rel)?;
            let meta: SnippetMeta = parse_toml(&meta_rel, &meta_raw)?;
            let code_rel = format!("snippets/{slug}/code.hpp");
            let code = self.read_optional_text(&code_rel)?;
            let notes_rel = format!("snippets/{slug}/notes.md");
            let has_notes = self.resolve(&notes_rel)?.is_file();

            let (has_code, deps, code_lines, code_hash) = match code {
                Some(ref src) => {
                    let deps = parse_snippet_deps_from_code(src);
                    let printed = preprocess_printed_code(src, DEFAULT_HASH_INTERVAL);
                    let hash = (!printed.total_hash.is_empty()).then_some(printed.total_hash);
                    (true, deps, printed.lines.len(), hash)
                }
                None => (false, Vec::new(), 0, None),
            };

            out.push(SnippetSummary {
                slug,
                title: meta.title,
                tags: meta.tags,
                time_complexity: meta.time_complexity,
                space_complexity: meta.space_complexity,
                description: meta.description,
                has_code,
                has_notes,
                verify_count: meta.verify.len(),
                deps,
                code_lines,
                code_hash,
            });
        }
        Ok(out)
    }

    /// Loads full detail for a snippet at `snippets/<slug>/`, including reverse references
    /// and preprocessed printed code.
    pub fn load_snippet(&self, slug: &str) -> Result<SnippetDetail> {
        check_id(slug)?;
        let dir = format!("snippets/{slug}");
        let mut detail = self.load_snippet_at(slug, &dir).map_err(|e| match e {
            Error::NotFound(p) if p == format!("{dir}/snippet.toml") => {
                Error::NotFound(format!("snippet {slug:?}"))
            }
            other => other,
        })?;
        detail.reverse_refs = self.snippet_reverse_refs(slug)?;
        Ok(detail)
    }

    /// Loads a snippet from an arbitrary workspace-relative directory (either `snippets/<slug>`
    /// or `trds/<id>/snapshot/snippets/<slug>`).
    pub fn load_snippet_at(&self, slug: &str, dir: &str) -> Result<SnippetDetail> {
        let meta_rel = format!("{dir}/snippet.toml");
        let meta_raw = self.read_text(&meta_rel)?;
        let meta: SnippetMeta = parse_toml(&meta_rel, &meta_raw)?;

        let code_rel = format!("{dir}/code.hpp");
        let code = self.read_optional_text(&code_rel)?.map(|text| TextFile {
            path: code_rel,
            text,
        });

        let notes_rel = format!("{dir}/notes.md");
        let notes = self.read_optional_text(&notes_rel)?.map(|text| TextFile {
            path: notes_rel,
            text,
        });

        let test_rel = format!("{dir}/test.cpp");
        let test_file = self.read_optional_text(&test_rel)?.map(|text| TextFile {
            path: test_rel,
            text,
        });

        let mut verify_files = Vec::new();
        let dir_path = self.resolve(dir)?;
        if let Ok(entries) = fs::read_dir(&dir_path) {
            let mut names: Vec<String> = entries
                .filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|n| {
                    !n.starts_with('.')
                        && n != "test.cpp"
                        && n != "code.hpp"
                        && (n.starts_with("verify_") || n.ends_with(".cpp"))
                })
                .collect();
            names.sort();
            for name in names {
                let rel = format!("{dir}/{name}");
                if let Some(text) = self.read_optional_text(&rel)? {
                    verify_files.push(TextFile { path: rel, text });
                }
            }
        }

        let printed_code = code
            .as_ref()
            .map(|c| preprocess_printed_code(&c.text, DEFAULT_HASH_INTERVAL));

        let direct_deps = code
            .as_ref()
            .map(|c| parse_snippet_deps_from_code(&c.text))
            .unwrap_or_default();

        Ok(SnippetDetail {
            slug: slug.to_owned(),
            dir: dir.to_owned(),
            meta,
            code,
            notes,
            test_file,
            verify_files,
            reverse_refs: SnippetReverseRefs {
                deps: direct_deps,
                ..Default::default()
            },
            printed_code,
        })
    }

    /// Creates a new snippet at `snippets/<slug>/`. Refuses if `snippets/<slug>/snippet.toml`
    /// already exists.
    pub fn create_snippet(
        &self,
        slug: &str,
        meta: &SnippetMeta,
        code: Option<&str>,
        notes: Option<&str>,
    ) -> Result<SnippetDetail> {
        check_id(slug)?;
        let meta_rel = format!("snippets/{slug}/snippet.toml");
        self.check_writable(&meta_rel)?;
        let meta_abs = self.resolve(&meta_rel)?;
        if meta_abs.exists() {
            return Err(Error::SnippetAlreadyExists(slug.to_owned()));
        }
        // Per review D: write clean TOML omitting empty `tags` / `verify` arrays on disk,
        // while keeping JSON IPC serialization always emitting `[]`.
        let mut doc = DocumentMut::new();
        doc["title"] = toml_edit::value(meta.title.as_str());
        if !meta.tags.is_empty() {
            let mut arr = toml_edit::Array::new();
            for tg in &meta.tags {
                arr.push(tg.as_str());
            }
            doc["tags"] = toml_edit::value(arr);
        }
        if let Some(tc) = meta
            .time_complexity
            .as_deref()
            .filter(|s| !s.trim().is_empty())
        {
            doc["time_complexity"] = toml_edit::value(tc);
        }
        if let Some(sc) = meta
            .space_complexity
            .as_deref()
            .filter(|s| !s.trim().is_empty())
        {
            doc["space_complexity"] = toml_edit::value(sc);
        }
        if let Some(desc) = meta.description.as_deref().filter(|s| !s.trim().is_empty()) {
            doc["description"] = toml_edit::value(desc);
        }
        if let Some(sp) = meta
            .source_problem
            .as_deref()
            .filter(|s| !s.trim().is_empty())
        {
            doc["source_problem"] = toml_edit::value(sp);
        }
        if !meta.verify.is_empty() {
            let mut arr = toml_edit::ArrayOfTables::new();
            for spec in &meta.verify {
                let mut tbl = toml_edit::Table::new();
                match spec {
                    xcpc_domain::SnippetVerifySpec::SelfTest { file } => {
                        tbl["kind"] = toml_edit::value("self");
                        tbl["file"] = toml_edit::value(file.as_str());
                    }
                    xcpc_domain::SnippetVerifySpec::Problem { problem, adapter } => {
                        tbl["kind"] = toml_edit::value("problem");
                        tbl["problem"] = toml_edit::value(problem.as_str());
                        tbl["adapter"] = toml_edit::value(adapter.as_str());
                    }
                    xcpc_domain::SnippetVerifySpec::LibraryChecker { problem, adapter } => {
                        tbl["kind"] = toml_edit::value("library_checker");
                        tbl["problem"] = toml_edit::value(problem.as_str());
                        tbl["adapter"] = toml_edit::value(adapter.as_str());
                    }
                }
                arr.push(tbl);
            }
            doc["verify"] = toml_edit::Item::ArrayOfTables(arr);
        }
        let toml_text = doc.to_string();
        self.write(&meta_rel, toml_text.as_bytes())?;
        if let Some(c) = code {
            self.write(&format!("snippets/{slug}/code.hpp"), c.as_bytes())?;
        }
        if let Some(n) = notes {
            self.write(&format!("snippets/{slug}/notes.md"), n.as_bytes())?;
        }
        self.load_snippet(slug)
    }

    /// Partially updates `snippets/<slug>/snippet.toml` via `toml_edit`, preserving comments
    /// and unknown fields.
    pub fn update_snippet_meta(&self, slug: &str, patch: &SnippetMetaPatch) -> Result<SnippetMeta> {
        check_id(slug)?;
        let rel = format!("snippets/{slug}/snippet.toml");
        self.check_writable(&rel)?;
        let raw = self.read_text(&rel).map_err(|e| match e {
            Error::NotFound(_) => Error::NotFound(format!("snippet {slug:?}")),
            other => other,
        })?;
        let mut doc = parse_doc(&rel, &raw)?;
        apply_snippet_meta_patch(&mut doc, patch);
        let rendered = doc.to_string();
        let meta: SnippetMeta = parse_toml(&rel, &rendered)?;
        self.write(&rel, rendered.as_bytes())?;
        Ok(meta)
    }

    /// Atomically writes one or more files inside `snippets/<slug>/` (`snippet.toml`,
    /// `code.hpp`, `notes.md`, `test.cpp`, `verify_*.cpp`, etc.).
    pub fn write_snippet_files(
        &self,
        slug: &str,
        files: &BTreeMap<String, String>,
    ) -> Result<SnippetDetail> {
        check_id(slug)?;
        if self.is_read_only() {
            return Err(Error::ReadOnly);
        }
        for (name, content) in files {
            if !is_allowed_snippet_file(name) {
                return Err(Error::DisallowedPath(format!("snippets/{slug}/{name}")));
            }
            if name == "snippet.toml" {
                let _: SnippetMeta = parse_toml(&format!("snippets/{slug}/snippet.toml"), content)?;
            }
        }
        for (name, content) in files {
            let rel = format!("snippets/{slug}/{name}");
            self.write(&rel, content.as_bytes())?;
        }
        self.load_snippet(slug)
    }

    /// Deletes `snippets/<slug>/` from disk.
    pub fn delete_snippet(&self, slug: &str) -> Result<()> {
        check_id(slug)?;
        if self.is_read_only() {
            return Err(Error::ReadOnly);
        }
        let dir_rel = format!("snippets/{slug}");
        let abs = self.resolve(&dir_rel)?;
        if !abs.is_dir() {
            return Err(Error::NotFound(format!("snippet {slug:?}")));
        }
        let meta = fs::symlink_metadata(&abs).map_err(|e| io(&dir_rel, e))?;
        if meta.file_type().is_symlink() {
            return Err(Error::PathEscape(dir_rel));
        }
        fs::remove_dir_all(&abs).map_err(|e| io(&dir_rel, e))?;
        Ok(())
    }

    // ---- Dependency DAG & Reverse Index ------------------------------------------------

    /// Builds the workspace snippet dependency DAG.
    pub fn snippet_dag(&self) -> Result<SnippetDag> {
        SnippetDag::build(self)
    }

    /// Computes dependency and reverse-reference metadata for one snippet.
    pub fn snippet_reverse_refs(&self, slug: &str) -> Result<SnippetReverseRefs> {
        check_id(slug)?;
        let dag = self.snippet_dag()?;
        let deps = dag.direct_deps(slug);
        let transitive_deps = dag.transitive_deps(slug)?;
        let reverse_deps = dag.reverse_deps(slug);
        let related_problems = self.related_problems_for_snippet(slug)?;
        Ok(SnippetReverseRefs {
            deps,
            transitive_deps,
            reverse_deps,
            related_problems,
        })
    }

    /// Computes dependency and reverse-reference metadata for all snippets in the workspace.
    pub fn all_snippet_reverse_refs(&self) -> Result<BTreeMap<String, SnippetReverseRefs>> {
        let dag = self.snippet_dag()?;
        let problem_ids: HashSet<String> = self.list_problems()?.into_iter().collect();
        let mut out = BTreeMap::new();
        for slug in dag.direct.keys() {
            let deps = dag.direct_deps(slug);
            let transitive_deps = dag.transitive_deps(slug).unwrap_or_default();
            let reverse_deps = dag.reverse_deps(slug);
            let related_problems =
                self.related_problems_for_snippet_with_set(slug, &problem_ids)?;
            out.insert(
                slug.clone(),
                SnippetReverseRefs {
                    deps,
                    transitive_deps,
                    reverse_deps,
                    related_problems,
                },
            );
        }
        Ok(out)
    }

    /// Returns `[slug, ...all transitive dependents]` in topological order for cascade verification.
    pub fn snippet_dependents_topo(&self, slug: &str) -> Result<Vec<String>> {
        check_id(slug)?;
        let dag = self.snippet_dag()?;
        if !dag.direct.contains_key(slug) {
            return Err(Error::NotFound(format!("snippet {slug:?}")));
        }
        dag.dependents_topo(slug)
    }

    fn related_problems_for_snippet(&self, slug: &str) -> Result<Vec<String>> {
        let problem_ids: HashSet<String> = self.list_problems()?.into_iter().collect();
        self.related_problems_for_snippet_with_set(slug, &problem_ids)
    }

    fn related_problems_for_snippet_with_set(
        &self,
        slug: &str,
        problem_ids: &HashSet<String>,
    ) -> Result<Vec<String>> {
        let mut related = BTreeSet::new();
        let meta_rel = format!("snippets/{slug}/snippet.toml");
        if let Ok(raw) = self.read_text(&meta_rel)
            && let Ok(meta) = parse_toml::<SnippetMeta>(&meta_rel, &raw)
        {
            if let Some(src) = meta.source_problem
                && !src.trim().is_empty()
            {
                related.insert(src);
            }
            for v in &meta.verify {
                if let SnippetVerifySpec::Problem { problem, .. } = v
                    && !problem.trim().is_empty()
                {
                    related.insert(problem.clone());
                }
            }
        }
        // Scan notes.md for explicit `problems/<slug>` references.
        let notes_rel = format!("snippets/{slug}/notes.md");
        if let Some(notes) = self.read_optional_text(&notes_rel)? {
            for part in notes.split("problems/").skip(1) {
                let candidate: String = part
                    .chars()
                    .take_while(|c| {
                        c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-' || *c == '_'
                    })
                    .collect();
                if !candidate.is_empty() && problem_ids.contains(&candidate) {
                    related.insert(candidate);
                }
            }
        }
        Ok(related.into_iter().collect())
    }

    // ---- Bundler Entry Points ----------------------------------------------------------

    /// Bundles a single snippet slug and all its transitive dependencies.
    pub fn bundle_snippet(&self, slug: &str, opts: &BundleOptions) -> Result<BundleResult> {
        bundle_snippet(self, slug, opts)
    }

    /// Sandboxed workspace-only bundler (for MCP `bundle_code`).
    pub fn bundle_workspace_code(
        &self,
        rel_path: Option<&str>,
        source_text: Option<&str>,
        opts: &BundleOptions,
    ) -> Result<BundleResult> {
        bundle_workspace_code(self, rel_path, source_text, opts)
    }

    /// Unrestricted local-file bundler (for CLI `xcpc bundle` and GUI).
    pub fn bundle_local_file(&self, path: &Path, opts: &BundleOptions) -> Result<BundleResult> {
        bundle_local_file(self, path, opts)
    }

    /// Unrestricted pasted-source bundler (for GUI/CLI).
    pub fn bundle_local_source(
        &self,
        source_text: &str,
        base_dir: Option<&Path>,
        opts: &BundleOptions,
    ) -> Result<BundleResult> {
        bundle_local_source(self, source_text, base_dir, opts)
    }

    // ---- TRDs CRUD ---------------------------------------------------------------------

    /// Lists all TRD IDs (`trds/<id>/trd.toml`), sorted.
    pub fn list_trds(&self) -> Result<Vec<String>> {
        self.list_dirs("trds", "trd.toml")
    }

    /// Lists lightweight summaries for all TRDs, sorted by ID.
    pub fn list_trd_summaries(&self) -> Result<Vec<TrdSummary>> {
        let ids = self.list_trds()?;
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            let trd = self.load_trd(&id)?;
            let item_count = trd.meta.chapters.iter().map(|c| c.items.len()).sum();
            out.push(TrdSummary {
                id: trd.id,
                name: trd.meta.name,
                author: trd.meta.author,
                date: trd.meta.date,
                template: trd.meta.template,
                frozen: trd.meta.frozen,
                archived: trd.meta.archived,
                chapter_count: trd.meta.chapters.len(),
                item_count,
            });
        }
        Ok(out)
    }

    /// Loads `trds/<id>/trd.toml`.
    pub fn load_trd(&self, id: &str) -> Result<TrdDetail> {
        check_id(id)?;
        let dir = format!("trds/{id}");
        let rel = format!("{dir}/trd.toml");
        let raw = self.read_text(&rel).map_err(|e| match e {
            Error::NotFound(_) => Error::NotFound(format!("trd {id:?}")),
            other => other,
        })?;
        let meta: TrdMeta = parse_toml(&rel, &raw)?;
        for ch in &meta.chapters {
            for item in &ch.items {
                check_id(item).map_err(|e| Error::Invalid {
                    path: rel.clone(),
                    message: e.to_string(),
                })?;
            }
        }
        let has_local_template = self.resolve(&format!("{dir}/template.typ"))?.is_file();
        let has_snapshot = self.resolve(&format!("{dir}/snapshot"))?.is_dir();
        Ok(TrdDetail {
            id: id.to_owned(),
            dir,
            meta,
            has_local_template,
            has_snapshot,
        })
    }

    /// Loads all chapters and their snippets for `trd`: from `snippets/<slug>/` if live,
    /// or from `trds/<id>/snapshot/snippets/<slug>/` if frozen.
    pub fn trd_snippets(&self, trd: &TrdDetail) -> Result<Vec<(String, Vec<SnippetDetail>)>> {
        let mut out = Vec::with_capacity(trd.meta.chapters.len());
        for ch in &trd.meta.chapters {
            let mut items = Vec::with_capacity(ch.items.len());
            for slug in &ch.items {
                let snippet = if trd.meta.frozen {
                    let snap_dir = format!("{}/snapshot/snippets/{slug}", trd.dir);
                    self.load_snippet_at(slug, &snap_dir)?
                } else {
                    self.load_snippet(slug)?
                };
                items.push(snippet);
            }
            out.push((ch.title.clone(), items));
        }
        Ok(out)
    }

    /// Creates or replaces a TRD's chapters (and optionally updates metadata) via `toml_edit`.
    pub fn compose_trd(
        &self,
        id: &str,
        chapters: &[TrdChapter],
        meta_patch: Option<&TrdMetaPatch>,
    ) -> Result<TrdDetail> {
        check_id(id)?;
        let rel = format!("trds/{id}/trd.toml");
        self.check_writable(&rel)?;

        for ch in chapters {
            for item in &ch.items {
                check_id(item)?;
            }
        }

        let mut doc = match self.read_text(&rel) {
            Ok(raw) => parse_doc(&rel, &raw)?,
            Err(Error::NotFound(_)) => {
                let mut d = DocumentMut::new();
                let default_name = meta_patch.and_then(|p| p.name.as_deref()).unwrap_or(id);
                d["name"] = toml_edit::value(default_name);
                d["template"] = toml_edit::value("trd-compact");
                d
            }
            Err(e) => return Err(e),
        };

        if let Some(patch) = meta_patch {
            apply_trd_meta_patch(&mut doc, patch);
        }
        rewrite_trd_chapters(&mut doc, chapters);

        let rendered = doc.to_string();
        let _: TrdMeta = parse_toml(&rel, &rendered)?;
        self.write(&rel, rendered.as_bytes())?;
        self.load_trd(id)
    }

    /// Partially updates `trds/<id>/trd.toml` metadata via `toml_edit`.
    pub fn update_trd_meta(&self, id: &str, patch: &TrdMetaPatch) -> Result<TrdDetail> {
        check_id(id)?;
        let rel = format!("trds/{id}/trd.toml");
        self.check_writable(&rel)?;
        let raw = self.read_text(&rel).map_err(|e| match e {
            Error::NotFound(_) => Error::NotFound(format!("trd {id:?}")),
            other => other,
        })?;
        let mut doc = parse_doc(&rel, &raw)?;
        apply_trd_meta_patch(&mut doc, patch);
        let rendered = doc.to_string();
        let _: TrdMeta = parse_toml(&rel, &rendered)?;
        self.write(&rel, rendered.as_bytes())?;
        self.load_trd(id)
    }

    /// Deletes an unfrozen, unarchived TRD directory `trds/<id>/`.
    pub fn delete_trd(&self, id: &str) -> Result<()> {
        check_id(id)?;
        let rel = format!("trds/{id}/trd.toml");
        self.check_writable(&rel)?;
        let _ = self.load_trd(id)?;
        let dir_rel = format!("trds/{id}");
        let abs = self.resolve(&dir_rel)?;
        let meta = fs::symlink_metadata(&abs).map_err(|e| io(&dir_rel, e))?;
        if meta.file_type().is_symlink() {
            return Err(Error::PathEscape(dir_rel));
        }
        fs::remove_dir_all(&abs).map_err(|e| io(&dir_rel, e))?;
        Ok(())
    }

    /// Archives or unarchives a TRD (orthogonal to `frozen`).
    pub fn set_trd_archived(&self, id: &str, archived: bool) -> Result<TrdMeta> {
        check_id(id)?;
        if self.is_read_only() {
            return Err(Error::ReadOnly);
        }
        let rel = format!("trds/{id}/trd.toml");
        let raw = self.read_text(&rel).map_err(|e| match e {
            Error::NotFound(_) => Error::NotFound(format!("trd {id:?}")),
            other => other,
        })?;
        let mut doc = parse_doc(&rel, &raw)?;
        if archived {
            set_scalar(doc.as_table_mut(), "archived", Value::from(true));
        } else {
            doc.as_table_mut().remove("archived");
        }
        let rendered = doc.to_string();
        let meta: TrdMeta = parse_toml(&rel, &rendered)?;
        self.write_unfreeze_bypass(&rel, rendered.as_bytes())?;
        Ok(meta)
    }

    // ---- TRD Preflight, Freeze & Unfreeze ----------------------------------------------

    /// Validates a TRD before build/freeze:
    /// - Missing snippet slugs (`Error`)
    /// - Dependency cycles (`Error`)
    /// - Missing transitive snippet dependencies (`Error`, with `missing_deps` populated)
    /// - Unverified / Stale / Failed code snippets (`Warning`; pure formula cards without
    ///   `code.hpp` are skipped)
    pub fn preflight_trd(
        &self,
        id: &str,
        verify_statuses: &BTreeMap<String, SnippetVerifyStatus>,
    ) -> Result<Vec<TrdPreflightIssue>> {
        let trd = self.load_trd(id)?;
        let dag = self.snippet_dag()?;
        let mut issues = Vec::new();

        let mut included: HashSet<String> = HashSet::new();
        let mut ordered_items: Vec<String> = Vec::new();
        for ch in &trd.meta.chapters {
            for slug in &ch.items {
                if included.insert(slug.clone()) {
                    ordered_items.push(slug.clone());
                }
            }
        }

        let title_of = |s: &str| -> String {
            self.load_snippet(s)
                .ok()
                .map(|d| d.meta.title)
                .filter(|t| !t.trim().is_empty())
                .unwrap_or_else(|| s.to_owned())
        };

        for slug in &ordered_items {
            if !dag.direct.contains_key(slug) {
                issues.push(TrdPreflightIssue {
                    severity: TrdPreflightSeverity::Error,
                    kind: TrdPreflightKind::MissingSnippet,
                    snippet: slug.clone(),
                    missing_deps: Vec::new(),
                    message: format!("未找到模板「{slug}」"),
                });
                continue;
            }
            let title = title_of(slug);

            match dag.transitive_deps(slug) {
                Ok(trans) => {
                    let missing: Vec<String> = trans
                        .into_iter()
                        .filter(|d| !included.contains(d))
                        .collect();
                    if !missing.is_empty() {
                        let missing_titles: Vec<String> =
                            missing.iter().map(|m| title_of(m)).collect();
                        issues.push(TrdPreflightIssue {
                            severity: TrdPreflightSeverity::Error,
                            kind: TrdPreflightKind::MissingDependency,
                            snippet: slug.clone(),
                            missing_deps: missing,
                            message: format!(
                                "模板「{title}」缺少前置依赖：{}",
                                missing_titles.join("、")
                            ),
                        });
                    }
                }
                Err(Error::DependencyCycle { cycle, .. }) => {
                    issues.push(TrdPreflightIssue {
                        severity: TrdPreflightSeverity::Error,
                        kind: TrdPreflightKind::DependencyCycle,
                        snippet: slug.clone(),
                        missing_deps: Vec::new(),
                        message: format!("模板「{title}」存在循环依赖：{}", cycle.join(" → ")),
                    });
                }
                Err(e) => return Err(e),
            }

            // Verification check applies only to snippets with `code.hpp`.
            // Per review S-2, `Unverified` does not emit a default preflight warning;
            // only `Stale` and `Failed` warn (`freeze_trd(..., strict = true)` still checks unverified).
            let has_code = self
                .resolve(&format!("snippets/{slug}/code.hpp"))?
                .is_file();
            if has_code {
                let status = verify_statuses
                    .get(slug)
                    .copied()
                    .unwrap_or(SnippetVerifyStatus::Unverified);
                match status {
                    SnippetVerifyStatus::Passed | SnippetVerifyStatus::Unverified => {}
                    SnippetVerifyStatus::Stale => issues.push(TrdPreflightIssue {
                        severity: TrdPreflightSeverity::Warning,
                        kind: TrdPreflightKind::StaleSnippet,
                        snippet: slug.clone(),
                        missing_deps: Vec::new(),
                        message: format!("模板「{title}」的代码或上游依赖已改动，建议重新验板"),
                    }),
                    SnippetVerifyStatus::Failed => issues.push(TrdPreflightIssue {
                        severity: TrdPreflightSeverity::Warning,
                        kind: TrdPreflightKind::FailedSnippet,
                        snippet: slug.clone(),
                        missing_deps: Vec::new(),
                        message: format!("模板「{title}」上次验板未通过"),
                    }),
                }
            }
        }

        Ok(issues)
    }

    /// Freezes a TRD into `trds/<id>/snapshot/` and sets `frozen = true`.
    ///
    /// - Hard errors (missing snippet, missing transitive dep, cycle) always block freeze.
    /// - Verification warnings (`Unverified` / `Stale` / `Failed` code snippets) are returned
    ///   as warnings by default, or block freeze with [`Error::TrdFreezeBlocked`] when `strict = true`.
    pub fn freeze_trd(
        &self,
        id: &str,
        strict: bool,
        verify_statuses: &BTreeMap<String, SnippetVerifyStatus>,
        template_archive: Option<&SnapshotTemplateArchive>,
        pdf_bytes: Option<&[u8]>,
    ) -> Result<TrdFreezeResult> {
        check_id(id)?;
        if self.is_read_only() {
            return Err(Error::ReadOnly);
        }
        let trd = self.load_trd(id)?;
        if trd.meta.archived {
            return Err(Error::TrdArchived(id.to_owned()));
        }
        if trd.meta.frozen {
            return Err(Error::TrdAlreadyFrozen(id.to_owned()));
        }

        let issues = self.preflight_trd(id, verify_statuses)?;
        let errors: Vec<String> = issues
            .iter()
            .filter(|i| i.severity == TrdPreflightSeverity::Error)
            .map(|i| i.message.clone())
            .collect();
        let warnings: Vec<TrdPreflightIssue> = issues
            .iter()
            .filter(|i| i.severity == TrdPreflightSeverity::Warning)
            .cloned()
            .collect();

        if !errors.is_empty() {
            return Err(Error::TrdFreezeBlocked {
                id: trd.meta.name.clone(),
                issues: errors,
            });
        }
        if strict {
            let mut strict_issues: Vec<String> =
                warnings.iter().map(|w| w.message.clone()).collect();
            let mut checked = HashSet::new();
            for ch in &trd.meta.chapters {
                for slug in &ch.items {
                    if !checked.insert(slug.as_str()) {
                        continue;
                    }
                    let has_code = self
                        .resolve(&format!("snippets/{slug}/code.hpp"))?
                        .is_file();
                    if has_code {
                        let status = verify_statuses
                            .get(slug)
                            .copied()
                            .unwrap_or(SnippetVerifyStatus::Unverified);
                        if status == SnippetVerifyStatus::Unverified {
                            let title = self
                                .load_snippet(slug)
                                .ok()
                                .map(|d| d.meta.title)
                                .filter(|t| !t.trim().is_empty())
                                .unwrap_or_else(|| slug.clone());
                            strict_issues.push(format!("模板「{title}」尚未验板"));
                        }
                    }
                }
            }
            if !strict_issues.is_empty() {
                return Err(Error::TrdFreezeBlocked {
                    id: trd.meta.name.clone(),
                    issues: strict_issues,
                });
            }
        }

        // Collect unique snippet slugs in order.
        let mut unique_slugs = Vec::new();
        let mut seen = HashSet::new();
        for ch in &trd.meta.chapters {
            for slug in &ch.items {
                if seen.insert(slug.clone()) {
                    unique_slugs.push(slug.clone());
                }
            }
        }

        let snap_rel = format!("trds/{id}/snapshot");
        let snap_abs = self.resolve(&snap_rel)?;
        if snap_abs.exists() {
            fs::remove_dir_all(&snap_abs).map_err(|e| io(&snap_rel, e))?;
        }

        // Copy each referenced snippet's source files into `snapshot/snippets/<slug>/`.
        for slug in &unique_slugs {
            let files = self.collect_snippet_snapshot_files(&format!("snippets/{slug}"))?;
            for (name, bytes) in files {
                self.write(&format!("{snap_rel}/snippets/{slug}/{name}"), &bytes)?;
            }
        }

        // Archive effective template & options if provided.
        if let Some(archive) = template_archive {
            let tpl_toml = toml::to_string(&archive.meta).map_err(|e| Error::Invalid {
                path: format!("{snap_rel}/template.toml"),
                message: e.to_string(),
            })?;
            self.write(&format!("{snap_rel}/template.toml"), tpl_toml.as_bytes())?;
            if let Some(bytes) = &archive.custom_template_bytes {
                self.write(&format!("{snap_rel}/template.typ"), bytes)?;
            }
            if let Some(bytes) = &archive.preamble_bytes {
                self.write(&format!("{snap_rel}/preamble.typ"), bytes)?;
            }
            for (name, bytes) in &archive.assets {
                self.write(&format!("{snap_rel}/assets/{name}"), bytes)?;
            }
            for (rel_file, bytes) in &archive.plugin_bundle_files {
                self.write(&format!("{snap_rel}/template/{rel_file}"), bytes)?;
            }
        }

        // Archive exported PDF if provided.
        let pdf_archived = if let Some(pdf) = pdf_bytes {
            self.write(&format!("{snap_rel}/{id}.pdf"), pdf)?;
            true
        } else {
            false
        };

        // Mark `frozen = true` in `trd.toml` via `toml_edit`.
        let trd_rel = format!("trds/{id}/trd.toml");
        let raw = self.read_text(&trd_rel)?;
        let mut doc = parse_doc(&trd_rel, &raw)?;
        set_scalar(doc.as_table_mut(), "frozen", Value::from(true));
        let rendered = doc.to_string();
        let _: TrdMeta = parse_toml(&trd_rel, &rendered)?;
        self.write(&trd_rel, rendered.as_bytes())?;

        Ok(TrdFreezeResult {
            id: id.to_owned(),
            snapshot_dir: snap_rel,
            snippet_count: unique_slugs.len(),
            warnings,
            pdf_archived,
        })
    }

    /// Compares a frozen TRD's `snapshot/snippets/` against the live `snippets/` library.
    pub fn diff_trd_for_unfreeze(&self, id: &str) -> Result<TrdUnfreezeDiff> {
        check_id(id)?;
        let trd = self.load_trd(id)?;
        if !trd.meta.frozen {
            return Err(Error::TrdNotFrozen(id.to_owned()));
        }

        let mut unique_slugs = Vec::new();
        let mut seen = HashSet::new();
        for ch in &trd.meta.chapters {
            for slug in &ch.items {
                if seen.insert(slug.clone()) {
                    unique_slugs.push(slug.clone());
                }
            }
        }

        let mut diffs = Vec::new();
        for slug in unique_slugs {
            let snap_dir = format!("trds/{id}/snapshot/snippets/{slug}");
            let live_dir = format!("snippets/{slug}");
            if !self.resolve(&format!("{live_dir}/snippet.toml"))?.is_file() {
                diffs.push(TrdUnfreezeSnippetDiff {
                    slug,
                    kinds: vec![TrdSnippetDiffKind::SnippetDeleted],
                    changed_files: Vec::new(),
                });
                continue;
            }
            let snap_files = self.collect_snippet_snapshot_files(&snap_dir)?;
            let live_files = self.collect_snippet_snapshot_files(&live_dir)?;
            let all_keys: BTreeSet<&str> = snap_files
                .keys()
                .chain(live_files.keys())
                .map(String::as_str)
                .collect();

            let mut kinds = BTreeSet::new();
            let mut changed_files = Vec::new();
            for key in all_keys {
                if snap_files.get(key) != live_files.get(key) {
                    changed_files.push(key.to_owned());
                    match key {
                        "code.hpp" => {
                            kinds.insert(TrdSnippetDiffKind::CodeChanged);
                        }
                        "notes.md" => {
                            kinds.insert(TrdSnippetDiffKind::NotesChanged);
                        }
                        "snippet.toml" => {
                            kinds.insert(TrdSnippetDiffKind::MetaChanged);
                        }
                        _ => {
                            kinds.insert(TrdSnippetDiffKind::MetaChanged);
                        }
                    }
                }
            }
            if !kinds.is_empty() {
                diffs.push(TrdUnfreezeSnippetDiff {
                    slug,
                    kinds: kinds.into_iter().collect(),
                    changed_files,
                });
            }
        }

        let has_changes = !diffs.is_empty();
        Ok(TrdUnfreezeDiff {
            id: id.to_owned(),
            diffs,
            has_changes,
        })
    }

    /// Sole TRD unfreeze entry point (for GUI/CLI with human confirmation; never exposed to MCP).
    /// Sets `frozen = false` in `trd.toml` via `toml_edit` and removes `snapshot/`.
    pub fn unfreeze_trd(&self, id: &str) -> Result<TrdDetail> {
        check_id(id)?;
        if self.is_read_only() {
            return Err(Error::ReadOnly);
        }
        let trd = self.load_trd(id)?;
        if trd.meta.archived {
            return Err(Error::TrdArchived(id.to_owned()));
        }
        if !trd.meta.frozen {
            return Err(Error::TrdNotFrozen(id.to_owned()));
        }

        let trd_rel = format!("trds/{id}/trd.toml");
        let raw = self.read_text(&trd_rel)?;
        let mut doc = parse_doc(&trd_rel, &raw)?;
        set_scalar(doc.as_table_mut(), "frozen", Value::from(false));
        let rendered = doc.to_string();
        let _: TrdMeta = parse_toml(&trd_rel, &rendered)?;
        self.write_unfreeze_bypass(&trd_rel, rendered.as_bytes())?;

        let snap_rel = format!("trds/{id}/snapshot");
        let snap_abs = self.resolve(&snap_rel)?;
        if snap_abs.exists() {
            let meta = fs::symlink_metadata(&snap_abs).map_err(|e| io(&snap_rel, e))?;
            if meta.file_type().is_symlink() {
                return Err(Error::PathEscape(snap_rel));
            }
            fs::remove_dir_all(&snap_abs).map_err(|e| io(&snap_rel, e))?;
        }

        self.load_trd(id)
    }

    fn collect_snippet_snapshot_files(&self, dir: &str) -> Result<BTreeMap<String, Vec<u8>>> {
        let mut out = BTreeMap::new();
        for name in ["snippet.toml", "code.hpp", "notes.md"] {
            let rel = format!("{dir}/{name}");
            let abs = self.resolve(&rel)?;
            if abs.is_file() {
                let bytes = fs::read(&abs).map_err(|e| io(&rel, e))?;
                out.insert(name.to_owned(), bytes);
            }
        }
        Ok(out)
    }

    fn read_optional_text(&self, rel: &str) -> Result<Option<String>> {
        match self.read_text(rel) {
            Ok(s) => Ok(Some(s)),
            Err(Error::NotFound(_)) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

// ---- toml_edit helpers -----------------------------------------------------------------

fn parse_doc(path: &str, raw: &str) -> Result<DocumentMut> {
    raw.parse::<DocumentMut>().map_err(|e| Error::Invalid {
        path: path.to_owned(),
        message: e.to_string().trim_end().to_owned(),
    })
}

fn set_scalar(table: &mut Table, key: &str, value: Value) {
    if let Some(existing) = table.get_mut(key) {
        *existing = Item::Value(value);
    } else {
        table.insert(key, Item::Value(value));
    }
}

fn apply_string_patch(table: &mut Table, key: &str, patch: &Patch<String>) {
    match patch {
        Patch::Keep => {}
        Patch::Clear => {
            table.remove(key);
        }
        Patch::Set(v) => {
            if v.trim().is_empty() {
                table.remove(key);
            } else {
                set_scalar(table, key, Value::from(v.as_str()));
            }
        }
    }
}

fn apply_snippet_meta_patch(doc: &mut DocumentMut, patch: &SnippetMetaPatch) {
    let tbl = doc.as_table_mut();
    if let Some(title) = &patch.title {
        set_scalar(tbl, "title", Value::from(title.as_str()));
    }
    if let Some(tags) = &patch.tags {
        let mut arr = Array::new();
        for t in tags {
            let trimmed = t.trim();
            if !trimmed.is_empty() {
                arr.push(trimmed);
            }
        }
        tbl.insert("tags", Item::Value(Value::Array(arr)));
    }
    apply_string_patch(tbl, "time_complexity", &patch.time_complexity);
    apply_string_patch(tbl, "space_complexity", &patch.space_complexity);
    apply_string_patch(tbl, "description", &patch.description);
    apply_string_patch(tbl, "source_problem", &patch.source_problem);

    if let Some(verifies) = &patch.verify {
        if verifies.is_empty() {
            tbl.remove("verify");
        } else {
            let mut aot = ArrayOfTables::new();
            for v in verifies {
                let mut t = Table::new();
                match v {
                    SnippetVerifySpec::SelfTest { file } => {
                        t["kind"] = toml_edit::value("self");
                        t["file"] = toml_edit::value(file.as_str());
                    }
                    SnippetVerifySpec::Problem { problem, adapter } => {
                        t["kind"] = toml_edit::value("problem");
                        t["problem"] = toml_edit::value(problem.as_str());
                        t["adapter"] = toml_edit::value(adapter.as_str());
                    }
                    SnippetVerifySpec::LibraryChecker { problem, adapter } => {
                        t["kind"] = toml_edit::value("library_checker");
                        t["problem"] = toml_edit::value(problem.as_str());
                        t["adapter"] = toml_edit::value(adapter.as_str());
                    }
                }
                aot.push(t);
            }
            tbl.insert("verify", Item::ArrayOfTables(aot));
        }
    }
}

fn apply_trd_meta_patch(doc: &mut DocumentMut, patch: &TrdMetaPatch) {
    let tbl = doc.as_table_mut();
    if let Some(name) = &patch.name {
        set_scalar(tbl, "name", Value::from(name.as_str()));
    }
    apply_string_patch(tbl, "author", &patch.author);
    apply_string_patch(tbl, "date", &patch.date);
    apply_string_patch(tbl, "template", &patch.template);
    if !patch.template_options.is_empty() {
        apply_trd_template_options_patch(tbl, &patch.template_options);
    }
}

fn apply_trd_template_options_patch(
    root: &mut Table,
    namespaces: &BTreeMap<String, TemplateOptionsPatch>,
) {
    if !root.contains_key("template-options") {
        let mut t = Table::new();
        t.set_implicit(true);
        root.insert("template-options", Item::Table(t));
    }
    let Some(opts_tbl) = root
        .get_mut("template-options")
        .and_then(Item::as_table_mut)
    else {
        return;
    };
    for (tpl, patch) in namespaces {
        if !opts_tbl.contains_key(tpl) {
            opts_tbl.insert(tpl, Item::Table(Table::new()));
        }
        let Some(ns_tbl) = opts_tbl.get_mut(tpl).and_then(Item::as_table_mut) else {
            continue;
        };
        for (k, p) in patch {
            match p {
                Patch::Keep => {}
                Patch::Clear => {
                    ns_tbl.remove(k);
                }
                Patch::Set(val) => {
                    ns_tbl.insert(k, Item::Value(option_value_to_toml(val)));
                }
            }
        }
        if ns_tbl.is_empty() {
            opts_tbl.remove(tpl);
        }
    }
    if opts_tbl.is_empty() {
        root.remove("template-options");
    }
}

fn option_value_to_toml(v: &TemplateOptionValue) -> Value {
    match v {
        TemplateOptionValue::Bool(b) => Value::from(*b),
        TemplateOptionValue::Int(i) => Value::from(*i),
        TemplateOptionValue::Float(f) => Value::from(*f),
        TemplateOptionValue::String(s) => Value::from(s.as_str()),
        TemplateOptionValue::Lines(lines) => {
            let mut arr = Array::new();
            for l in lines {
                arr.push(l.as_str());
            }
            Value::Array(arr)
        }
    }
}

fn rewrite_trd_chapters(doc: &mut DocumentMut, chapters: &[TrdChapter]) {
    if chapters.is_empty() {
        doc.as_table_mut().remove("chapter");
        return;
    }
    let mut aot = ArrayOfTables::new();
    for ch in chapters {
        let mut t = Table::new();
        t["title"] = toml_edit::value(ch.title.as_str());
        let mut arr = Array::new();
        for item in &ch.items {
            arr.push(item.as_str());
        }
        t["items"] = Item::Value(Value::Array(arr));
        aot.push(t);
    }
    doc.as_table_mut()
        .insert("chapter", Item::ArrayOfTables(aot));
}
