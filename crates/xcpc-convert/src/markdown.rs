//! Markdown + Math → Typst markup: a thin layer over `pulldown-cmark`'s events and
//! `mitex`'s math, no parsing of our own.
//!
//! Text is escaped; inline structure becomes function calls (`#strong[…];`), so nothing
//! depends on markup characters meeting the right neighbours; math goes through `mitex`
//! and is evaluated by `m` / `dm` of the bundled `/xcpc/markdown.typ` with mitex's scope.
//! Second-level headings become the template's sections (docs/product.md §2.1) and the
//! samples table goes where the statement asks for it, or after the input and output.

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use xcpc_domain::{SectionKind, recognize_section};

use crate::{Diagnostic, Out, string};

/// How to convert one Markdown file.
pub(crate) struct Body<'a> {
    pub text: &'a str,
    /// The file, relative to the workspace (for diagnostics and the source map).
    pub file: usize,
    pub path: &'a str,
    /// The problem's directory, relative to the workspace: pictures resolve against it.
    pub dir: &'a str,
    /// An editorial: headings are its own (`##` a subsection, deeper ones run in), not
    /// a statement's sections.
    pub editorial: bool,
    /// The samples table (`#samples(…)`), if there are samples.
    pub samples: Option<String>,
}

/// An open element whose closing text we owe.
enum Open {
    /// Text up to the end goes to a side buffer: a heading's, a code block's, an
    /// image's alternative text.
    Capture {
        kind: Capture,
        start: usize,
    },
    /// A fenced `typst` block passed directly into the `olymp-classic` scope with
    /// line-by-line source mapping.
    RawTypst,
    Close(&'static str),
    Item,
}

#[derive(PartialEq)]
enum Capture {
    Heading(HeadingLevel),
    Code(Option<String>),
    Image(String),
}

/// The Markdown dialect: GFM with math, no smart punctuation.
pub(crate) fn options() -> Options {
    let mut options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_MATH
        | Options::ENABLE_GFM
        | Options::ENABLE_SUPERSCRIPT
        | Options::ENABLE_SUBSCRIPT;
    options.remove(Options::ENABLE_SMART_PUNCTUATION);
    options
}

pub(crate) fn convert(body: Body, out: &mut Out, diagnostics: &mut Vec<Diagnostic>) {
    let options = options();
    let mut stack: Vec<Open> = Vec::new();
    let mut samples = body.samples.clone();
    // The section we are in: after the input and output sections, the next other
    // heading gets the samples first.
    let mut in_io = false;
    let place_samples = |out: &mut Out, samples: &mut Option<String>| {
        if let Some(code) = samples.take() {
            out.block(&code);
        }
    };
    let warn = |diagnostics: &mut Vec<Diagnostic>, at: usize, message: String| {
        diagnostics.push(Diagnostic::at(body.path, body.text, at, false, message));
    };

    let mut iter = Parser::new_ext(body.text, options)
        .into_offset_iter()
        .peekable();
    while let Some((event, range)) = iter.next() {
        out.mark(body.file, range.start);
        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => {}
                Tag::Heading { level, .. } => {
                    out.capture();
                    stack.push(Open::Capture {
                        kind: Capture::Heading(level),
                        start: range.start,
                    });
                }
                Tag::BlockQuote(_) => {
                    out.block_start("#quote(block: true)[");
                    stack.push(Open::Close("]"));
                }
                Tag::CodeBlock(kind) => {
                    let lang = match kind {
                        CodeBlockKind::Fenced(info) => info
                            .split_whitespace()
                            .next()
                            .filter(|l| !l.is_empty())
                            .map(str::to_owned),
                        CodeBlockKind::Indented => None,
                    };
                    if lang
                        .as_deref()
                        .is_some_and(|l| l.eq_ignore_ascii_case("typst"))
                    {
                        out.block_start("");
                        stack.push(Open::RawTypst);
                    } else {
                        out.capture();
                        stack.push(Open::Capture {
                            kind: Capture::Code(lang),
                            start: range.start,
                        });
                    }
                }
                Tag::List(start) => {
                    match start {
                        None => out.block_start("#list("),
                        Some(n) => out.block_start(&format!("#enum(start: {n}, ")),
                    }
                    stack.push(Open::Close(")"));
                }
                Tag::Item => {
                    out.raw("[");
                    stack.push(Open::Item);
                }
                Tag::Table(aligns) => {
                    let aligns: Vec<&str> = aligns
                        .iter()
                        .map(|a| match a {
                            pulldown_cmark::Alignment::Center => "center",
                            pulldown_cmark::Alignment::Right => "right",
                            _ => "left",
                        })
                        .collect();
                    out.block_start(&format!(
                        "#table(columns: {}, align: ({},), ",
                        aligns.len(),
                        aligns.join(", ")
                    ));
                    stack.push(Open::Close(")"));
                }
                Tag::TableHead => {
                    out.raw("table.header(");
                    stack.push(Open::Close("), "));
                }
                Tag::TableRow => stack.push(Open::Close("")),
                Tag::TableCell => {
                    out.raw("[");
                    stack.push(Open::Close("], "));
                }
                Tag::Emphasis => open(out, &mut stack, "#emph["),
                Tag::Strong => open(out, &mut stack, "#strong["),
                Tag::Strikethrough => open(out, &mut stack, "#strike["),
                Tag::Superscript => open(out, &mut stack, "#super["),
                Tag::Subscript => open(out, &mut stack, "#sub["),
                Tag::Link { dest_url, .. } => {
                    open(out, &mut stack, &format!("#link({})[", string(&dest_url)))
                }
                Tag::Image { dest_url, .. } => {
                    let path = match picture(body.dir, &dest_url) {
                        Ok(path) => path,
                        Err(message) => {
                            diagnostics.push(Diagnostic::at_with_code(
                                body.path,
                                body.text,
                                range.start,
                                true,
                                Some(xcpc_lint::code::MISSING_IMAGE),
                                message,
                            ));
                            String::new()
                        }
                    };
                    out.capture();
                    stack.push(Open::Capture {
                        kind: Capture::Image(path),
                        start: range.start,
                    });
                }
                Tag::HtmlBlock => {}
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Paragraph => out.par_end(),
                TagEnd::HtmlBlock => {}
                TagEnd::Heading(_) | TagEnd::CodeBlock | TagEnd::Image => {
                    if matches!(tag, TagEnd::CodeBlock)
                        && matches!(stack.last(), Some(Open::RawTypst))
                    {
                        stack.pop();
                        out.par_end();
                        continue;
                    }
                    let Some(Open::Capture { kind, start }) = stack.pop() else {
                        continue;
                    };
                    let (markup, plain) = out.release();
                    out.mark(body.file, start);
                    match kind {
                        Capture::Heading(level) if level <= HeadingLevel::H2 => {
                            let meaning = if body.editorial {
                                None
                            } else {
                                recognize_section(&plain)
                            };
                            let io = matches!(
                                meaning,
                                Some(SectionKind::Known("input" | "output" | "interaction"))
                            );
                            if in_io && !io && meaning != Some(SectionKind::Samples) {
                                place_samples(out, &mut samples);
                            }
                            in_io |= io;
                            match meaning {
                                Some(SectionKind::Known(key)) => {
                                    out.block(&format!("#section({})", string(key)))
                                }
                                Some(SectionKind::Samples) => {
                                    if samples.is_some() {
                                        place_samples(out, &mut samples);
                                    } else {
                                        warn(
                                            diagnostics,
                                            start,
                                            "a samples heading, but samples/ has none".into(),
                                        );
                                    }
                                }
                                Some(SectionKind::Legend) => {}
                                None if body.editorial => {
                                    out.block(&format!("#subsection[{}]", markup.trim()))
                                }
                                None => out
                                    .block(&format!("#section(none, title: [{}])", markup.trim())),
                            }
                        }
                        // A run-in head starts the paragraph after it.
                        Capture::Heading(_) if body.editorial => {
                            out.block_start(&format!("#runin[{}];", markup.trim()))
                        }
                        Capture::Heading(_) => {
                            out.block(&format!("#block(sticky: true, strong[{}])", markup.trim()))
                        }
                        Capture::Code(lang) => {
                            let lang = lang
                                .map(|l| format!("lang: {}, ", string(&l)))
                                .unwrap_or_default();
                            let code = plain.strip_suffix('\n').unwrap_or(&plain);
                            out.block(&format!("#raw(block: true, {lang}{})", string(code)));
                        }
                        Capture::Image(path) => {
                            let width_attr = iter.peek().and_then(|(next_ev, next_range)| {
                                if next_range.start != range.end {
                                    return None;
                                }
                                match next_ev {
                                    Event::Text(next_text) => parse_image_width_attr(next_text),
                                    _ => None,
                                }
                            });
                            let trailing = if let Some((_, consumed)) = &width_attr {
                                let consumed = *consumed;
                                if let Some((Event::Text(next_text), next_range)) = iter.next() {
                                    let rest = next_text[consumed..].to_owned();
                                    (!rest.is_empty())
                                        .then_some((rest, next_range.start + consumed))
                                } else {
                                    None
                                }
                            } else {
                                None
                            };
                            if !path.is_empty() {
                                let alt = if plain.is_empty() {
                                    String::new()
                                } else {
                                    format!(", alt: {}", string(&plain))
                                };
                                if let Some((width_pct, _)) = width_attr {
                                    out.block(&format!(
                                        "#align(center, image({}{alt}, width: {width_pct}%))",
                                        string(&path)
                                    ));
                                } else {
                                    out.inline(&format!("#box(image({}{alt}));", string(&path)));
                                }
                            }
                            if let Some((rest, rest_offset)) = trailing {
                                out.mark(body.file, rest_offset);
                                out.text(&rest);
                            }
                        }
                    }
                }
                TagEnd::Item => {
                    stack.pop();
                    out.trim_end();
                    out.raw("], ");
                }
                _ => {
                    if let Some(Open::Close(close)) = stack.pop() {
                        out.trim_end();
                        out.raw(close);
                        if matches!(tag, TagEnd::List(_) | TagEnd::Table | TagEnd::BlockQuote(_)) {
                            out.par_end();
                        } else if !close.is_empty() && close.ends_with(']') {
                            // `#strong[…];`: end the call, so a following `.` or `[`
                            // is text.
                            out.raw(";");
                        }
                    }
                }
            },
            Event::Text(text) => {
                if matches!(stack.last(), Some(Open::RawTypst)) {
                    let mut offset = 0usize;
                    for chunk in text.split_inclusive('\n') {
                        out.mark(body.file, range.start + offset);
                        out.raw(chunk);
                        offset += chunk.len();
                    }
                } else {
                    out.text(&text);
                }
            }
            Event::Code(code) => out.inline(&format!("#raw({});", string(&code))),
            Event::InlineMath(ref tex) | Event::DisplayMath(ref tex) => {
                let display = matches!(event, Event::DisplayMath(_));
                match mitex::convert_math(tex, None).map(|c| modernize(&c)) {
                    Ok(code) => {
                        let f = if display { "dm" } else { "m" };
                        out.inline(&format!("#{f}({});", string(code.trim())));
                    }
                    Err(message) => {
                        diagnostics.push(Diagnostic::at(
                            body.path,
                            body.text,
                            range.start,
                            true,
                            format!("cannot convert the formula: {message}"),
                        ));
                        out.inline(&format!("#raw({});", string(tex)));
                    }
                }
            }
            Event::Html(html) | Event::InlineHtml(html) => {
                let tag = html.trim().to_lowercase();
                if tag.starts_with("<br") {
                    out.inline("#linebreak();");
                } else if !tag.starts_with("<!--") {
                    out.text(&html);
                }
            }
            Event::SoftBreak => out.soft_break(),
            Event::HardBreak => out.inline("#linebreak();"),
            Event::Rule => out.block("#line(length: 100%)"),
            Event::TaskListMarker(done) => out.text(if done { "☒ " } else { "☐ " }),
            Event::FootnoteReference(_) => {}
        }
    }
    if let Some(code) = samples.take() {
        out.block(&code);
    }
}

