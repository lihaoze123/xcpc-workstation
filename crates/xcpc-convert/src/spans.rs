//! Where the Markdown's structure lies in its source, for an editor that styles the
//! source as it is typed (headings, emphasis, math, code), read off `pulldown-cmark`'s
//! offset events with the same options the conversion uses: what the editor marks as
//! math is what typesets as math.

use std::ops::Range;

use pulldown_cmark::{CodeBlockKind, Event, Parser, Tag};

use crate::markdown::options;

/// What a span of the source is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanKind {
    /// A heading's text; its level is in [`Span::level`].
    Heading,
    Strong,
    Emphasis,
    Strikethrough,
    /// Inline math's content, between its dollars.
    InlineMath,
    /// Display math's content, between its double dollars.
    DisplayMath,
    /// A LaTeX control sequence inside math (`\le`, `\times`, `\frac`, `\\`).
    MathCommand,
    /// A LaTeX structural symbol inside math (`^`, `_`, `{`, `}`, `&`).
    MathOperator,
    /// Inline code's content, between its backticks.
    Code,
    /// A whole code block, fences included.
    CodeBlock,
    /// A comment inside a fenced code block.
    CodeComment,
    /// A string or character literal inside a fenced code block.
    CodeString,
    /// A language keyword or C/C++ preprocessor directive inside a fenced code block.
    CodeKeyword,
    /// A numeric literal inside a fenced code block.
    CodeNumber,
    /// A link's or picture's text.
    Link,
    /// A link's or picture's destination, with its parentheses.
    Url,
    /// A whole table.
    Table,
    /// Characters that only mark structure up: `##`, `**`, `$`, backticks, fences,
    /// brackets, list and quote markers.
    Markup,
}

/// One span, as byte offsets into the text. Spans of different kinds overlap (a strong
/// span covers its markup); later spans are meant to be drawn over earlier ones.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub kind: SpanKind,
    pub range: Range<usize>,
    /// A heading's level (1–6); 0 for every other kind.
    pub level: u8,
}

struct Open {
    tag: Tag<'static>,
    range: Range<usize>,
    /// Where the element's children start and end, once it has any.
    inner: Option<Range<usize>>,
}

