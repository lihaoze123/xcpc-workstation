//! Workspace snippet dependency DAG parser and general C++ single-file bundler.
//!
//! Supports both:
//! - Sandboxed workspace-only bundling (for MCP `bundle_code`), and
//! - Unrestricted local `.cpp` + `-I <dir>` bundling (for CLI `xcpc bundle` and GUI).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use xcpc_domain::{BundleIncludeEdge, BundleOptions, BundleResult};

use crate::code_hash::{
    extract_snippet_slug_from_include, is_pragma_once_line, parse_local_include_line,
    parse_system_include_line, strip_cpp_comments,
};
use crate::{Error, Result, Workspace, io};

/// Parsed local `#include "..."` directive with 1-based line number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedInclude {
    pub line: usize,
    pub path: String,
}

/// Parses all local `#include "..."` directives from C++ source text, ignoring
/// directives inside `/* ... */` block comments or `//` line comments.
pub fn parse_local_includes(source: &str) -> Vec<ParsedInclude> {
    let mut out = Vec::new();
    let mut in_block_comment = false;

    for (idx, raw_line) in source.lines().enumerate() {
        let trimmed = strip_leading_block_comment(raw_line, &mut in_block_comment);
        if let Some(path) = parse_local_include_line(trimmed.trim()) {
            out.push(ParsedInclude {
                line: idx + 1,
                path: path.to_owned(),
            });
        }
        update_trailing_block_comment(raw_line, &mut in_block_comment);
    }

    out
}

/// Extracts direct workspace snippet dependencies (`#include "<slug>/code.hpp"`) from `code.hpp`,
/// deduplicated in first-seen order.
pub fn parse_snippet_deps_from_code(code: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut deps = Vec::new();
    for inc in parse_local_includes(code) {
        if let Some(slug) = extract_snippet_slug_from_include(&inc.path)
            && seen.insert(slug.clone())
        {
            deps.push(slug);
        }
    }
    deps
}

/// Workspace-wide snippet dependency graph.
#[derive(Debug, Clone, Default)]
pub struct SnippetDag {
    /// Direct dependencies for each snippet slug (`slug -> [(line, dep_slug)]`).
    pub direct: BTreeMap<String, Vec<(usize, String)>>,
}

impl SnippetDag {
    /// Builds the snippet DAG by scanning `snippets/<slug>/code.hpp` for all workspace snippets.
    pub fn build(ws: &Workspace) -> Result<Self> {
        let slugs = ws.list_snippets()?;
        let mut direct = BTreeMap::new();
        for slug in slugs {
            let code_rel = format!("snippets/{slug}/code.hpp");
            let deps = match ws.read_text(&code_rel) {
                Ok(code) => {
                    let mut seen = HashSet::new();
                    let mut list = Vec::new();
                    for inc in parse_local_includes(&code) {
                        if let Some(dep_slug) = extract_snippet_slug_from_include(&inc.path)
                            && seen.insert(dep_slug.clone())
                        {
                            list.push((inc.line, dep_slug));
                        }
                    }
                    list
                }
                Err(Error::NotFound(_)) => Vec::new(),
                Err(e) => return Err(e),
            };
            direct.insert(slug, deps);
        }
        Ok(Self { direct })
    }

    /// Returns direct dependency slugs for `slug`.
    pub fn direct_deps(&self, slug: &str) -> Vec<String> {
        self.direct
            .get(slug)
            .map(|v| v.iter().map(|(_, d)| d.clone()).collect())
            .unwrap_or_default()
    }

    /// Returns all transitive dependencies of `slug` in topological order (excluding `slug` itself).
    /// Returns [`Error::DependencyCycle`] if a cycle is reachable from `slug`.
    pub fn transitive_deps(&self, slug: &str) -> Result<Vec<String>> {
        let mut visiting: Vec<String> = Vec::new();
        let mut visited: HashSet<String> = HashSet::new();
        let mut order: Vec<String> = Vec::new();
        self.dfs_deps(slug, &mut visiting, &mut visited, &mut order)?;
        order.retain(|s| s != slug);
        Ok(order)
    }

