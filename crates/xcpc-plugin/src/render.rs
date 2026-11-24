use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::error::{PluginError, Result};

const MAX_RENDER_DIM: u32 = 4096;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderedRgba {
    pub width: u32,
    pub height: u32,
    pub rgba_base64: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HighlightToken {
    /// One of: `keyword|type|string|number|comment|operator|ident|plain`.
    pub kind: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HighlightLine {
    pub tokens: Vec<HighlightToken>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HighlightResult {
    pub language: String,
    pub lines: Vec<HighlightLine>,
}

fn render_svg_pixmap(svg: &str, scale: f32) -> Result<tiny_skia::Pixmap> {
    if !scale.is_finite() || scale <= 0.0 {
        return Err(PluginError::Render(
            "scale must be a positive finite number".into(),
        ));
    }
    let scale = scale.clamp(0.1, 8.0);

    let opts = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_data(svg.as_bytes(), &opts)
        .map_err(|e| PluginError::Render(format!("invalid svg: {e}")))?;

    let size = tree.size();
    let width = ((size.width() * scale).ceil() as u32).clamp(1, MAX_RENDER_DIM);
    let height = ((size.height() * scale).ceil() as u32).clamp(1, MAX_RENDER_DIM);

    let mut pixmap = tiny_skia::Pixmap::new(width, height).ok_or_else(|| {
        PluginError::Render(format!("failed to allocate pixmap {width}x{height}"))
    })?;

    let sx = width as f32 / size.width().max(1.0);
    let sy = height as f32 / size.height().max(1.0);
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(sx, sy),
        &mut pixmap.as_mut(),
    );

    Ok(pixmap)
}

/// Renders an SVG string to a PNG image and returns it as a base64-encoded string.
pub fn svg_to_png_base64(svg: &str, scale: Option<f32>) -> Result<String> {
    let pixmap = render_svg_pixmap(svg, scale.unwrap_or(1.0))?;
    let mut png_bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png_bytes, pixmap.width(), pixmap.height());
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|e| PluginError::Render(format!("png header error: {e}")))?;
        writer
            .write_image_data(pixmap.data())
            .map_err(|e| PluginError::Render(format!("png encode error: {e}")))?;
    }

    Ok(base64::engine::general_purpose::STANDARD.encode(png_bytes))
}