/// The spans of `text`, in source order of their starts.
pub fn markdown_spans(text: &str) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut stack: Vec<Open> = Vec::new();

    for (event, range) in Parser::new_ext(text, options()).into_offset_iter() {
        // A child (nested element or text) widens its parent's inner range.
        if !matches!(event, Event::End(_))
            && let Some(parent) = stack.last_mut()
        {
            let inner = parent.inner.get_or_insert(range.clone());
            inner.start = inner.start.min(range.start);
            inner.end = inner.end.max(range.end);
        }
        match event {
            Event::Start(tag) => {
                stack.push(Open {
                    tag: tag.into_static(),
                    range,
                    inner: None,
                });
            }
            Event::End(end) => {
                let Some(open) = stack.pop() else { continue };
                let whole = open.range.clone();
                let inner = open.inner.clone().unwrap_or(whole.end..whole.end);
                let marks = |spans: &mut Vec<Span>| {
                    push(
                        spans,
                        SpanKind::Markup,
                        whole.start..inner.start.max(whole.start),
                        0,
                    );
                    push(
                        spans,
                        SpanKind::Markup,
                        inner.end.min(whole.end)..trim_end(text, &whole),
                        0,
                    );
                };
                match (&open.tag, end) {
                    (Tag::Heading { level, .. }, _) => {
                        let level = *level as u8;
                        let end = trim_end(text, &whole);
                        if let Some(inner) = open.inner.clone() {
                            let body_start = inner.start.clamp(whole.start, end);
                            let body_end = inner.end.clamp(body_start, end);
                            push(&mut spans, SpanKind::Markup, whole.start..body_start, 0);
                            push(&mut spans, SpanKind::Heading, body_start..body_end, level);
                            push(&mut spans, SpanKind::Markup, body_end..end, 0);
                        } else {
                            push(&mut spans, SpanKind::Markup, whole.start..end, 0);
                        }
                    }
                    (Tag::Strong, _) => {
                        push(&mut spans, SpanKind::Strong, whole.clone(), 0);
                        marks(&mut spans);
                    }
                    (Tag::Emphasis, _) => {
                        push(&mut spans, SpanKind::Emphasis, whole.clone(), 0);
                        marks(&mut spans);
                    }
                    (Tag::Strikethrough, _) => {
                        push(&mut spans, SpanKind::Strikethrough, whole.clone(), 0);
                        marks(&mut spans);
                    }
                    (Tag::Link { .. } | Tag::Image { .. }, _) => {
                        // `[text](url)` / `![text](url)`: the text, then the destination.
                        let close = text[inner.end..whole.end].find(']').map(|i| inner.end + i);
                        push(
                            &mut spans,
                            SpanKind::Markup,
                            whole.start..inner.start.max(whole.start),
                            0,
                        );
                        push(&mut spans, SpanKind::Link, inner.clone(), 0);
                        if let Some(close) = close {
                            push(&mut spans, SpanKind::Markup, close..close + 1, 0);
                            push(&mut spans, SpanKind::Url, close + 1..whole.end, 0);
                        }
                    }
                    (Tag::CodeBlock(kind), _) => {
                        let end = trim_end(text, &whole);
                        push(&mut spans, SpanKind::CodeBlock, whole.start..end, 0);
                        if let CodeBlockKind::Fenced(info) = kind {
                            let first = text[whole.start..end]
                                .find('\n')
                                .map_or(end, |i| whole.start + i);
                            push(&mut spans, SpanKind::Markup, whole.start..first, 0);
                            let last = text[whole.start..end]
                                .rfind('\n')
                                .map_or(end, |i| whole.start + i + 1);
                            let body_end = if last > first && is_fence(&text[last..end]) {
                                push(&mut spans, SpanKind::Markup, last..end, 0);
                                last
                            } else {
                                end
                            };
                            if first < body_end {
                                let body_start = (first + 1).min(body_end);
                                if let Some(lang) = CodeLang::parse(info.as_ref()) {
                                    push_code_spans(&mut spans, text, body_start..body_end, lang);
                                }
                            }
                        }
                    }
                    (Tag::Table(_), _) => push(
                        &mut spans,
                        SpanKind::Table,
                        whole.start..trim_end(text, &whole),
                        0,
                    ),
                    (Tag::Item | Tag::BlockQuote(_), _) => {
                        // The marker: `-`, `1.`, `>` up to the first child.
                        let marker = whole.start..inner.start.min(whole.end);
                        if text[marker.clone()].trim().len() <= 4 {
                            push(&mut spans, SpanKind::Markup, marker, 0);
                        }
                    }
                    _ => {}
                }
            }
            Event::InlineMath(_) | Event::DisplayMath(_) => {
                let display = matches!(event, Event::DisplayMath(_));
                let source = &text[range.clone()];
                let open = source.len() - source.trim_start_matches('$').len();
                let close = source.len() - source.trim_end_matches('$').len();
                let content = range.start + open..range.end - close;
                push(
                    &mut spans,
                    if display {
                        SpanKind::DisplayMath
                    } else {
                        SpanKind::InlineMath
                    },
                    content.clone(),
                    0,
                );
                push(
                    &mut spans,
                    SpanKind::Markup,
                    range.start..range.start + open,
                    0,
                );
                push(
                    &mut spans,
                    SpanKind::Markup,
                    range.end - close..range.end,
                    0,
                );
                push_math_spans(&mut spans, text, content);
            }
            Event::Code(_) => {
                let source = &text[range.clone()];
                let open = source.len() - source.trim_start_matches('`').len();
                let close = source.len() - source.trim_end_matches('`').len();
                push(
                    &mut spans,
                    SpanKind::Code,
                    range.start + open..range.end - close,
                    0,
                );
                push(
                    &mut spans,
                    SpanKind::Markup,
                    range.start..range.start + open,
                    0,
                );
                push(
                    &mut spans,
                    SpanKind::Markup,
                    range.end - close..range.end,
                    0,
                );
            }
            Event::Html(_) | Event::InlineHtml(_) => push(&mut spans, SpanKind::Markup, range, 0),
            _ => {}
        }
    }
    spans.sort_by_key(|s| s.range.start);
    spans
}