    fn dfs_deps(
        &self,
        current: &str,
        visiting: &mut Vec<String>,
        visited: &mut HashSet<String>,
        order: &mut Vec<String>,
    ) -> Result<()> {
        if visited.contains(current) {
            return Ok(());
        }
        visiting.push(current.to_owned());
        if let Some(deps) = self.direct.get(current) {
            for (line, dep) in deps {
                if let Some(pos) = visiting.iter().position(|s| s == dep) {
                    let mut cycle = visiting[pos..].to_vec();
                    cycle.push(dep.clone());
                    return Err(Error::DependencyCycle {
                        file: format!("snippets/{current}/code.hpp"),
                        line: *line,
                        cycle,
                    });
                }
                if self.direct.contains_key(dep) {
                    self.dfs_deps(dep, visiting, visited, order)?;
                }
            }
        }
        visiting.pop();
        visited.insert(current.to_owned());
        order.push(current.to_owned());
        Ok(())
    }

    /// Returns all snippets that directly or transitively depend on `target`, sorted.
    pub fn reverse_deps(&self, target: &str) -> Vec<String> {
        let mut rev_adj: HashMap<&str, Vec<&str>> = HashMap::new();
        for (slug, deps) in &self.direct {
            for (_, dep) in deps {
                rev_adj.entry(dep.as_str()).or_default().push(slug.as_str());
            }
        }
        let mut visited = BTreeSet::new();
        let mut stack = vec![target];
        while let Some(node) = stack.pop() {
            if let Some(dependents) = rev_adj.get(node) {
                for &dep in dependents {
                    if dep != target && visited.insert(dep.to_owned()) {
                        stack.push(dep);
                    }
                }
            }
        }
        visited.into_iter().collect()
    }

    /// Returns `[target, ...all transitive dependents]` in topological order so upstream
    /// snippets are always verified before downstream dependents.
    pub fn dependents_topo(&self, target: &str) -> Result<Vec<String>> {
        let rev: HashSet<String> = self.reverse_deps(target).into_iter().collect();
        let mut subset: BTreeSet<String> = rev.into_iter().collect();
        subset.insert(target.to_owned());

        let mut visiting = Vec::new();
        let mut visited = HashSet::new();
        let mut order = Vec::new();
        for slug in &subset {
            self.dfs_subset(slug, &subset, &mut visiting, &mut visited, &mut order)?;
        }
        Ok(order)
    }

    fn dfs_subset(
        &self,
        current: &str,
        subset: &BTreeSet<String>,
        visiting: &mut Vec<String>,
        visited: &mut HashSet<String>,
        order: &mut Vec<String>,
    ) -> Result<()> {
        if visited.contains(current) {
            return Ok(());
        }
        visiting.push(current.to_owned());
        if let Some(deps) = self.direct.get(current) {
            for (line, dep) in deps {
                if !subset.contains(dep) {
                    continue;
                }
                if let Some(pos) = visiting.iter().position(|s| s == dep) {
                    let mut cycle = visiting[pos..].to_vec();
                    cycle.push(dep.clone());
                    return Err(Error::DependencyCycle {
                        file: format!("snippets/{current}/code.hpp"),
                        line: *line,
                        cycle,
                    });
                }
                self.dfs_subset(dep, subset, visiting, visited, order)?;
            }
        }
        visiting.pop();
        visited.insert(current.to_owned());
        order.push(current.to_owned());
        Ok(())
    }
}

// ---- Single-file C++ Bundler -----------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum SandboxMode {
    WorkspaceOnly,
    LocalUnrestricted,
}

struct BundlerCtx<'a> {
    ws: &'a Workspace,
    mode: SandboxMode,
    include_dirs: Vec<PathBuf>,
    snippets_dir: PathBuf,
    compact: bool,
    boundary_comments: bool,
    system_includes: Vec<String>,
    seen_system: HashSet<String>,
    inlined_files: Vec<String>,
    inlined_snippets: Vec<String>,
    seen_snippets: HashSet<String>,
    edges: Vec<BundleIncludeEdge>,
    visiting: Vec<PathBuf>,
    visited: HashSet<PathBuf>,
}