/// Renders an SVG string to raw premultiplied RGBA8888 pixel bytes (base64-encoded).
pub fn svg_to_rgba(svg: &str, scale: f32) -> Result<RenderedRgba> {
    let pixmap = render_svg_pixmap(svg, scale)?;
    Ok(RenderedRgba {
        width: pixmap.width(),
        height: pixmap.height(),
        rgba_base64: base64::engine::general_purpose::STANDARD.encode(pixmap.data()),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lang {
    Cpp,
    C,
    Rust,
    Python,
    Js,
    Ts,
    Json,
    Toml,
    Text,
}

impl Lang {
    fn from_name(raw: &str) -> (Self, &'static str) {
        match raw.trim().to_ascii_lowercase().as_str() {
            "cpp" | "c++" | "cc" | "cxx" | "hpp" | "hh" => (Self::Cpp, "cpp"),
            "c" | "h" => (Self::C, "c"),
            "rust" | "rs" => (Self::Rust, "rust"),
            "python" | "py" | "py3" => (Self::Python, "python"),
            "js" | "javascript" | "jsx" | "mjs" | "cjs" => (Self::Js, "js"),
            "ts" | "typescript" | "tsx" => (Self::Ts, "ts"),
            "json" | "jsonc" => (Self::Json, "json"),
            "toml" => (Self::Toml, "toml"),
            _ => (Self::Text, "text"),
        }
    }

    fn supports_slash_comments(self) -> bool {
        matches!(self, Self::Cpp | Self::C | Self::Rust | Self::Js | Self::Ts)
    }

    fn supports_hash_comments(self) -> bool {
        matches!(self, Self::Python | Self::Toml)
    }

    fn supports_backtick_strings(self) -> bool {
        matches!(self, Self::Js | Self::Ts)
    }

    fn is_keyword(self, word: &str) -> bool {
        match self {
            Self::Cpp => matches!(
                word,
                "alignas"
                    | "alignof"
                    | "and"
                    | "asm"
                    | "auto"
                    | "break"
                    | "case"
                    | "catch"
                    | "class"
                    | "co_await"
                    | "co_return"
                    | "co_yield"
                    | "concept"
                    | "const"
                    | "consteval"
                    | "constexpr"
                    | "constinit"
                    | "continue"
                    | "decltype"
                    | "default"
                    | "delete"
                    | "do"
                    | "else"
                    | "enum"
                    | "explicit"
                    | "export"
                    | "extern"
                    | "false"
                    | "for"
                    | "friend"
                    | "goto"
                    | "if"
                    | "inline"
                    | "mutable"
                    | "namespace"
                    | "new"
                    | "noexcept"
                    | "not"
                    | "nullptr"
                    | "operator"
                    | "or"
                    | "override"
                    | "private"
                    | "protected"
                    | "public"
                    | "register"
                    | "requires"
                    | "return"
                    | "sizeof"
                    | "static"
                    | "static_assert"
                    | "struct"
                    | "switch"
                    | "template"
                    | "this"
                    | "throw"
                    | "true"
                    | "try"
                    | "typedef"
                    | "typeid"
                    | "typename"
                    | "union"
                    | "using"
                    | "virtual"
                    | "volatile"
                    | "while"
            ),
            Self::C => matches!(
                word,
                "auto"
                    | "break"
                    | "case"
                    | "const"
                    | "continue"
                    | "default"
                    | "do"
                    | "else"
                    | "enum"
                    | "extern"
                    | "false"
                    | "for"
                    | "goto"
                    | "if"
                    | "inline"
                    | "NULL"
                    | "register"
                    | "restrict"
                    | "return"
                    | "sizeof"
                    | "static"
                    | "struct"
                    | "switch"
                    | "true"
                    | "typedef"
                    | "union"
                    | "volatile"
                    | "while"
            ),
            Self::Rust => matches!(
                word,
                "as" | "async"
                    | "await"
                    | "break"
                    | "const"
                    | "continue"
                    | "crate"
                    | "dyn"
                    | "else"
                    | "enum"
                    | "extern"
                    | "false"
                    | "fn"
                    | "for"
                    | "if"
                    | "impl"
                    | "in"
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
                    | "struct"
                    | "super"
                    | "trait"
                    | "true"
                    | "type"
                    | "unsafe"
                    | "use"
                    | "where"
                    | "while"
            ),
            Self::Python => matches!(
                word,
                "False"
                    | "None"
                    | "True"
                    | "and"
                    | "as"
                    | "assert"
                    | "async"
                    | "await"
                    | "break"
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
            Self::Js | Self::Ts => matches!(
                word,
                "as" | "async"
                    | "await"
                    | "break"
                    | "case"
                    | "catch"
                    | "class"
                    | "const"
                    | "continue"
                    | "debugger"
                    | "default"
                    | "delete"
                    | "do"
                    | "else"
                    | "enum"
                    | "export"
                    | "extends"
                    | "false"
                    | "finally"
                    | "for"
                    | "from"
                    | "function"
                    | "if"
                    | "implements"
                    | "import"
                    | "in"
                    | "instanceof"
                    | "interface"
                    | "let"
                    | "new"
                    | "null"
                    | "of"
                    | "private"
                    | "protected"
                    | "public"
                    | "readonly"
                    | "return"
                    | "static"
                    | "super"
                    | "switch"
                    | "this"
                    | "throw"
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
            Self::Json => matches!(word, "true" | "false" | "null"),
            Self::Toml => matches!(word, "true" | "false"),
            Self::Text => false,
        }
    }

    fn is_type(self, word: &str) -> bool {
        match self {
            Self::Cpp => matches!(
                word,
                "bool"
                    | "char"
                    | "char16_t"
                    | "char32_t"
                    | "char8_t"
                    | "double"
                    | "float"
                    | "int"
                    | "int16_t"
                    | "int32_t"
                    | "int64_t"
                    | "int8_t"
                    | "long"
                    | "map"
                    | "pair"
                    | "priority_queue"
                    | "queue"
                    | "set"
                    | "short"
                    | "signed"
                    | "size_t"
                    | "stack"
                    | "string"
                    | "tuple"
                    | "uint16_t"
                    | "uint32_t"
                    | "uint64_t"
                    | "uint8_t"
                    | "unordered_map"
                    | "unordered_set"
                    | "unsigned"
                    | "vector"
                    | "void"
                    | "wchar_t"
            ),
            Self::C => matches!(
                word,
                "bool"
                    | "char"
                    | "double"
                    | "float"
                    | "int"
                    | "int16_t"
                    | "int32_t"
                    | "int64_t"
                    | "int8_t"
                    | "long"
                    | "short"
                    | "signed"
                    | "size_t"
                    | "uint16_t"
                    | "uint32_t"
                    | "uint64_t"
                    | "uint8_t"
                    | "unsigned"
                    | "void"
            ),
            Self::Rust => matches!(
                word,
                "bool"
                    | "char"
                    | "f32"
                    | "f64"
                    | "i128"
                    | "i16"
                    | "i32"
                    | "i64"
                    | "i8"
                    | "isize"
                    | "Option"
                    | "Result"
                    | "Self"
                    | "str"
                    | "String"
                    | "u128"
                    | "u16"
                    | "u32"
                    | "u64"
                    | "u8"
                    | "usize"
                    | "Vec"
            ),
            Self::Python => matches!(
                word,
                "bool"
                    | "bytes"
                    | "dict"
                    | "float"
                    | "frozenset"
                    | "int"
                    | "list"
                    | "object"
                    | "set"
                    | "str"
                    | "tuple"
            ),
            Self::Ts => matches!(
                word,
                "any"
                    | "bigint"
                    | "boolean"
                    | "never"
                    | "number"
                    | "object"
                    | "Record"
                    | "string"
                    | "symbol"
                    | "unknown"
                    | "Array"
                    | "Map"
                    | "Promise"
                    | "Set"
            ),
            Self::Js => matches!(
                word,
                "Array"
                    | "BigInt"
                    | "Date"
                    | "Map"
                    | "Math"
                    | "Number"
                    | "Object"
                    | "Promise"
                    | "Set"
                    | "String"
            ),
            Self::Json | Self::Toml | Self::Text => false,
        }
    }
}

fn push_token(tokens: &mut Vec<HighlightToken>, kind: &str, text: &str) {
    if text.is_empty() {
        return;
    }
    if let Some(last) = tokens.last_mut()
        && last.kind == kind
    {
        last.text.push_str(text);
        return;
    }
    tokens.push(HighlightToken {
        kind: kind.to_string(),
        text: text.to_string(),
    });
}

/// Lightweight offline lexical syntax highlighter supporting `cpp`, `c`, `rust`,
/// `python`, `js`, `ts`, `json`, `toml`, and `text`.
pub fn highlight_code(code: &str, lang_name: &str) -> HighlightResult {
    let (lang, canonical) = Lang::from_name(lang_name);
    let mut lines = Vec::new();
    let mut in_block_comment = false;

    for raw_line in code.split('\n') {
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        if lang == Lang::Text {
            let tokens = if line.is_empty() {
                Vec::new()
            } else {
                vec![HighlightToken {
                    kind: "plain".to_string(),
                    text: line.to_string(),
                }]
            };
            lines.push(HighlightLine { tokens });
            continue;
        }

        let mut tokens = Vec::new();
        let bytes = line.as_bytes();
        let len = bytes.len();
        let mut i = 0usize;

        while i < len {
            if in_block_comment {
                if let Some(end_rel) = line[i..].find("*/") {
                    let end = i + end_rel + 2;
                    push_token(&mut tokens, "comment", &line[i..end]);
                    i = end;
                    in_block_comment = false;
                } else {
                    push_token(&mut tokens, "comment", &line[i..]);
                    break;
                }
                continue;
            }

            let b = bytes[i];

            // Whitespace -> plain
            if b.is_ascii_whitespace() {
                let start = i;
                while i < len && bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                push_token(&mut tokens, "plain", &line[start..i]);
                continue;
            }

            // C/C++ preprocessor directive at start of line
            if matches!(lang, Lang::Cpp | Lang::C)
                && b == b'#'
                && tokens.iter().all(|t| t.kind == "plain")
            {
                let start = i;
                i += 1;
                while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                push_token(&mut tokens, "keyword", &line[start..i]);
                continue;
            }

            // Line / block comments
            if lang.supports_slash_comments() && b == b'/' && i + 1 < len {
                if bytes[i + 1] == b'/' {
                    push_token(&mut tokens, "comment", &line[i..]);
                    break;
                }
                if bytes[i + 1] == b'*' {
                    if let Some(end_rel) = line[i + 2..].find("*/") {
                        let end = i + 2 + end_rel + 2;
                        push_token(&mut tokens, "comment", &line[i..end]);
                        i = end;
                    } else {
                        push_token(&mut tokens, "comment", &line[i..]);
                        in_block_comment = true;
                        break;
                    }
                    continue;
                }
            }
            if lang.supports_hash_comments() && b == b'#' {
                push_token(&mut tokens, "comment", &line[i..]);
                break;
            }

            // Strings
            if b == b'"'
                || (b == b'\'' && lang != Lang::Json)
                || (b == b'`' && lang.supports_backtick_strings())
            {
                let quote = b;
                // Rust lifetimes like `'a` should not be lexed as unclosed strings
                if lang == Lang::Rust
                    && quote == b'\''
                    && i + 1 < len
                    && (bytes[i + 1].is_ascii_alphabetic() || bytes[i + 1] == b'_')
                    && !(i + 2 < len && bytes[i + 2] == b'\'')
                {
                    let start = i;
                    i += 1;
                    while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                        i += 1;
                    }
                    push_token(&mut tokens, "type", &line[start..i]);
                    continue;
                }

                let start = i;
                i += 1;
                while i < len {
                    if bytes[i] == b'\\' && i + 1 < len {
                        i += 2;
                        continue;
                    }
                    if bytes[i] == quote {
                        i += 1;
                        break;
                    }
                    i += 1;
                }
                push_token(&mut tokens, "string", &line[start..i]);
                continue;
            }

            // Numbers
            if b.is_ascii_digit() || (b == b'.' && i + 1 < len && bytes[i + 1].is_ascii_digit()) {
                let start = i;
                if b == b'0'
                    && i + 1 < len
                    && matches!(bytes[i + 1], b'x' | b'X' | b'b' | b'B' | b'o' | b'O')
                {
                    i += 2;
                    while i < len
                        && (bytes[i].is_ascii_hexdigit() || bytes[i] == b'_' || bytes[i] == b'\'')
                    {
                        i += 1;
                    }
                } else {
                    while i < len
                        && (bytes[i].is_ascii_digit()
                            || bytes[i] == b'_'
                            || bytes[i] == b'\''
                            || bytes[i] == b'.')
                    {
                        i += 1;
                    }
                    if i < len && matches!(bytes[i], b'e' | b'E') {
                        i += 1;
                        if i < len && matches!(bytes[i], b'+' | b'-') {
                            i += 1;
                        }
                        while i < len && bytes[i].is_ascii_digit() {
                            i += 1;
                        }
                    }
                }
                // Trailing numeric type suffixes (e.g. `1000000007LL`, `1u64`, `0.5f`)
                while i < len && bytes[i].is_ascii_alphanumeric() {
                    i += 1;
                }
                push_token(&mut tokens, "number", &line[start..i]);
                continue;
            }

            // Identifiers / Keywords / Types
            if b.is_ascii_alphabetic() || b == b'_' {
                let start = i;
                while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                let word = &line[start..i];
                let kind = if lang.is_keyword(word) {
                    "keyword"
                } else if lang.is_type(word) {
                    "type"
                } else {
                    "ident"
                };
                push_token(&mut tokens, kind, word);
                continue;
            }

            // Operators and punctuation
            if b.is_ascii_punctuation() {
                let start = i;
                i += 1;
                // Group multi-character operators cleanly
                while i < len
                    && matches!(
                        bytes[i],
                        b'=' | b'<'
                            | b'>'
                            | b'!'
                            | b'&'
                            | b'|'
                            | b'+'
                            | b'-'
                            | b'*'
                            | b'/'
                            | b'%'
                            | b'^'
                            | b':'
                    )
                    && matches!(
                        b,
                        b'=' | b'<'
                            | b'>'
                            | b'!'
                            | b'&'
                            | b'|'
                            | b'+'
                            | b'-'
                            | b'*'
                            | b'/'
                            | b'%'
                            | b'^'
                            | b':'
                    )
                {
                    i += 1;
                }
                push_token(&mut tokens, "operator", &line[start..i]);
                continue;
            }

            // Non-ASCII UTF-8 character -> plain
            let ch = line[i..].chars().next().unwrap();
            let next = i + ch.len_utf8();
            push_token(&mut tokens, "plain", &line[i..next]);
            i = next;
        }

        lines.push(HighlightLine { tokens });
    }

    HighlightResult {
        language: canonical.to_string(),
        lines,
    }
}