fn push(spans: &mut Vec<Span>, kind: SpanKind, range: Range<usize>, level: u8) {
    if range.start < range.end {
        spans.push(Span { kind, range, level });
    }
}

/// The end of `range` without its trailing line break.
fn trim_end(text: &str, range: &Range<usize>) -> usize {
    range.start + text[range.clone()].trim_end_matches(['\n', '\r']).len()
}

fn is_fence(line: &str) -> bool {
    let t = line.trim();
    t.len() >= 3 && (t.chars().all(|c| c == '`') || t.chars().all(|c| c == '~'))
}

fn push_math_spans(spans: &mut Vec<Span>, text: &str, range: Range<usize>) {
    if range.start >= range.end {
        return;
    }
    let slice = &text[range.clone()];
    let bytes = slice.as_bytes();
    let base = range.start;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\\' {
            let start = i;
            i += 1;
            if i < bytes.len() {
                if bytes[i].is_ascii_alphabetic() {
                    while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                        i += 1;
                    }
                } else {
                    let ch = slice[i..].chars().next().unwrap();
                    i += ch.len_utf8();
                }
            }
            push(spans, SpanKind::MathCommand, base + start..base + i, 0);
        } else if matches!(b, b'^' | b'_' | b'{' | b'}' | b'&') {
            push(spans, SpanKind::MathOperator, base + i..base + i + 1, 0);
            i += 1;
        } else if b < 0x80 {
            i += 1;
        } else {
            let ch = slice[i..].chars().next().unwrap();
            i += ch.len_utf8();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CodeLang {
    Cpp,
    Python,
    Rust,
    CLike,
}

impl CodeLang {
    fn parse(info: &str) -> Option<Self> {
        let token = info.split_whitespace().next()?.to_ascii_lowercase();
        match token.as_str() {
            "c" | "h" | "cpp" | "c++" | "cc" | "cxx" | "hpp" | "hh" | "hxx" => Some(Self::Cpp),
            "py" | "python" | "python3" | "py3" => Some(Self::Python),
            "rs" | "rust" => Some(Self::Rust),
            "java" | "js" | "javascript" | "ts" | "typescript" | "go" | "golang" | "cs"
            | "csharp" | "c#" | "kt" | "kotlin" | "swift" | "php" | "d" | "zig" | "scala"
            | "dart" => Some(Self::CLike),
            _ => None,
        }
    }
}

fn push_code_spans(spans: &mut Vec<Span>, text: &str, range: Range<usize>, lang: CodeLang) {
    if range.start >= range.end {
        return;
    }
    let slice = &text[range.clone()];
    let bytes = slice.as_bytes();
    let base = range.start;
    let mut i = 0;
    let mut at_line_start = true;

    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\n' {
            at_line_start = true;
            i += 1;
            continue;
        }
        if matches!(b, b' ' | b'\t' | b'\r') {
            i += 1;
            continue;
        }

        let line_start = at_line_start;
        at_line_start = false;

        // C/C++ preprocessor directives at line start (e.g. `#include`, `#define`).
        if lang == CodeLang::Cpp && line_start && b == b'#' {
            let start = i;
            let mut j = i + 1;
            while j < bytes.len() && matches!(bytes[j], b' ' | b'\t') {
                j += 1;
            }
            let ident_start = j;
            while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
                j += 1;
            }
            if j > ident_start {
                push(spans, SpanKind::CodeKeyword, base + start..base + j, 0);
                i = j;
                continue;
            }
        }

        // Line comments.
        if lang == CodeLang::Python && b == b'#' {
            let start = i;
            while i < bytes.len() && !matches!(bytes[i], b'\n' | b'\r') {
                i += 1;
            }
            push(spans, SpanKind::CodeComment, base + start..base + i, 0);
            continue;
        }
        if lang != CodeLang::Python && b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            let start = i;
            i += 2;
            while i < bytes.len() && !matches!(bytes[i], b'\n' | b'\r') {
                i += 1;
            }
            push(spans, SpanKind::CodeComment, base + start..base + i, 0);
            continue;
        }

        // Block comments (`/* ... */`, nested for Rust).
        if lang != CodeLang::Python && b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            let start = i;
            i += 2;
            let mut depth = 1usize;
            while i < bytes.len() && depth > 0 {
                if lang == CodeLang::Rust
                    && bytes[i] == b'/'
                    && i + 1 < bytes.len()
                    && bytes[i + 1] == b'*'
                {
                    depth += 1;
                    i += 2;
                } else if bytes[i] == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                    depth -= 1;
                    i += 2;
                } else if bytes[i] < 0x80 {
                    i += 1;
                } else {
                    i += slice[i..].chars().next().unwrap().len_utf8();
                }
            }
            push(spans, SpanKind::CodeComment, base + start..base + i, 0);
            continue;
        }

        // Language-specific strings & char literals.
        if let Some(end) = scan_string(slice, bytes, i, lang) {
            push(spans, SpanKind::CodeString, base + i..base + end, 0);
            i = end;
            continue;
        }

        // Numbers.
        if b.is_ascii_digit() {
            let start = i;
            i = scan_number(bytes, i, lang);
            push(spans, SpanKind::CodeNumber, base + start..base + i, 0);
            continue;
        }

        // Identifiers & keywords.
        if b.is_ascii_alphabetic() || b == b'_' {
            let start = i;
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            if is_keyword(lang, &slice[start..i]) {
                push(spans, SpanKind::CodeKeyword, base + start..base + i, 0);
            }
            continue;
        }

        if b < 0x80 {
            i += 1;
        } else {
            i += slice[i..].chars().next().unwrap().len_utf8();
        }
    }
}