/// Bundles a single workspace snippet (`snippets/<slug>/code.hpp`) and all its transitive
/// local `#include "..."` dependencies into one compilable C++ file.
pub fn bundle_snippet(ws: &Workspace, slug: &str, opts: &BundleOptions) -> Result<BundleResult> {
    xcpc_domain::check_id(slug)?;
    let rel = format!("snippets/{slug}/code.hpp");
    let abs = ws.resolve(&rel)?;
    if !abs.is_file() {
        return Err(Error::NotFound(format!("snippet code {rel:?}")));
    }
    let source = ws.read_text(&rel)?;
    bundle_internal(
        ws,
        SandboxMode::WorkspaceOnly,
        &rel,
        Some(&abs),
        &source,
        opts,
    )
}

/// Sandboxed workspace-only bundler (for MCP `bundle_code`).
///
/// Accepts either `rel_path` (inside the workspace) or inline `source_text` (or both, with
/// `rel_path` taking precedence for reading). Every `-I` directory and every resolved
/// `#include "..."` target is strictly checked against the workspace sandbox.
pub fn bundle_workspace_code(
    ws: &Workspace,
    rel_path: Option<&str>,
    source_text: Option<&str>,
    opts: &BundleOptions,
) -> Result<BundleResult> {
    let (display_name, disk_path, source) = if let Some(rel) = rel_path {
        let abs = ws.resolve(rel)?;
        let text = ws.read_text(rel)?;
        (rel.to_owned(), Some(abs), text)
    } else if let Some(text) = source_text {
        ("<input>".to_owned(), None, text.to_owned())
    } else {
        return Err(Error::Invalid {
            path: "<bundle>".to_owned(),
            message: "either rel_path or source_text must be provided".to_owned(),
        });
    };

    bundle_internal(
        ws,
        SandboxMode::WorkspaceOnly,
        &display_name,
        disk_path.as_deref(),
        &source,
        opts,
    )
}

/// Unrestricted local-file bundler (for CLI `xcpc bundle <file.cpp>` and GUI).
///
/// Automatically includes the source file's parent directory, any `-I` directories in `opts`,
/// and the workspace's `snippets/` directory.
pub fn bundle_local_file(
    ws: &Workspace,
    path: &Path,
    opts: &BundleOptions,
) -> Result<BundleResult> {
    let canonical = path.canonicalize().map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => Error::NotFound(path.display().to_string()),
        _ => io(path.display(), e),
    })?;
    let source = fs::read_to_string(&canonical).map_err(|e| io(canonical.display(), e))?;
    let display = path.to_string_lossy().into_owned();
    bundle_internal(
        ws,
        SandboxMode::LocalUnrestricted,
        &display,
        Some(&canonical),
        &source,
        opts,
    )
}

/// Unrestricted local source-text bundler (for pasted C++ code in GUI/CLI).
pub fn bundle_local_source(
    ws: &Workspace,
    source_text: &str,
    base_dir: Option<&Path>,
    opts: &BundleOptions,
) -> Result<BundleResult> {
    let fake_path = base_dir.map(|d| d.join("<input>.cpp"));
    bundle_internal(
        ws,
        SandboxMode::LocalUnrestricted,
        "<input>",
        fake_path.as_deref(),
        source_text,
        opts,
    )
}