fn open(out: &mut Out, stack: &mut Vec<Open>, start: &str) {
    out.inline(start);
    stack.push(Open::Close("]"));
}

/// Symbols MiTeX 0.2.4 names as Typst did before 0.14, with their names in the Typst
/// we use: `\oplus` comes out as `plus.circle`, which Typst 0.15 knows as `plus.o`.
/// Found by evaluating every symbol of MiTeX's scope (see the test
/// `every_mitex_symbol_evaluates`).
const RENAMED: &[(&str, &str)] = &[
    ("sect", "inter"),
    ("dot.circle", "dot.o"),
    ("plus.circle", "plus.o"),
    ("times.circle", "times.o"),
    ("minus.circle", "minus.o"),
    ("ast.circle", "convolve.o"),
    ("circle.nested", "compose.o"),
    ("dash.circle", "dash.o"),
    ("arrow.l.dash", "arrow.l.dashed"),
    ("arrow.r.dash", "arrow.r.dashed"),
    ("planck.reduce", "planck"),
    ("angle.l", "chevron.l"),
    ("angle.r", "chevron.r"),
    ("bracket.l.double", "bracket.l.stroked"),
    ("bracket.r.double", "bracket.r.stroked"),
    ("ohm.inv", "Omega.inv"),
    ("diff", "partial"),
];