fn scan_string(slice: &str, bytes: &[u8], start: usize, lang: CodeLang) -> Option<usize> {
    let b = bytes[start];
    match lang {
        CodeLang::Python => {
            let mut q = start;
            if matches!(b, b'r' | b'R' | b'b' | b'B' | b'f' | b'F' | b'u' | b'U') {
                q += 1;
                if q < bytes.len()
                    && matches!(
                        (b.to_ascii_lowercase(), bytes[q].to_ascii_lowercase()),
                        (b'r', b'b' | b'f') | (b'b' | b'f', b'r')
                    )
                {
                    q += 1;
                }
            }
            if q >= bytes.len() || !matches!(bytes[q], b'"' | b'\'') {
                return None;
            }
            let quote = bytes[q];
            if q + 2 < bytes.len() && bytes[q + 1] == quote && bytes[q + 2] == quote {
                return Some(scan_triple_quoted(slice, bytes, q + 3, quote));
            }
            Some(scan_quoted(slice, bytes, q + 1, quote, false))
        }
        CodeLang::Rust => {
            // Raw strings: `r"..."`, `r#"..."#`, `br#"..."#`.
            let mut q = start;
            if b == b'b' && q + 1 < bytes.len() {
                q += 1;
            }
            if q < bytes.len() && bytes[q] == b'r' {
                let mut hashes = 0usize;
                let mut k = q + 1;
                while k < bytes.len() && bytes[k] == b'#' {
                    hashes += 1;
                    k += 1;
                }
                if k < bytes.len() && bytes[k] == b'"' {
                    let mut j = k + 1;
                    while j < bytes.len() {
                        if bytes[j] == b'"'
                            && j + hashes < bytes.len()
                            && bytes[j + 1..=j + hashes].iter().all(|&c| c == b'#')
                        {
                            return Some(j + 1 + hashes);
                        }
                        if bytes[j] < 0x80 {
                            j += 1;
                        } else {
                            j += slice[j..].chars().next().unwrap().len_utf8();
                        }
                    }
                    return Some(bytes.len());
                }
            }
            // Byte string `b"..."` or normal string `"..."`.
            if q < bytes.len() && bytes[q] == b'"' {
                return Some(scan_quoted(slice, bytes, q + 1, b'"', true));
            }
            // Char / byte-char literal `'a'`, `'\n'`, `b'x'` (not lifetimes like `'a`).
            if q < bytes.len() && bytes[q] == b'\'' {
                return scan_rust_char(slice, bytes, q);
            }
            None
        }
        CodeLang::Cpp => {
            let mut q = start;
            if b == b'u' && q + 1 < bytes.len() && bytes[q + 1] == b'8' {
                q += 2;
            } else if matches!(b, b'u' | b'U' | b'L') {
                q += 1;
            }
            if q < bytes.len() && matches!(bytes[q], b'"' | b'\'') {
                return Some(scan_quoted(slice, bytes, q + 1, bytes[q], false));
            }
            None
        }
        CodeLang::CLike => match b {
            b'"' | b'\'' => Some(scan_quoted(slice, bytes, start + 1, b, false)),
            b'`' => Some(scan_quoted(slice, bytes, start + 1, b'`', true)),
            _ => None,
        },
    }
}