fn bundle_internal(
    ws: &Workspace,
    mode: SandboxMode,
    display_name: &str,
    disk_path: Option<&Path>,
    source: &str,
    opts: &BundleOptions,
) -> Result<BundleResult> {
    let mut include_dirs = Vec::new();
    for dir in &opts.include_dirs {
        let trimmed = dir.trim();
        if trimmed.is_empty() {
            continue;
        }
        match mode {
            SandboxMode::WorkspaceOnly => {
                let resolved = ws.resolve(trimmed)?;
                include_dirs.push(resolved);
            }
            SandboxMode::LocalUnrestricted => {
                let p = PathBuf::from(trimmed);
                let resolved = p.canonicalize().unwrap_or(p);
                include_dirs.push(resolved);
            }
        }
    }
    let snippets_dir = ws.root().join("snippets");

    let mut ctx = BundlerCtx {
        ws,
        mode,
        include_dirs,
        snippets_dir,
        compact: opts.compact,
        boundary_comments: opts.include_boundary_comments && !opts.compact,
        system_includes: Vec::new(),
        seen_system: HashSet::new(),
        inlined_files: Vec::new(),
        inlined_snippets: Vec::new(),
        seen_snippets: HashSet::new(),
        edges: Vec::new(),
        visiting: Vec::new(),
        visited: HashSet::new(),
    };

    if let Some(p) = disk_path
        && p.is_file()
        && let Ok(canon) = p.canonicalize()
    {
        ctx.visiting.push(canon.clone());
        if let Some(slug) = snippet_slug_for_path(&ctx.snippets_dir, &canon)
            && ctx.seen_snippets.insert(slug.clone())
        {
            ctx.inlined_snippets.push(slug);
        }
    }

    let body = ctx.expand_source(display_name, disk_path, source, false)?;
    let trimmed_body = body.trim_start_matches('\n');

    let mut final_code = String::new();
    for sys in &ctx.system_includes {
        final_code.push_str("#include <");
        final_code.push_str(sys);
        final_code.push_str(">\n");
    }
    if !ctx.system_includes.is_empty() && !trimmed_body.trim().is_empty() {
        final_code.push('\n');
    }
    final_code.push_str(trimmed_body);

    let final_code = if ctx.compact {
        compact_cpp(&final_code)
    } else {
        normalize_trailing_newline(&final_code)
    };

    Ok(BundleResult {
        code: final_code,
        system_includes: ctx.system_includes,
        inlined_files: ctx.inlined_files,
        inlined_snippets: ctx.inlined_snippets,
        edges: ctx.edges,
    })
}

impl<'a> BundlerCtx<'a> {
    fn expand_source(
        &mut self,
        display_name: &str,
        disk_path: Option<&Path>,
        source: &str,
        is_inlined_header: bool,
    ) -> Result<String> {
        let normalized = source.replace("\r\n", "\n").replace('\r', "\n");
        let lines: Vec<&str> = normalized.lines().collect();
        let (start_idx, end_idx) = if is_inlined_header {
            strip_include_guard_range(&lines)
        } else {
            (0, lines.len())
        };

        let mut out = String::new();
        let mut in_block_comment = false;

        for (idx, &raw_line) in lines.iter().enumerate() {
            if idx < start_idx || idx >= end_idx {
                continue;
            }
            let line_no = idx + 1;
            let effective = strip_leading_block_comment(raw_line, &mut in_block_comment);
            let trimmed = effective.trim();

            if is_pragma_once_line(trimmed) {
                update_trailing_block_comment(raw_line, &mut in_block_comment);
                continue;
            }

            if let Some(sys_hdr) = parse_system_include_line(trimmed) {
                if self.seen_system.insert(sys_hdr.to_owned()) {
                    self.system_includes.push(sys_hdr.to_owned());
                }
                update_trailing_block_comment(raw_line, &mut in_block_comment);
                continue;
            }

            if let Some(local_inc) = parse_local_include_line(trimmed) {
                let target = self.resolve_include(display_name, disk_path, line_no, local_inc)?;
                let target_display = self.display_path(&target);
                let slug = snippet_slug_for_path(&self.snippets_dir, &target)
                    .or_else(|| extract_snippet_slug_from_include(local_inc));

                self.edges.push(BundleIncludeEdge {
                    from_file: display_name.to_owned(),
                    line: line_no,
                    raw_include: local_inc.to_owned(),
                    resolved_file: target_display.clone(),
                    snippet_slug: slug.clone(),
                });

                if let Some(pos) = self.visiting.iter().position(|p| p == &target) {
                    let mut cycle: Vec<String> = self.visiting[pos..]
                        .iter()
                        .map(|p| self.display_path(p))
                        .collect();
                    cycle.push(target_display);
                    return Err(Error::DependencyCycle {
                        file: display_name.to_owned(),
                        line: line_no,
                        cycle,
                    });
                }

                if self.visited.insert(target.clone()) {
                    let inc_src =
                        fs::read_to_string(&target).map_err(|e| io(&target_display, e))?;
                    self.visiting.push(target.clone());
                    let expanded =
                        self.expand_source(&target_display, Some(&target), &inc_src, true)?;
                    self.visiting.pop();

                    self.inlined_files.push(target_display.clone());
                    if let Some(s) = slug
                        && self.seen_snippets.insert(s.clone())
                    {
                        self.inlined_snippets.push(s);
                    }

                    if self.boundary_comments {
                        out.push_str("// --- ");
                        out.push_str(&target_display);
                        out.push_str(" ---\n");
                    }
                    out.push_str(expanded.trim_matches('\n'));
                    out.push('\n');
                }
                update_trailing_block_comment(raw_line, &mut in_block_comment);
                continue;
            }

            out.push_str(raw_line);
            out.push('\n');
            update_trailing_block_comment(raw_line, &mut in_block_comment);
        }

        Ok(out)
    }

