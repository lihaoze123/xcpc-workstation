//! Workspace content → a Typst document in `olymp-classic`: the glue between the
//! problem library and the engine (docs/architecture.md §6).
//!
//! [`statements`] sets a problem book (one problem, or a contest with its cover) and
//! [`editorials`] the A4 editorial book. The result is a Typst source for the engine's
//! main file with a [`SourceMap`] back to the Markdown, so the engine's diagnostics can
//! point into `statement.md` instead of the generated code.

use xcpc_domain::{ContestItem, ContestMeta, Language, Limits, Problem, ProblemMeta};

mod markdown;
pub mod publish;
mod spans;

pub use spans::{Span, SpanKind, markdown_spans};

/// A diagnostic in a workspace file (1-based line and column).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub error: bool,
    pub code: Option<String>,
    pub message: String,
    pub file: String,
    pub line: usize,
    pub column: usize,
}

impl Diagnostic {
    fn at(path: &str, text: &str, offset: usize, error: bool, message: String) -> Self {
        Self::at_with_code(path, text, offset, error, None, message)
    }

    fn at_with_code(
        path: &str,
        text: &str,
        offset: usize,
        error: bool,
        code: Option<&str>,
        message: String,
    ) -> Self {
        let (line, column) = line_column(text, offset);
        Self {
            error,
            code: code.map(str::to_owned),
            message,
            file: path.to_owned(),
            line,
            column,
        }
    }
}

fn line_column(text: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(text.len());
    let before = &text[..text.floor_char_boundary(offset)];
    let line = before.matches('\n').count() + 1;
    let column = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
    (line, column)
}

/// Where the generated code came from.
#[derive(Debug, Clone, Default)]
pub struct SourceMap {
    /// The workspace files converted, by index: path and text.
    files: Vec<(String, String)>,
    /// `(offset in the generated source, Some((file, offset in the file)))`, by the
    /// first; `None` marks generated regions (e.g. between problems) with no Markdown source.
    marks: Vec<(usize, Option<(usize, usize)>)>,
}

impl SourceMap {
    /// The file, line and column (1-based) that the generated source's byte `offset`
    /// came from, if it came from one.
    pub fn locate(&self, offset: usize) -> Option<(&str, usize, usize)> {
        let i = self
            .marks
            .partition_point(|m| m.0 <= offset)
            .checked_sub(1)?;
        let (file, at) = self.marks[i].1?;
        let (path, text) = &self.files[file];
        let (line, column) = line_column(text, at);
        Some((path, line, column))
    }
}

/// A generated Typst document.
#[derive(Debug, Clone)]
pub struct Document {
    pub source: String,
    pub map: SourceMap,
    /// What converting found: formulas that do not convert, pictures outside the
    /// problem, a missing statement.
    pub diagnostics: Vec<Diagnostic>,
    /// The problems set, in order, with their label.
    pub problems: Vec<(String, String)>,
}

/// The book's running head, cover and switches.
#[derive(Debug, Clone)]
pub struct Book {
    pub name: String,
    pub location: Option<String>,
    pub date: Option<String>,
    pub author: Option<String>,
    pub language: Language,
    pub show_metadata: bool,
    pub cover: bool,
    pub blank_pages: bool,
    /// One problem on its own, not a contest's.
    pub single: bool,
}

impl Book {
    /// A contest's book.
    pub fn contest(meta: &ContestMeta) -> Self {
        Self {
            name: meta.name.clone(),
            location: meta.location.clone(),
            date: meta.date.clone(),
            author: meta.author.clone(),
            language: meta.language,
            show_metadata: meta.show_metadata,
            cover: meta.has_cover(),
            blank_pages: meta.blank_pages,
            single: false,
        }
    }

    /// One problem on its own, as the reference template's standalone problem: a
    /// running head of "题面预览" (Chinese) and no cover.
    pub fn single(language: Language) -> Self {
        Self {
            name: match language {
                Language::Chinese => "题面预览".into(),
                Language::English => String::new(),
            },
            location: None,
            date: None,
            author: None,
            language,
            show_metadata: true,
            cover: false,
            blank_pages: false,
            single: true,
        }
    }
}

/// A problem of a book: its label and the contest's overrides.
pub struct Entry<'a> {
    pub item: ContestItem,
    pub problem: &'a Problem,
}

impl<'a> Entry<'a> {
    /// A problem on its own: labelled A, as the reference's standalone problem is.
    pub fn single(problem: &'a Problem) -> Self {
        Self {
            item: ContestItem {
                id: problem.id.clone(),
                label: "A".into(),
                ..Default::default()
            },
            problem,
        }
    }
}