fn scan_quoted(slice: &str, bytes: &[u8], mut i: usize, quote: u8, multiline: bool) -> usize {
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\\' && i + 1 < bytes.len() {
            i += 1;
            if bytes[i] < 0x80 {
                i += 1;
            } else {
                i += slice[i..].chars().next().unwrap().len_utf8();
            }
            continue;
        }
        if b == quote {
            return i + 1;
        }
        if !multiline && b == b'\n' {
            return i;
        }
        if b < 0x80 {
            i += 1;
        } else {
            i += slice[i..].chars().next().unwrap().len_utf8();
        }
    }
    bytes.len()
}

fn scan_triple_quoted(slice: &str, bytes: &[u8], mut i: usize, quote: u8) -> usize {
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\\' && i + 1 < bytes.len() {
            i += 1;
            if bytes[i] < 0x80 {
                i += 1;
            } else {
                i += slice[i..].chars().next().unwrap().len_utf8();
            }
            continue;
        }
        if b == quote && i + 2 < bytes.len() && bytes[i + 1] == quote && bytes[i + 2] == quote {
            return i + 3;
        }
        if b < 0x80 {
            i += 1;
        } else {
            i += slice[i..].chars().next().unwrap().len_utf8();
        }
    }
    bytes.len()
}

fn scan_rust_char(slice: &str, bytes: &[u8], quote_pos: usize) -> Option<usize> {
    let i = quote_pos + 1;
    if i >= bytes.len() || matches!(bytes[i], b'\'' | b'\n' | b'\r') {
        return None;
    }
    if bytes[i] == b'\\' {
        let mut j = i + 1;
        if j >= bytes.len() || matches!(bytes[j], b'\n' | b'\r') {
            return None;
        }
        if bytes[j] < 0x80 {
            j += 1;
        } else {
            j += slice[j..].chars().next().unwrap().len_utf8();
        }
        while j < bytes.len() && !matches!(bytes[j], b'\'' | b'\n' | b'\r') {
            if bytes[j] < 0x80 {
                j += 1;
            } else {
                j += slice[j..].chars().next().unwrap().len_utf8();
            }
            if j - quote_pos > 16 {
                return None;
            }
        }
        return (j < bytes.len() && bytes[j] == b'\'').then_some(j + 1);
    }
    let ch_len = if bytes[i] < 0x80 {
        1
    } else {
        slice[i..].chars().next().unwrap().len_utf8()
    };
    let after = i + ch_len;
    (after < bytes.len() && bytes[after] == b'\'').then_some(after + 1)
}