/// MiTeX's math code with [`RENAMED`] symbols under their current names. Only names in
/// math count: not escaped characters (`\(`), nor the text of `#textmath[…]`.
pub(crate) fn modernize(code: &str) -> String {
    let mut out = String::with_capacity(code.len());
    let mut depth = 0usize; // inside `[…]` content: text, left alone
    let mut chars = code.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' => {
                out.push(c);
                if let Some((_, next)) = chars.next() {
                    out.push(next);
                }
            }
            '[' => {
                depth += 1;
                out.push(c);
            }
            ']' => {
                depth = depth.saturating_sub(1);
                out.push(c);
            }
            c if c.is_ascii_alphabetic() && depth == 0 => {
                // A name and its modifiers: `plus.circle.big`.
                let mut end = i + c.len_utf8();
                while let Some(&(j, d)) = chars.peek() {
                    let dot_then_letter =
                        d == '.' && code[j + 1..].starts_with(|n: char| n.is_ascii_alphabetic());
                    if d.is_ascii_alphanumeric() || dot_then_letter {
                        end = j + d.len_utf8();
                        chars.next();
                    } else {
                        break;
                    }
                }
                let name = &code[i..end];
                let after_hash = out.ends_with('#');
                let renamed = RENAMED.iter().find_map(|(old, new)| {
                    let rest = name.strip_prefix(old)?;
                    (rest.is_empty() || rest.starts_with('.')).then(|| format!("{new}{rest}"))
                });
                match renamed {
                    Some(new) if !after_hash => out.push_str(&new),
                    _ => out.push_str(name),
                }
            }
            c => out.push(c),
        }
    }
    out
}