    fn resolve_include(
        &self,
        from_display: &str,
        from_disk: Option<&Path>,
        line: usize,
        raw_inc: &str,
    ) -> Result<PathBuf> {
        let mut candidates: Vec<PathBuf> = Vec::new();
        if let Some(p) = from_disk
            && let Some(parent) = p.parent()
        {
            candidates.push(parent.join(raw_inc));
        }
        for dir in &self.include_dirs {
            candidates.push(dir.join(raw_inc));
        }
        candidates.push(self.snippets_dir.join(raw_inc));

        for cand in candidates {
            if !cand.is_file() {
                continue;
            }
            let canon = cand.canonicalize().map_err(|e| io(cand.display(), e))?;
            if self.mode == SandboxMode::WorkspaceOnly && !canon.starts_with(self.ws.root()) {
                return Err(Error::PathEscape(raw_inc.to_owned()));
            }
            return Ok(canon);
        }

        Err(Error::MissingInclude {
            file: from_display.to_owned(),
            line,
            include: raw_inc.to_owned(),
        })
    }

    fn display_path(&self, path: &Path) -> String {
        if let Ok(rel) = path.strip_prefix(&self.snippets_dir) {
            return path_to_slash(rel);
        }
        if let Ok(rel) = path.strip_prefix(self.ws.root()) {
            return path_to_slash(rel);
        }
        path.to_string_lossy().into_owned()
    }
}

fn path_to_slash(p: &Path) -> String {
    p.components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn snippet_slug_for_path(snippets_dir: &Path, canon: &Path) -> Option<String> {
    let rel = canon.strip_prefix(snippets_dir).ok()?;
    let rel_str = path_to_slash(rel);
    extract_snippet_slug_from_include(&rel_str)
}

/// Detects a classic header include guard (`#ifndef MACRO` / `#define MACRO` ... `#endif`)
/// wrapping the entire file and returns the inner `[start, end)` line range to keep.
fn strip_include_guard_range(lines: &[&str]) -> (usize, usize) {
    let n = lines.len();
    let mut first = 0;
    while first < n && is_blank_or_line_comment(lines[first]) {
        first += 1;
    }
    if first + 2 >= n {
        return (0, n);
    }

    let Some(guard_macro) = parse_ifndef_macro(lines[first].trim()) else {
        return (0, n);
    };

    let mut second = first + 1;
    while second < n && is_blank_or_line_comment(lines[second]) {
        second += 1;
    }
    if second >= n {
        return (0, n);
    }
    if parse_define_macro(lines[second].trim()) != Some(guard_macro) {
        return (0, n);
    }

    let mut last = n;
    while last > second + 1 && is_blank_or_line_comment(lines[last - 1]) {
        last -= 1;
    }
    if last <= second + 1 || !is_endif_line(lines[last - 1].trim()) {
        return (0, n);
    }

    // Verify that the `#ifndef` at `first` stays open until `last - 1` (depth never drops to 0 earlier).
    let mut depth = 1i32;
    for &line in &lines[second + 1..last - 1] {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix('#').map(str::trim_start) {
            if rest.starts_with("if")
                && rest[2..]
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_whitespace() || c == '(' || c == 'n' || c == 'd')
            {
                if rest.starts_with("ifdef")
                    || rest.starts_with("ifndef")
                    || rest.starts_with("if ")
                    || rest.starts_with("if\t")
                    || rest.starts_with("if(")
                {
                    depth += 1;
                }
            } else if rest.starts_with("endif") {
                depth -= 1;
                if depth == 0 {
                    return (0, n);
                }
            }
        }
    }

    if depth == 1 {
        (second + 1, last - 1)
    } else {
        (0, n)
    }
}