/// The problem book: each problem's statement with its limits and samples.
pub fn statements(book: &Book, entries: &[Entry]) -> Document {
    let mut out = Out::new();
    head(&mut out, book, "statements", book.cover, book.blank_pages);
    let mut diagnostics = Vec::new();
    for entry in entries {
        let meta = entry.item.apply(&entry.problem.meta);
        let show = entry.item.show_metadata.unwrap_or(book.show_metadata);
        let (time, memory) = limits(&meta.limits, book.language);
        let file = |name: &Option<String>| match name.as_deref() {
            None | Some("") => "none".to_owned(),
            Some(f) => string(f),
        };
        out.raw(&format!(
            "#problem(label: {}, title: {}, input: {}, output: {}, time-limit: {}, memory-limit: {}, show-metadata: {show})[\n",
            string(&entry.item.label),
            string(&meta.title),
            file(&meta.limits.input),
            file(&meta.limits.output),
            time.as_deref().map_or("none".into(), string),
            memory.as_deref().map_or("none".into(), string),
        ));
        let samples = (!entry.problem.samples.is_empty()).then(|| {
            let pairs: Vec<String> = entry
                .problem
                .samples
                .iter()
                .map(|s| format!("({}, {})", string(&s.input), string(&s.output)))
                .collect();
            format!("#samples({},)", pairs.join(", "))
        });
        match entry.problem.statement(book.language) {
            Some(statement) => {
                let file = out.file(&statement.file.path, &statement.file.text);
                markdown::convert(
                    markdown::Body {
                        text: &statement.file.text,
                        file,
                        path: &statement.file.path,
                        dir: &entry.problem.dir,
                        editorial: false,
                        samples,
                    },
                    &mut out,
                    &mut diagnostics,
                );
            }
            None => {
                diagnostics.push(Diagnostic {
                    error: false,
                    code: None,
                    message: format!("problem {:?} has no statement", entry.problem.id),
                    file: format!("{}/statement.md", entry.problem.dir),
                    line: 1,
                    column: 1,
                });
                if let Some(code) = samples {
                    out.block(&code);
                }
            }
        }
        out.trim_end();
        out.unmark();
        out.raw("\n]\n\n");
    }
    let set = entries
        .iter()
        .map(|e| (e.item.label.clone(), e.problem.id.clone()))
        .collect();
    out.finish(set, diagnostics)
}

/// The editorial book: each problem's `editorial.md` from a new page, under its label,
/// title and what is known about it; a contest's with a title page and contents (when
/// its statements have a cover). Problems without one are left out, with a warning.
pub fn editorials(book: &Book, entries: &[Entry]) -> Document {
    let mut out = Out::new();
    // A problem on its own: the running head says "Editorial" and its title.
    let book = &Book {
        name: if book.single {
            String::new()
        } else {
            book.name.clone()
        },
        ..book.clone()
    };
    head(
        &mut out,
        book,
        "editorial",
        book.cover && !book.single,
        false,
    );
    let mut diagnostics = Vec::new();
    let mut set = Vec::new();
    for entry in entries {
        let meta = entry.item.apply(&entry.problem.meta);
        let Some(editorial) = &entry.problem.editorial else {
            diagnostics.push(Diagnostic {
                error: false,
                code: Some(xcpc_lint::code::MISSING_EDITORIAL.to_owned()),
                message: format!(
                    "problem {} ({}) has no editorial",
                    entry.item.label, entry.problem.id
                ),
                file: format!("{}/editorial.md", entry.problem.dir),
                line: 1,
                column: 1,
            });
            continue;
        };
        set.push((entry.item.label.clone(), entry.problem.id.clone()));
        let label = if book.single {
            "none".into()
        } else {
            string(&entry.item.label)
        };
        let about: Vec<String> = about(&meta, book.language)
            .iter()
            .map(|m| string(m))
            .collect();
        out.raw(&format!(
            "#tutorial(label: {label}, title: {}, meta: ({}))[\n",
            string(&meta.title),
            about.iter().map(|m| format!("{m},")).collect::<String>(),
        ));
        let file = out.file(&editorial.path, &editorial.text);
        markdown::convert(
            markdown::Body {
                text: &editorial.text,
                file,
                path: &editorial.path,
                dir: &entry.problem.dir,
                editorial: true,
                samples: None,
            },
            &mut out,
            &mut diagnostics,
        );
        out.trim_end();
        out.unmark();
        out.raw("\n]\n\n");
    }
    out.finish(set, diagnostics)
}