/// Parses a trailing `{width=60%}` attribute immediately after an image, accepting
/// percentages in `10%–100%`. Returns `(percent_number_str, consumed_byte_len)`.
pub(crate) fn parse_image_width_attr(text: &str) -> Option<(String, usize)> {
    if !text.starts_with('{') {
        return None;
    }
    let close = text.find('}')?;
    let inside = &text[1..close];
    if inside.contains('\n') || inside.contains('\r') {
        return None;
    }
    let trimmed = inside.trim();
    let (key, val) = trimmed.split_once('=')?;
    if !key.trim().eq_ignore_ascii_case("width") {
        return None;
    }
    let num_str = val.trim().strip_suffix('%')?.trim();
    if num_str.is_empty() {
        return None;
    }
    let num: f64 = num_str.parse().ok()?;
    if !num.is_finite() || !(10.0..=100.0).contains(&num) {
        return None;
    }
    Some((num_str.to_owned(), close + 1))
}

/// A picture's path for Typst: relative to the problem's directory, and in it.
fn picture(dir: &str, url: &str) -> Result<String, String> {
    xcpc_lint::resolve_image_path(dir, url).map(|rel| format!("/{rel}"))
}

/// Converts a LaTeX math expression into modernized Typst math code via `mitex`.
pub fn math_to_typst(tex: &str) -> Result<String, String> {
    mitex::convert_math(tex, None).map(|c| modernize(&c).trim().to_owned())
}