fn scan_number(bytes: &[u8], start: usize, lang: CodeLang) -> usize {
    let mut i = start;
    if bytes[i] == b'0' && i + 1 < bytes.len() {
        match bytes[i + 1] {
            b'x' | b'X' => {
                i += 2;
                while i < bytes.len()
                    && (bytes[i].is_ascii_hexdigit()
                        || bytes[i] == b'_'
                        || (lang == CodeLang::Cpp
                            && bytes[i] == b'\''
                            && i + 1 < bytes.len()
                            && bytes[i + 1].is_ascii_hexdigit()))
                {
                    i += 1;
                }
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                return i;
            }
            b'b' | b'B' | b'o' | b'O' => {
                i += 2;
                while i < bytes.len()
                    && (bytes[i].is_ascii_digit()
                        || bytes[i] == b'_'
                        || (lang == CodeLang::Cpp
                            && bytes[i] == b'\''
                            && i + 1 < bytes.len()
                            && bytes[i + 1].is_ascii_digit()))
                {
                    i += 1;
                }
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                return i;
            }
            _ => {}
        }
    }
    while i < bytes.len()
        && (bytes[i].is_ascii_digit()
            || bytes[i] == b'_'
            || (lang == CodeLang::Cpp
                && bytes[i] == b'\''
                && i + 1 < bytes.len()
                && bytes[i + 1].is_ascii_digit()))
    {
        i += 1;
    }
    if i + 1 < bytes.len() && bytes[i] == b'.' && bytes[i + 1].is_ascii_digit() {
        i += 1;
        while i < bytes.len()
            && (bytes[i].is_ascii_digit()
                || bytes[i] == b'_'
                || (lang == CodeLang::Cpp
                    && bytes[i] == b'\''
                    && i + 1 < bytes.len()
                    && bytes[i + 1].is_ascii_digit()))
        {
            i += 1;
        }
    }
    if i < bytes.len() && matches!(bytes[i], b'e' | b'E') {
        let mut j = i + 1;
        if j < bytes.len() && matches!(bytes[j], b'+' | b'-') {
            j += 1;
        }
        if j < bytes.len() && bytes[j].is_ascii_digit() {
            i = j;
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
                i += 1;
            }
        }
    }
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
        i += 1;
    }
    i
}

