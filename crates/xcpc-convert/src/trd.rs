//! Living TRD (Team Reference Document) and single-snippet Typst document generation.

use std::collections::BTreeMap;

use xcpc_domain::{Language, PrintedSnippetCode, SnippetDetail, TemplateOptionValue, TrdMeta};
use xcpc_storage::{preprocess_printed_code, strip_cpp_comments};

use crate::{Document, Out, markdown, string, typst_options_dict, typst_string_array};

/// Default contest-hall verification one-liner printed on TRD covers.
pub const CONTEST_HASH_VERIFY_CMD: &str =
    "sed -n '1,Np' a.cpp | cpp -dD -P -fpreprocessed | tr -d '[:space:]' | md5sum | cut -c-6";

/// Book-level configuration for a TRD or single-snippet preview.
#[derive(Debug, Clone)]
pub struct TrdBook {
    pub name: String,
    pub author: Option<String>,
    pub date: Option<String>,
    pub language: Language,
    /// `true` when rendering a single snippet preview (Studio right-hand preview).
    pub single: bool,
    /// Resolved template ID (defaults to `"trd-compact"`).
    pub template_name: String,
    /// Typst module import path (e.g. `"/templates/trd-compact.typ"` or `"/xcpc/trd-compact.typ"`).
    pub template_import: Option<String>,
    /// Merged book-level template options.
    pub template_options: BTreeMap<String, TemplateOptionValue>,
    /// Optional workspace/snapshot `preamble.typ` injected before `#__xcpc_tpl.render(...)`.
    pub preamble: Option<String>,
    /// Mapping of snippet `slug -> title` so single-snippet previews can resolve dependency titles.
    pub snippet_titles: BTreeMap<String, String>,
}

impl TrdBook {
    pub fn from_meta(meta: &TrdMeta, language: Language) -> Self {
        Self {
            name: meta.name.clone(),
            author: meta.author.clone(),
            date: meta.date.clone(),
            language,
            single: false,
            template_name: meta
                .template
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("trd-compact")
                .to_owned(),
            template_import: None,
            template_options: BTreeMap::new(),
            preamble: None,
            snippet_titles: BTreeMap::new(),
        }
    }

    pub fn single(title: &str, language: Language) -> Self {
        Self {
            name: title.to_owned(),
            author: None,
            date: None,
            language,
            single: true,
            template_name: "trd-compact".into(),
            template_import: None,
            template_options: BTreeMap::new(),
            preamble: None,
            snippet_titles: BTreeMap::new(),
        }
    }
}

/// One TRD chapter with its resolved snippets.
pub struct TrdChapterInput<'a> {
    pub title: String,
    pub items: Vec<&'a SnippetDetail>,
}

fn option_hash_interval(opts: &BTreeMap<String, TemplateOptionValue>) -> usize {
    match opts.get("hash-interval") {
        Some(TemplateOptionValue::Int(i)) => (*i).max(0) as usize,
        Some(TemplateOptionValue::Float(f)) if f.is_finite() => (*f).max(0.0).round() as usize,
        Some(TemplateOptionValue::String(s)) => s.trim().parse::<usize>().unwrap_or(10),
        _ => 10,
    }
}

fn option_strip_comments(opts: &BTreeMap<String, TemplateOptionValue>) -> bool {
    matches!(
        opts.get("strip-comments"),
        Some(TemplateOptionValue::Bool(true))
    )
}

/// Preprocesses snippet `code.hpp` according to TRD options (`hash-interval`, `strip-comments`).
///
/// When `strip_comments` is `true`, C/C++ comments are removed and blank-only lines
/// collapsed *before* computing the cumulative KACTL++ hashes so every printed hash
/// matches the exact lines printed on paper.
pub fn prepare_printed_code(
    raw_code: &str,
    hash_interval: usize,
    strip_comments: bool,
) -> PrintedSnippetCode {
    let base = preprocess_printed_code(raw_code, hash_interval);
    if !strip_comments || base.code.is_empty() {
        return base;
    }
    let stripped_deps = base.stripped_deps;
    let without_comments = strip_cpp_comments(&base.code);
    let mut cleaned_lines: Vec<String> = Vec::new();
    let mut prev_blank = false;
    for line in without_comments.lines() {
        let trimmed_end = line.trim_end();
        if trimmed_end.trim().is_empty() {
            if !prev_blank && !cleaned_lines.is_empty() {
                cleaned_lines.push(String::new());
                prev_blank = true;
            }
        } else {
            cleaned_lines.push(trimmed_end.to_owned());
            prev_blank = false;
        }
    }
    while cleaned_lines.last().is_some_and(|l| l.trim().is_empty()) {
        cleaned_lines.pop();
    }
    let cleaned_src = cleaned_lines.join("\n");
    let mut recomputed = preprocess_printed_code(&cleaned_src, hash_interval);
    recomputed.stripped_deps = stripped_deps;
    recomputed
}