fn is_blank_or_line_comment(line: &str) -> bool {
    let t = line.trim();
    t.is_empty() || t.starts_with("//")
}

fn parse_ifndef_macro(trimmed: &str) -> Option<&str> {
    let rest = trimmed.strip_prefix('#')?.trim_start();
    if let Some(after) = rest.strip_prefix("ifndef") {
        if !after.starts_with(|c: char| c.is_ascii_whitespace()) {
            return None;
        }
        let name = after.split_whitespace().next()?;
        return (!name.is_empty()).then_some(name);
    }
    if let Some(after) = rest.strip_prefix("if") {
        let after = after.trim_start();
        let after = after.strip_prefix("!defined")?.trim_start();
        let name = if let Some(inner) = after.strip_prefix('(') {
            inner.split(')').next()?.trim()
        } else {
            after.split_whitespace().next()?
        };
        return (!name.is_empty()).then_some(name);
    }
    None
}

fn parse_define_macro(trimmed: &str) -> Option<&str> {
    let rest = trimmed.strip_prefix('#')?.trim_start();
    let after = rest.strip_prefix("define")?;
    if !after.starts_with(|c: char| c.is_ascii_whitespace()) {
        return None;
    }
    let name = after.split_whitespace().next()?;
    (!name.is_empty()).then_some(name)
}

fn is_endif_line(trimmed: &str) -> bool {
    let Some(rest) = trimmed.strip_prefix('#').map(str::trim_start) else {
        return false;
    };
    let Some(after) = rest.strip_prefix("endif") else {
        return false;
    };
    let after = after.trim_start();
    after.is_empty() || after.starts_with("//") || after.starts_with("/*")
}

fn strip_leading_block_comment<'a>(line: &'a str, in_block_comment: &mut bool) -> &'a str {
    if !*in_block_comment {
        return line;
    }
    if let Some(end) = line.find("*/") {
        *in_block_comment = false;
        &line[end + 2..]
    } else {
        ""
    }
}

fn update_trailing_block_comment(line: &str, in_block_comment: &mut bool) {
    let bytes = line.as_bytes();
    let n = bytes.len();
    let mut i = 0;
    while i < n {
        if *in_block_comment {
            if bytes[i] == b'*' && i + 1 < n && bytes[i + 1] == b'/' {
                *in_block_comment = false;
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        if bytes[i] == b'/' && i + 1 < n && bytes[i + 1] == b'/' {
            break;
        }
        if bytes[i] == b'/' && i + 1 < n && bytes[i + 1] == b'*' {
            *in_block_comment = true;
            i += 2;
            continue;
        }
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            let q = bytes[i];
            i += 1;
            while i < n {
                if bytes[i] == b'\\' {
                    i += 2;
                    continue;
                }
                if bytes[i] == q {
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        i += 1;
    }
}

/// Strips comments and collapses multiple consecutive blank lines for `--compact` bundling.
fn compact_cpp(code: &str) -> String {
    let no_comments = strip_cpp_comments(code);
    let mut out = Vec::new();
    let mut prev_blank = true;
    for raw in no_comments.lines() {
        let trimmed_end = raw.trim_end();
        if trimmed_end.is_empty() {
            if !prev_blank {
                out.push("");
                prev_blank = true;
            }
        } else {
            out.push(trimmed_end);
            prev_blank = false;
        }
    }
    while out.last().is_some_and(|l| l.is_empty()) {
        out.pop();
    }
    if out.is_empty() {
        String::new()
    } else {
        out.join("\n") + "\n"
    }
}

fn normalize_trailing_newline(code: &str) -> String {
    let trimmed = code.trim_end_matches('\n');
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("{trimmed}\n")
    }
}