fn is_keyword(lang: CodeLang, ident: &str) -> bool {
    match lang {
        CodeLang::Cpp => matches!(
            ident,
            "alignas"
                | "alignof"
                | "and"
                | "auto"
                | "bool"
                | "break"
                | "case"
                | "catch"
                | "char"
                | "char8_t"
                | "char16_t"
                | "char32_t"
                | "class"
                | "co_await"
                | "co_return"
                | "co_yield"
                | "const"
                | "const_cast"
                | "consteval"
                | "constexpr"
                | "constinit"
                | "continue"
                | "decltype"
                | "default"
                | "delete"
                | "do"
                | "double"
                | "dynamic_cast"
                | "else"
                | "enum"
                | "explicit"
                | "export"
                | "extern"
                | "false"
                | "float"
                | "for"
                | "friend"
                | "goto"
                | "if"
                | "inline"
                | "int"
                | "long"
                | "mutable"
                | "namespace"
                | "new"
                | "noexcept"
                | "not"
                | "NULL"
                | "nullptr"
                | "operator"
                | "or"
                | "private"
                | "protected"
                | "public"
                | "register"
                | "reinterpret_cast"
                | "requires"
                | "return"
                | "short"
                | "signed"
                | "sizeof"
                | "static"
                | "static_assert"
                | "static_cast"
                | "struct"
                | "switch"
                | "template"
                | "this"
                | "thread_local"
                | "throw"
                | "true"
                | "try"
                | "typedef"
                | "typeid"
                | "typename"
                | "union"
                | "unsigned"
                | "using"
                | "virtual"
                | "void"
                | "volatile"
                | "wchar_t"
                | "while"
        ),
        CodeLang::Python => matches!(
            ident,
            "False"
                | "None"
                | "True"
                | "and"
                | "as"
                | "assert"
                | "async"
                | "await"
                | "break"
                | "case"
                | "class"
                | "continue"
                | "def"
                | "del"
                | "elif"
                | "else"
                | "except"
                | "finally"
                | "for"
                | "from"
                | "global"
                | "if"
                | "import"
                | "in"
                | "is"
                | "lambda"
                | "match"
                | "nonlocal"
                | "not"
                | "or"
                | "pass"
                | "raise"
                | "return"
                | "try"
                | "while"
                | "with"
                | "yield"
        ),
        CodeLang::Rust => matches!(
            ident,
            "Self"
                | "as"
                | "async"
                | "await"
                | "bool"
                | "break"
                | "char"
                | "const"
                | "continue"
                | "crate"
                | "dyn"
                | "else"
                | "enum"
                | "extern"
                | "f32"
                | "f64"
                | "false"
                | "fn"
                | "for"
                | "i8"
                | "i16"
                | "i32"
                | "i64"
                | "i128"
                | "if"
                | "impl"
                | "in"
                | "isize"
                | "let"
                | "loop"
                | "match"
                | "mod"
                | "move"
                | "mut"
                | "pub"
                | "ref"
                | "return"
                | "self"
                | "static"
                | "str"
                | "struct"
                | "super"
                | "trait"
                | "true"
                | "type"
                | "u8"
                | "u16"
                | "u32"
                | "u64"
                | "u128"
                | "unsafe"
                | "use"
                | "usize"
                | "where"
                | "while"
        ),
        CodeLang::CLike => matches!(
            ident,
            "abstract"
                | "as"
                | "async"
                | "await"
                | "bool"
                | "boolean"
                | "break"
                | "byte"
                | "case"
                | "catch"
                | "char"
                | "class"
                | "const"
                | "continue"
                | "default"
                | "defer"
                | "do"
                | "double"
                | "else"
                | "enum"
                | "export"
                | "extends"
                | "false"
                | "final"
                | "finally"
                | "float"
                | "for"
                | "func"
                | "function"
                | "go"
                | "goto"
                | "if"
                | "implements"
                | "import"
                | "in"
                | "instanceof"
                | "int"
                | "interface"
                | "let"
                | "long"
                | "map"
                | "new"
                | "nil"
                | "null"
                | "package"
                | "private"
                | "protected"
                | "public"
                | "range"
                | "return"
                | "select"
                | "short"
                | "static"
                | "string"
                | "struct"
                | "super"
                | "switch"
                | "this"
                | "throw"
                | "throws"
                | "true"
                | "try"
                | "type"
                | "typeof"
                | "undefined"
                | "var"
                | "void"
                | "while"
                | "yield"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn of<'a>(text: &'a str, spans: &[Span], kind: SpanKind) -> Vec<&'a str> {
        spans
            .iter()
            .filter(|s| s.kind == kind)
            .map(|s| &text[s.range.clone()])
            .collect()
    }

    #[test]
    fn headings_emphasis_and_their_markup() {
        let text = "## 题目描述\n\n求 **最大** 子段和，_注意_ 边界。\n";
        let spans = markdown_spans(text);
        assert_eq!(of(text, &spans, SpanKind::Heading), ["题目描述"]);
        assert_eq!(
            spans
                .iter()
                .find(|s| s.kind == SpanKind::Heading)
                .unwrap()
                .level,
            2
        );
        assert_eq!(of(text, &spans, SpanKind::Strong), ["**最大**"]);
        assert_eq!(of(text, &spans, SpanKind::Emphasis), ["_注意_"]);
        assert_eq!(
            of(text, &spans, SpanKind::Markup),
            ["## ", "**", "**", "_", "_"]
        );
    }

    #[test]
    fn heading_markup_and_body_never_overlap() {
        let text = "### 输入格式 ###\n\n## \n";
        let spans = markdown_spans(text);
        assert_eq!(of(text, &spans, SpanKind::Heading), ["输入格式"]);
        assert_eq!(of(text, &spans, SpanKind::Markup), ["### ", " ###", "## "]);
        let heading = spans.iter().find(|s| s.kind == SpanKind::Heading).unwrap();
        for m in spans.iter().filter(|s| s.kind == SpanKind::Markup) {
            assert!(
                m.range.end <= heading.range.start || m.range.start >= heading.range.end,
                "markup {:?} overlaps heading {:?}",
                &text[m.range.clone()],
                &text[heading.range.clone()]
            );
        }
    }

    #[test]
    fn math_content_and_fine_grained_tokens() {
        let text = "长为 $n$ 的序列，满足 $2 \\le n \\le 2 \\times 10^5$ 和 $a_1, a_2, \\ldots, a_n$：\n\n$$\n\\sum_{i=1}^n a_i \\\\\n\\frac{1}{2} & \\{x\\}\n$$\n";
        let spans = markdown_spans(text);
        assert_eq!(
            of(text, &spans, SpanKind::InlineMath),
            [
                "n",
                "2 \\le n \\le 2 \\times 10^5",
                "a_1, a_2, \\ldots, a_n"
            ]
        );
        assert_eq!(
            of(text, &spans, SpanKind::DisplayMath),
            ["\n\\sum_{i=1}^n a_i \\\\\n\\frac{1}{2} & \\{x\\}\n"]
        );
        assert_eq!(
            of(text, &spans, SpanKind::Markup),
            ["$", "$", "$", "$", "$", "$", "$$", "$$"]
        );
        assert_eq!(
            of(text, &spans, SpanKind::MathCommand),
            [
                "\\le", "\\le", "\\times", "\\ldots", "\\sum", "\\\\", "\\frac", "\\{", "\\}",
            ]
        );
        assert_eq!(
            of(text, &spans, SpanKind::MathOperator),
            [
                "^", "_", "_", "_", "_", "{", "}", "^", "_", "{", "}", "{", "}", "&"
            ]
        );
    }

    #[test]
    fn code_and_code_blocks_cpp_python_rust() {
        let text = concat!(
            "用 `long long`。\n\n",
            "```cpp\n",
            "#include <bits/stdc++.h>\n",
            "// 求和 100\n",
            "int main() { long long x = 1e9 + 0x3f; return 0; }\n",
            "```\n\n",
            "```python\n",
            "# 注释\n",
            "def solve(n):\n",
            "    s = f\"ans={n}\"\n",
            "    return 42\n",
            "```\n\n",
            "```rust\n",
            "fn f<'a>(s: &'a str) -> usize { let c = 'x'; 0..10; 100_000usize }\n",
            "```\n\n",
            "```\n",
            "int not_highlighted = 123;\n",
            "```\n",
        );
        let spans = markdown_spans(text);
        assert_eq!(of(text, &spans, SpanKind::Code), ["long long"]);
        assert_eq!(of(text, &spans, SpanKind::CodeBlock).len(), 4);
        assert_eq!(
            of(text, &spans, SpanKind::CodeComment),
            ["// 求和 100", "# 注释"]
        );
        assert_eq!(
            of(text, &spans, SpanKind::CodeString),
            ["f\"ans={n}\"", "'x'"]
        );
        assert_eq!(
            of(text, &spans, SpanKind::CodeKeyword),
            [
                "#include", "int", "long", "long", "return", "def", "return", "fn", "str", "usize",
                "let",
            ]
        );
        assert_eq!(
            of(text, &spans, SpanKind::CodeNumber),
            ["1e9", "0x3f", "0", "42", "0", "10", "100_000usize"]
        );
    }

    #[test]
    fn links_and_list_markers() {
        let text = "- 见 [原题](https://codeforces.com/1)\n";
        let spans = markdown_spans(text);
        assert_eq!(of(text, &spans, SpanKind::Link), ["原题"]);
        assert_eq!(
            of(text, &spans, SpanKind::Url),
            ["(https://codeforces.com/1)"]
        );
        assert_eq!(of(text, &spans, SpanKind::Markup), ["- ", "[", "]"]);
    }

    #[test]
    fn dollars_in_code_are_not_math() {
        let text = "`$x$`\n";
        let spans = markdown_spans(text);
        assert!(of(text, &spans, SpanKind::InlineMath).is_empty());
        assert!(of(text, &spans, SpanKind::MathCommand).is_empty());
        assert!(of(text, &spans, SpanKind::MathOperator).is_empty());
    }

    #[test]
    fn rust_char_escape_with_multibyte_utf8_never_panics() {
        let text = "```rust\nlet a = '\\中'; let b = '中'; let c = '\\中\n```\n";
        let spans = markdown_spans(text);
        assert_eq!(of(text, &spans, SpanKind::CodeString), ["'\\中'", "'中'"]);
        assert_eq!(
            of(text, &spans, SpanKind::CodeKeyword),
            ["let", "let", "let"]
        );
    }
}