/// What an editorial's head says about a problem, besides its title: its author, tags
/// and sources (`Codeforces 1999A`), each one item. Not its rating.
fn about(meta: &ProblemMeta, language: Language) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(author) = meta.author.as_deref().filter(|a| !a.trim().is_empty()) {
        out.push(match language {
            Language::Chinese => format!("作者 {author}"),
            Language::English => format!("by {author}"),
        });
    }
    out.extend(meta.tags.iter().cloned());
    out.extend(meta.sources.iter().map(|s| {
        let platform = match s.platform.as_str() {
            "codeforces" => "Codeforces",
            "atcoder" => "AtCoder",
            "qoj" => "QOJ",
            "luogu" => "Luogu",
            "nowcoder" => "NowCoder",
            "leetcode" => "LeetCode",
            other => other,
        };
        format!("{platform} {}", s.id)
    }));
    out
}

fn head(out: &mut Out, book: &Book, kind: &str, cover: bool, blank_pages: bool) {
    let opt = |v: &Option<String>| v.as_deref().map_or("none".into(), string);
    out.raw(&format!(
        "// Generated by xcpc from the workspace: edit the workspace, not this.\n\
         #import \"/xcpc/olymp-classic.typ\": *\n\
         #import \"/xcpc/markdown.typ\": m, dm\n\n\
         #show: contest.with(name: {}, location: {}, date: {}, author: {}, language: {}, show-cover: {cover}, blank-pages: {}, book: {})\n\n",
        string(&book.name),
        opt(&book.location),
        opt(&book.date),
        opt(&book.author),
        string(book.language.name()),
        blank_pages,
        string(kind),
    ));
}

/// The time and memory limits as the template shows them: `1 秒` / `2 seconds`,
/// `256 MB` / `256 megabytes`. What does not parse shows as written.
pub fn limits(limits: &Limits, language: Language) -> (Option<String>, Option<String>) {
    let time = limits
        .time
        .as_deref()
        .filter(|t| !t.trim().is_empty())
        .map(|t| {
            let Some((n, unit)) = number(t) else {
                return t.to_owned();
            };
            let ms = match unit.to_lowercase().as_str() {
                "s" | "sec" | "secs" | "second" | "seconds" | "秒" => false,
                "ms" | "millisecond" | "milliseconds" | "毫秒" => true,
                _ => return t.to_owned(),
            };
            match (language, ms) {
                (Language::Chinese, false) => format!("{n} 秒"),
                (Language::Chinese, true) => format!("{n} 毫秒"),
                (Language::English, false) => format!("{n} {}", plural(&n, "second")),
                (Language::English, true) => format!("{n} {}", plural(&n, "millisecond")),
            }
        });
    let memory = limits
        .memory
        .as_deref()
        .filter(|t| !t.trim().is_empty())
        .map(|t| {
            let Some((n, unit)) = number(t) else {
                return t.to_owned();
            };
            let (short, long) = match unit.to_lowercase().as_str() {
                "kb" | "k" => ("KB", "kilobyte"),
                "mb" | "m" => ("MB", "megabyte"),
                "gb" | "g" => ("GB", "gigabyte"),
                "kib" => ("KiB", "kibibyte"),
                "mib" => ("MiB", "mebibyte"),
                "gib" => ("GiB", "gibibyte"),
                _ => return t.to_owned(),
            };
            match language {
                Language::Chinese => format!("{n} {short}"),
                Language::English => format!("{n} {}", plural(&n, long)),
            }
        });
    (time, memory)
}

/// `"1.5s"` → `("1.5", "s")`.
fn number(text: &str) -> Option<(String, String)> {
    let text = text.trim();
    let end = text
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(text.len());
    let (n, unit) = text.split_at(end);
    if n.is_empty() || n.parse::<f64>().is_err() {
        return None;
    }
    Some((n.to_owned(), unit.trim().to_owned()))
}

fn plural(n: &str, word: &str) -> String {
    if n == "1" {
        word.to_owned()
    } else {
        format!("{word}s")
    }
}

/// A Typst string literal.
pub(crate) fn string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Whether the line `buf` ends with is digits only: a `.` would make it a list item.
fn line_digits(buf: &str) -> bool {
    let line = buf.rsplit(['\n', '[']).next().unwrap_or("");
    !line.is_empty() && line.bytes().all(|b| b.is_ascii_digit())
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x2E80..=0x9FFF | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFFEF | 0x20000..=0x3FFFF)
}