/// Generates a Typst [`Document`] for a TRD book or single-snippet preview.
pub fn render_trd(book: &TrdBook, chapters: &[TrdChapterInput]) -> Document {
    let mut out = Out::new();
    let mut diagnostics = Vec::new();
    let mut items_meta: Vec<(String, String)> = Vec::new();

    let hash_interval = option_hash_interval(&book.template_options);
    let strip_comments = option_strip_comments(&book.template_options);

    let template = book
        .template_import
        .as_deref()
        .unwrap_or("/xcpc/trd-compact.typ");
    let tpl_str = string(template);

    out.raw(&format!(
        "// Generated by xcpc from the workspace: edit the workspace, not this.\n\
         #import \"/xcpc/defaults.typ\": section, samples, subsection, runin\n\
         #import {tpl_str} as __xcpc_tpl\n\
         #import {tpl_str}: *\n\
         #import \"/xcpc/markdown.typ\": m, dm\n\n"
    ));

    if let Some(preamble) = book.preamble.as_deref().filter(|s| !s.trim().is_empty()) {
        let file = out.file("templates/preamble.typ", preamble);
        let mut offset = 0usize;
        for line in preamble.split_inclusive('\n') {
            out.mark(file, offset);
            out.raw(line);
            offset += line.len();
        }
        if !preamble.ends_with('\n') {
            out.raw("\n");
        }
        out.unmark();
        out.raw("\n");
    }

    let opt = |v: &Option<String>| {
        v.as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map_or("none".into(), string)
    };
    let pdf_title = if book.name.trim().is_empty() {
        "none".to_owned()
    } else {
        string(book.name.trim())
    };
    let pdf_author = opt(&book.author);

    let titles_lit = if book.snippet_titles.is_empty() {
        "(:)".to_owned()
    } else {
        let entries: Vec<String> = book
            .snippet_titles
            .iter()
            .map(|(k, v)| format!("{}: {}", string(k), string(v)))
            .collect();
        format!("({},)", entries.join(", "))
    };

    out.raw(&format!(
        "#__xcpc_tpl.render((\n\
         name: {},\n\
         author: {},\n\
         date: {},\n\
         language: {},\n\
         single: {},\n\
         pdf-title: {pdf_title},\n\
         pdf-author: {pdf_author},\n\
         verify-cmd: {},\n\
         options: {},\n\
         snippet-titles: {titles_lit},\n\
         chapters: (\n",
        string(&book.name),
        opt(&book.author),
        opt(&book.date),
        string(book.language.name()),
        book.single,
        string(CONTEST_HASH_VERIFY_CMD),
        typst_options_dict(&book.template_options),
    ));

    for ch in chapters {
        out.raw(&format!("(\ntitle: {},\nitems: (\n", string(&ch.title)));
        for snippet in &ch.items {
            items_meta.push((snippet.slug.clone(), snippet.slug.clone()));

            let printed = snippet
                .code
                .as_ref()
                .map(|c| prepare_printed_code(&c.text, hash_interval, strip_comments));

            let (has_code, code_str, deps, hash_blocks_lit, total_hash) = match &printed {
                Some(p) => {
                    let deps = if !p.stripped_deps.is_empty() {
                        p.stripped_deps.clone()
                    } else {
                        snippet.reverse_refs.deps.clone()
                    };
                    let blocks: Vec<String> = p
                        .hash_blocks
                        .iter()
                        .map(|b| {
                            format!(
                                "(line: {}, hash: {}, is-final: {})",
                                b.line,
                                string(&b.hash),
                                b.is_final
                            )
                        })
                        .collect();
                    let blocks_lit = if blocks.is_empty() {
                        "()".to_owned()
                    } else {
                        format!("({},)", blocks.join(", "))
                    };
                    (
                        !p.code.is_empty(),
                        p.code.as_str(),
                        deps,
                        blocks_lit,
                        p.total_hash.as_str(),
                    )
                }
                None => (
                    false,
                    "",
                    snippet.reverse_refs.deps.clone(),
                    "()".to_owned(),
                    "",
                ),
            };

            let title = if snippet.meta.title.trim().is_empty() {
                snippet.slug.as_str()
            } else {
                snippet.meta.title.as_str()
            };

            out.raw(&format!(
                "(\n\
                 slug: {},\n\
                 title: {},\n\
                 tags: {},\n\
                 time-complexity: {},\n\
                 space-complexity: {},\n\
                 description: {},\n\
                 deps: {},\n\
                 has-code: {has_code},\n\
                 code-lang: \"cpp\",\n\
                 code: {},\n\
                 hash-blocks: {hash_blocks_lit},\n\
                 total-hash: {},\n",
                string(&snippet.slug),
                string(title),
                typst_string_array(&snippet.meta.tags),
                opt(&snippet.meta.time_complexity),
                opt(&snippet.meta.space_complexity),
                opt(&snippet.meta.description),
                typst_string_array(&deps),
                string(code_str),
                string(total_hash),
            ));

            match &snippet.notes {
                Some(notes) if !notes.text.trim().is_empty() => {
                    out.raw("notes: [\n");
                    let file = out.file(&notes.path, &notes.text);
                    markdown::convert(
                        markdown::Body {
                            text: &notes.text,
                            file,
                            path: &notes.path,
                            dir: &snippet.dir,
                            editorial: true,
                            samples: None,
                        },
                        &mut out,
                        &mut diagnostics,
                    );
                    out.trim_end();
                    out.unmark();
                    out.raw("\n],\n");
                }
                _ => {
                    out.raw("notes: none,\n");
                }
            }

            out.raw("),\n");
        }
        out.raw("),\n),\n");
    }

    out.raw("),\n))\n");
    out.finish(items_meta, diagnostics)
}