/// Formats `samples` as a standalone Markdown section and inserts it into `statement`
/// without mutating the source file: replaces an explicit `## 样例` / `## Examples`
/// placeholder heading if present, otherwise inserts before `## 提示` / `## 样例解释` /
/// `## Note` / `## Explanation`, or appends at the end.
pub fn markdown_with_samples(
    statement: &str,
    samples: &[xcpc_domain::Sample],
    language: xcpc_domain::Language,
) -> String {
    if samples.is_empty() {
        return statement.to_owned();
    }
    let block = format_samples_markdown(samples, language);

    // Locate H2 headings via `pulldown_cmark` so fenced code blocks are never misparsed.
    let mut in_h2 = false;
    let mut h2_start = 0usize;
    let mut h2_text = String::new();
    let mut samples_heading: Option<std::ops::Range<usize>> = None;
    let mut before_note: Option<usize> = None;

    for (event, range) in Parser::new_ext(statement, options()).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading {
                level: HeadingLevel::H2,
                ..
            }) => {
                in_h2 = true;
                h2_start = range.start;
                h2_text.clear();
            }
            Event::Text(t) | Event::Code(t) if in_h2 => {
                h2_text.push_str(&t);
            }
            Event::End(TagEnd::Heading(HeadingLevel::H2)) if in_h2 => {
                in_h2 = false;
                match recognize_section(&h2_text) {
                    Some(SectionKind::Samples) if samples_heading.is_none() => {
                        samples_heading = Some(h2_start..range.end);
                    }
                    Some(SectionKind::Known("note" | "explanation")) if before_note.is_none() => {
                        before_note = Some(h2_start);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    if let Some(span) = samples_heading {
        let prefix = statement[..span.start].trim_end();
        let suffix = statement[span.end..].trim_start_matches(['\r', '\n']);
        let mut out = String::with_capacity(statement.len() + block.len() + 4);
        if !prefix.is_empty() {
            out.push_str(prefix);
            out.push_str("\n\n");
        }
        out.push_str(&block);
        if !suffix.is_empty() {
            out.push_str("\n\n");
            out.push_str(suffix);
        } else {
            out.push('\n');
        }
        return out;
    }

    if let Some(at) = before_note {
        let prefix = statement[..at].trim_end();
        let suffix = statement[at..].trim_start_matches(['\r', '\n']);
        let mut out = String::with_capacity(statement.len() + block.len() + 4);
        if !prefix.is_empty() {
            out.push_str(prefix);
            out.push_str("\n\n");
        }
        out.push_str(&block);
        out.push_str("\n\n");
        out.push_str(suffix);
        return out;
    }

    let trimmed = statement.trim_end();
    if trimmed.is_empty() {
        return format!("{block}\n");
    }
    format!("{trimmed}\n\n{block}\n")
}

fn format_samples_markdown(
    samples: &[xcpc_domain::Sample],
    language: xcpc_domain::Language,
) -> String {
    let zh = language == xcpc_domain::Language::Chinese;
    let mut out = String::new();
    out.push_str(if zh { "## 样例" } else { "## Examples" });
    for (idx, sample) in samples.iter().enumerate() {
        let n = idx + 1;
        out.push_str("\n\n");
        if zh {
            out.push_str(&format!("### 样例 {n} 输入\n\n"));
        } else {
            out.push_str(&format!("### Example {n} Input\n\n"));
        }
        push_fenced_block(&mut out, &sample.input);
        out.push_str("\n\n");
        if zh {
            out.push_str(&format!("### 样例 {n} 输出\n\n"));
        } else {
            out.push_str(&format!("### Example {n} Output\n\n"));
        }
        push_fenced_block(&mut out, &sample.output);
    }
    out
}

fn push_fenced_block(out: &mut String, content: &str) {
    let mut max_run = 0usize;
    let mut run = 0usize;
    for b in content.bytes() {
        if b == b'`' {
            run += 1;
            max_run = max_run.max(run);
        } else {
            run = 0;
        }
    }
    let fence_len = max_run.max(2) + 1;
    let fence: String = "`".repeat(fence_len);
    out.push_str(&fence);
    out.push_str("text\n");
    let body = content.trim_end_matches('\n');
    out.push_str(body);
    if !body.is_empty() {
        out.push('\n');
    }
    out.push_str(&fence);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pictures_stay_in_the_problem() {
        assert_eq!(
            picture("problems/a", "assets/x.png").unwrap(),
            "/problems/a/assets/x.png"
        );
        assert_eq!(
            picture("problems/a", "./assets/../y.png").unwrap(),
            "/problems/a/y.png"
        );
        assert!(picture("problems/a", "../b/x.png").is_err());
        assert!(picture("problems/a", "/etc/x.png").is_err());
        assert!(picture("problems/a", "https://x/y.png").is_err());
    }

    #[test]
    fn old_symbol_names_are_renamed() {
        assert_eq!(modernize("k  plus.circle  x "), "k  plus.o  x ");
        assert_eq!(
            modernize("plus.circle.big _(i ) sect.double diff  x"),
            "plus.o.big _(i ) inter.double partial  x"
        );
        // A name that only starts like one, text, escapes and functions stay.
        assert_eq!(
            modernize("angle  diffuse #textmath[sect diff]; \\diff"),
            "angle  diffuse #textmath[sect diff]; \\diff"
        );
        assert_eq!(
            modernize("lr(angle.l  x  angle.r )"),
            "lr(chevron.l  x  chevron.r )"
        );
    }

    #[test]
    fn headings() {
        assert_eq!(
            recognize_section("输入格式"),
            Some(SectionKind::Known("input"))
        );
        assert_eq!(
            recognize_section(" Output: "),
            Some(SectionKind::Known("output"))
        );
        assert_eq!(recognize_section("样例"), Some(SectionKind::Samples));
        assert_eq!(recognize_section("其他"), None);
    }

    #[test]
    fn markdown_with_samples_inserts_before_notes() {
        let stmt = "给定 $a, b$。\n\n## 输入格式\n\n一行两个数。\n\n## 提示\n\n注意范围。\n";
        let samples = vec![xcpc_domain::Sample {
            name: "01".into(),
            input: "1 2\n".into(),
            output: "3\n".into(),
        }];
        let out = markdown_with_samples(stmt, &samples, xcpc_domain::Language::Chinese);
        let idx_in = out.find("## 输入格式").unwrap();
        let idx_samples = out.find("## 样例").unwrap();
        let idx_note = out.find("## 提示").unwrap();
        assert!(idx_in < idx_samples && idx_samples < idx_note);
        assert!(out.contains("### 样例 1 输入\n\n```text\n1 2\n```"));
        assert!(out.contains("### 样例 1 输出\n\n```text\n3\n```"));
    }

    #[test]
    fn markdown_with_samples_replaces_explicit_placeholder_and_uses_english() {
        let stmt = "## Input\n\nOne line.\n\n## Examples\n\n## Note\n\nHint.\n";
        let samples = vec![xcpc_domain::Sample {
            name: "01".into(),
            input: "```raw```\n".into(),
            output: "ok\n".into(),
        }];
        let out = markdown_with_samples(stmt, &samples, xcpc_domain::Language::English);
        assert_eq!(out.matches("## Examples").count(), 1);
        assert!(out.contains("````text\n```raw```\n````"));
        assert!(out.contains("### Example 1 Output"));
    }
}