/// The generated source as it is written.
pub(crate) struct Out {
    buf: String,
    map: SourceMap,
    /// Side buffers for headings, code and alternative text: markup and plain text.
    captures: Vec<(String, String)>,
    /// A soft line break waits for the next text: a space, or nothing between CJK.
    soft: bool,
}

impl Out {
    fn new() -> Self {
        Self {
            buf: String::new(),
            map: SourceMap::default(),
            captures: Vec::new(),
            soft: false,
        }
    }

    fn file(&mut self, path: &str, text: &str) -> usize {
        self.map.files.push((path.to_owned(), text.to_owned()));
        self.map.files.len() - 1
    }

    fn finish(self, problems: Vec<(String, String)>, diagnostics: Vec<Diagnostic>) -> Document {
        Document {
            source: self.buf,
            map: self.map,
            diagnostics,
            problems,
        }
    }

    fn cur(&mut self) -> &mut String {
        match self.captures.last_mut() {
            Some((markup, _)) => markup,
            None => &mut self.buf,
        }
    }

    pub(crate) fn mark(&mut self, file: usize, at: usize) {
        self.push_mark(Some((file, at)));
    }

    pub(crate) fn unmark(&mut self) {
        self.push_mark(None);
    }

    fn push_mark(&mut self, target: Option<(usize, usize)>) {
        if self.captures.is_empty() {
            let here = self.buf.len();
            // Trimming may have taken back what earlier marks point past.
            while self.map.marks.last().is_some_and(|m| m.0 > here) {
                self.map.marks.pop();
            }
            if let Some(last) = self.map.marks.last_mut().filter(|m| m.0 == here) {
                *last = (here, target);
            } else {
                self.map.marks.push((here, target));
            }
        }
    }

    pub(crate) fn capture(&mut self) {
        self.flush_soft(false);
        self.captures.push(Default::default());
    }

    pub(crate) fn release(&mut self) -> (String, String) {
        self.soft = false;
        self.captures.pop().unwrap_or_default()
    }

    fn flush_soft(&mut self, next_cjk: bool) {
        if std::mem::take(&mut self.soft) {
            let prev_cjk = self.cur().chars().last().is_some_and(is_cjk);
            if !(prev_cjk && next_cjk) {
                self.cur().push(' ');
            }
        }
    }

    pub(crate) fn soft_break(&mut self) {
        self.soft = true;
        if let Some((_, plain)) = self.captures.last_mut() {
            plain.push(' ');
        }
    }

    /// Text, escaped.
    pub(crate) fn text(&mut self, text: &str) {
        self.flush_soft(text.chars().next().is_some_and(is_cjk));
        if let Some((_, plain)) = self.captures.last_mut() {
            plain.push_str(text);
        }
        let buf = self.cur();
        for c in text.chars() {
            match c {
                '\\' | '#' | '$' | '*' | '_' | '`' | '<' | '>' | '@' | '[' | ']' | '~' | '/'
                | '=' | '+' | '-' => {
                    buf.push('\\');
                    buf.push(c);
                }
                // `1.` at the start of a line would be a numbered list.
                '.' if line_digits(buf) => buf.push_str("\\."),
                '\n' | '\r' => buf.push(' '),
                c => buf.push(c),
            }
        }
    }

    /// Inline code (a call ending in `;`).
    pub(crate) fn inline(&mut self, code: &str) {
        self.flush_soft(false);
        self.cur().push_str(code);
    }

    /// Structural text, as is.
    pub(crate) fn raw(&mut self, code: &str) {
        self.flush_soft(false);
        self.cur().push_str(code);
    }

    /// A block on its own lines.
    pub(crate) fn block(&mut self, code: &str) {
        self.block_start(code);
        self.cur().push_str("\n\n");
    }

    /// The start of a block on its own line.
    pub(crate) fn block_start(&mut self, code: &str) {
        self.soft = false;
        let buf = self.cur();
        let keep = buf.trim_end_matches([' ', '\t']).len();
        buf.truncate(keep);
        if !(buf.is_empty() || buf.ends_with('\n') || buf.ends_with('[')) {
            buf.push('\n');
        }
        buf.push_str(code);
    }

    pub(crate) fn par_end(&mut self) {
        self.soft = false;
        let buf = self.cur();
        if !(buf.ends_with("\n\n") || buf.ends_with('[')) {
            buf.push_str("\n\n");
        }
    }

    pub(crate) fn trim_end(&mut self) {
        self.soft = false;
        let buf = self.cur();
        let keep = buf.trim_end().len();
        buf.truncate(keep);
    }
}

#[cfg(test)]
mod tests;
