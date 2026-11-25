//! KACTL++ printed-view preprocessor and pure-Rust cumulative MD5 verification hash.
//!
//! Matches the contest-hall one-liner byte-for-byte:
//! `sed -n '1,Np' a.cpp | cpp -dD -P -fpreprocessed | tr -d '[:space:]' | md5sum | cut -c-6`

use std::collections::HashSet;
use std::fmt::Write as _;

use xcpc_domain::{CodeHashBlock, PrintedSnippetCode, check_id};

/// Computes the 128-bit RFC 1321 MD5 digest of `input`.
pub fn md5_digest(input: &[u8]) -> [u8; 16] {
    const K: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613,
        0xfd469501, 0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193,
        0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d,
        0x02441453, 0xd8a1e681, 0xe7d3fbc8, 0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
        0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122,
        0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa,
        0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665, 0xf4292244,
        0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb,
        0xeb86d391,
    ];
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];

    let bit_len = (input.len() as u64).wrapping_mul(8);
    let mut msg = Vec::with_capacity(input.len() + 72);
    msg.extend_from_slice(input);
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_le_bytes());

    let mut a0: u32 = 0x6745_2301;
    let mut b0: u32 = 0xefcd_ab89;
    let mut c0: u32 = 0x98ba_dcfe;
    let mut d0: u32 = 0x1032_5476;

    for chunk in msg.chunks_exact(64) {
        let mut m = [0u32; 16];
        for (i, word) in m.iter_mut().enumerate() {
            let off = i * 4;
            *word =
                u32::from_le_bytes([chunk[off], chunk[off + 1], chunk[off + 2], chunk[off + 3]]);
        }

        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for i in 0..64 {
            let (f, g) = match i {
                0..=15 => ((b & c) | ((!b) & d), i),
                16..=31 => ((d & b) | ((!d) & c), (5 * i + 1) % 16),
                32..=47 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | (!d)), (7 * i) % 16),
            };
            let tmp = d;
            d = c;
            c = b;
            let sum = a.wrapping_add(f).wrapping_add(K[i]).wrapping_add(m[g]);
            b = b.wrapping_add(sum.rotate_left(S[i]));
            a = tmp;
        }

        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }

    let mut out = [0u8; 16];
    out[0..4].copy_from_slice(&a0.to_le_bytes());
    out[4..8].copy_from_slice(&b0.to_le_bytes());
    out[8..12].copy_from_slice(&c0.to_le_bytes());
    out[12..16].copy_from_slice(&d0.to_le_bytes());
    out
}

/// Returns the lowercase 6-character hex prefix of the MD5 digest of `bytes`.
pub fn md5_hex_6(bytes: &[u8]) -> String {
    let digest = md5_digest(bytes);
    let mut out = String::with_capacity(6);
    for b in &digest[..3] {
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

/// Strips C/C++ comments (`//...` and `/*...*/`) while preserving string literals,
/// character literals, and C++11 raw string literals (`R"delim(...)delim"`).
///
/// Block comments are replaced by a single space (or preserved newlines if multiline)
/// so adjacent tokens are never accidentally glued together.
pub fn strip_cpp_comments(source: &str) -> String {
    let bytes = source.as_bytes();
    let n = bytes.len();
    let mut out = Vec::with_capacity(n);
    let mut i = 0;

    while i < n {
        // Check for C++11 raw string literal: R"delim(...)delim"
        // (optionally prefixed by u8, u, U, L; must not be part of a longer identifier).
        if let Some((prefix_len, delim_start, delim_end)) = try_match_raw_string_start(bytes, i) {
            let delim = &bytes[delim_start..delim_end];
            // Emit prefix + R" + delim + (
            let content_start = delim_end + 1;
            out.extend_from_slice(&bytes[i..content_start]);
            i = content_start;
            // Scan until )delim"
            while i < n {
                if bytes[i] == b')'
                    && i + 1 + delim.len() < n
                    && &bytes[i + 1..i + 1 + delim.len()] == delim
                    && bytes[i + 1 + delim.len()] == b'"'
                {
                    let end = i + 2 + delim.len();
                    out.extend_from_slice(&bytes[i..end]);
                    i = end;
                    break;
                }
                out.push(bytes[i]);
                i += 1;
            }
            let _ = prefix_len;
            continue;
        }

        // Line splice outside raw strings: `\<LF>` or `\<CR><LF>`
        if bytes[i] == b'\\' {
            if i + 1 < n && bytes[i + 1] == b'\n' {
                i += 2;
                continue;
            }
            if i + 2 < n && bytes[i + 1] == b'\r' && bytes[i + 2] == b'\n' {
                i += 3;
                continue;
            }
        }

        // String literal
        if bytes[i] == b'"' {
            out.push(b'"');
            i += 1;
            while i < n {
                if bytes[i] == b'\\' {
                    if i + 1 < n && bytes[i + 1] == b'\n' {
                        i += 2;
                        continue;
                    }
                    if i + 2 < n && bytes[i + 1] == b'\r' && bytes[i + 2] == b'\n' {
                        i += 3;
                        continue;
                    }
                    out.push(bytes[i]);
                    i += 1;
                    if i < n {
                        out.push(bytes[i]);
                        i += 1;
                    }
                    continue;
                }
                let ch = bytes[i];
                out.push(ch);
                i += 1;
                if ch == b'"' {
                    break;
                }
            }
            continue;
        }

        // Character literal
        if bytes[i] == b'\'' {
            out.push(b'\'');
            i += 1;
            while i < n {
                if bytes[i] == b'\\' {
                    if i + 1 < n && bytes[i + 1] == b'\n' {
                        i += 2;
                        continue;
                    }
                    if i + 2 < n && bytes[i + 1] == b'\r' && bytes[i + 2] == b'\n' {
                        i += 3;
                        continue;
                    }
                    out.push(bytes[i]);
                    i += 1;
                    if i < n {
                        out.push(bytes[i]);
                        i += 1;
                    }
                    continue;
                }
                let ch = bytes[i];
                out.push(ch);
                i += 1;
                if ch == b'\'' {
                    break;
                }
            }
            continue;
        }

        // Line comment `//...`
        if bytes[i] == b'/' && i + 1 < n && bytes[i + 1] == b'/' {
            i += 2;
            while i < n {
                if bytes[i] == b'\\' {
                    if i + 1 < n && bytes[i + 1] == b'\n' {
                        i += 2;
                        continue;
                    }
                    if i + 2 < n && bytes[i + 1] == b'\r' && bytes[i + 2] == b'\n' {
                        i += 3;
                        continue;
                    }
                }
                if bytes[i] == b'\n' {
                    break;
                }
                i += 1;
            }
            continue;
        }

        // Block comment `/*...*/`
        if bytes[i] == b'/' && i + 1 < n && bytes[i + 1] == b'*' {
            i += 2;
            let mut saw_newline = false;
            while i < n {
                if bytes[i] == b'\n' {
                    saw_newline = true;
                }
                if bytes[i] == b'*' && i + 1 < n && bytes[i + 1] == b'/' {
                    i += 2;
                    break;
                }
                i += 1;
            }
            out.push(if saw_newline { b'\n' } else { b' ' });
            continue;
        }

        out.push(bytes[i]);
        i += 1;
    }

    String::from_utf8(out).expect("valid UTF-8 preserved by ASCII comment stripping")
}

fn is_ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Checks if `bytes[i..]` begins a C++11 raw string literal (`R"delim(...)delim"`,
/// optionally prefixed by `u8`, `u`, `U`, `L`). Returns `(prefix_len, delim_start, delim_end)`
/// where `bytes[delim_end] == b'('`.
fn try_match_raw_string_start(bytes: &[u8], i: usize) -> Option<(usize, usize, usize)> {
    if i > 0 && is_ident_char(bytes[i - 1]) {
        return None;
    }
    let n = bytes.len();
    let prefix_len = if i + 3 < n && &bytes[i..i + 4] == b"u8R\"" {
        4
    } else if i + 2 < n
        && matches!(bytes[i], b'u' | b'U' | b'L')
        && bytes[i + 1] == b'R'
        && bytes[i + 2] == b'"'
    {
        3
    } else if i + 1 < n && bytes[i] == b'R' && bytes[i + 1] == b'"' {
        2
    } else {
        return None;
    };

    let delim_start = i + prefix_len;
    let mut j = delim_start;
    while j < n && j - delim_start <= 16 {
        let ch = bytes[j];
        if ch == b'(' {
            return Some((prefix_len, delim_start, j));
        }
        if ch == b')' || ch == b'\\' || ch.is_ascii_whitespace() {
            return None;
        }
        j += 1;
    }
    None
}

/// Computes the 6-character KACTL++ verification hash of `code`, matching:
/// `cpp -dD -P -fpreprocessed | tr -d '[:space:]' | md5sum | cut -c-6`.
pub fn compute_cpp_hash_6(code: &str) -> String {
    let stripped = strip_cpp_comments(code);
    let compact: Vec<u8> = stripped.bytes().filter(|&b| !is_posix_space(b)).collect();
    md5_hex_6(&compact)
}

fn is_posix_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

/// Checks if a trimmed line is `#pragma once` (ignoring trailing comments).
pub(crate) fn is_pragma_once_line(trimmed: &str) -> bool {
    let Some(rest) = trimmed.strip_prefix('#') else {
        return false;
    };
    let rest = rest.trim_start();
    let Some(rest) = rest.strip_prefix("pragma") else {
        return false;
    };
    if !rest.starts_with(|c: char| c.is_ascii_whitespace()) {
        return false;
    }
    let rest = rest.trim_start();
    let Some(after) = rest.strip_prefix("once") else {
        return false;
    };
    let after = after.trim_start();
    after.is_empty() || after.starts_with("//") || after.starts_with("/*")
}

/// Parses a `#include "..."` directive on a single trimmed line, returning the quoted path.
pub(crate) fn parse_local_include_line(trimmed: &str) -> Option<&str> {
    let rest = trimmed.strip_prefix('#')?.trim_start();
    let rest = rest.strip_prefix("include")?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    let path = &rest[..end];
    (!path.is_empty()).then_some(path)
}

/// Parses a `#include <...>` directive on a single trimmed line, returning the full header
/// token inside angle brackets (e.g. `"vector"`).
pub(crate) fn parse_system_include_line(trimmed: &str) -> Option<&str> {
    let rest = trimmed.strip_prefix('#')?.trim_start();
    let rest = rest.strip_prefix("include")?.trim_start();
    let rest = rest.strip_prefix('<')?;
    let end = rest.find('>')?;
    let header = rest[..end].trim();
    (!header.is_empty()).then_some(header)
}

/// Extracts the workspace snippet slug from a local include path like `"modint/code.hpp"`
/// or `"../modint/code.hpp"`.
pub fn extract_snippet_slug_from_include(raw: &str) -> Option<String> {
    let norm = raw.replace('\\', "/");
    let trimmed = norm.trim_start_matches("./").trim_start_matches("../");
    let slug = trimmed.strip_suffix("/code.hpp")?;
    if !slug.contains('/') && check_id(slug).is_ok() {
        Some(slug.to_owned())
    } else {
        None
    }
}

/// Preprocesses `code.hpp` for printed TRD output and computes KACTL++ cumulative hashes.
///
/// - Strips `#pragma once` and `#include "<slug>/code.hpp"` lines (recording stripped slugs).
/// - Trims leading and trailing blank lines.
/// - Computes cumulative 6-char MD5 hashes at every `hash_interval` lines, at top-level
///   `}` block boundaries (when `hash_interval > 0`), and always on the final line.
pub fn preprocess_printed_code(code: &str, hash_interval: usize) -> PrintedSnippetCode {
    let normalized = code.replace("\r\n", "\n").replace('\r', "\n");
    let mut kept_lines: Vec<String> = Vec::new();
    let mut stripped_deps: Vec<String> = Vec::new();
    let mut seen_deps: HashSet<String> = HashSet::new();

    for line in normalized.lines() {
        let trimmed = line.trim();
        if is_pragma_once_line(trimmed) {
            continue;
        }
        if let Some(inc) = parse_local_include_line(trimmed)
            && let Some(slug) = extract_snippet_slug_from_include(inc)
        {
            if seen_deps.insert(slug.clone()) {
                stripped_deps.push(slug);
            }
            continue;
        }
        kept_lines.push(line.to_owned());
    }

    // Trim leading & trailing empty lines.
    while kept_lines.first().is_some_and(|l| l.trim().is_empty()) {
        kept_lines.remove(0);
    }
    while kept_lines.last().is_some_and(|l| l.trim().is_empty()) {
        kept_lines.pop();
    }

    if kept_lines.is_empty() {
        return PrintedSnippetCode {
            code: String::new(),
            lines: Vec::new(),
            stripped_deps,
            hash_blocks: Vec::new(),
            total_hash: String::new(),
        };
    }

    let printed_text = kept_lines.join("\n");
    let total_lines = kept_lines.len();

    // Track top-level closing brace lines (brace depth 1 -> 0) and lines that end inside
    // an unterminated `/* ... */` block comment (where `sed -n '1,Np' | cpp` would error).
    let (top_level_close_lines, inside_block_comment_lines) = analyze_line_boundaries(&kept_lines);

    let mut hash_blocks: Vec<CodeHashBlock> = Vec::new();
    let mut prefix = String::with_capacity(printed_text.len() + total_lines);

    for (idx, line) in kept_lines.iter().enumerate() {
        prefix.push_str(line);
        prefix.push('\n');
        let line_no = idx + 1;
        let is_final = line_no == total_lines;
        let emit = is_final
            || (hash_interval > 0
                && !inside_block_comment_lines.contains(&line_no)
                && (line_no % hash_interval == 0 || top_level_close_lines.contains(&line_no)));
        if emit {
            let hash = compute_cpp_hash_6(&prefix);
            // Per review T4: avoid emitting duplicate consecutive hashes (e.g. a blank line
            // immediately after a block end); always keep the final line marked `is_final`.
            if let Some(prev) = hash_blocks.last_mut()
                && prev.hash == hash
            {
                if is_final {
                    prev.line = line_no;
                    prev.is_final = true;
                }
            } else {
                hash_blocks.push(CodeHashBlock {
                    line: line_no,
                    hash,
                    is_final,
                });
            }
        }
    }

    let total_hash = hash_blocks
        .last()
        .map(|b| b.hash.clone())
        .unwrap_or_default();

    PrintedSnippetCode {
        code: printed_text,
        lines: kept_lines,
        stripped_deps,
        hash_blocks,
        total_hash,
    }
}

/// Identifies:
/// 1. 1-based line numbers where a top-level `{ ... }` block closes (depth 1 -> 0), and
/// 2. 1-based line numbers that end while still inside a `/* ... */` block comment.
fn analyze_line_boundaries(lines: &[String]) -> (HashSet<usize>, HashSet<usize>) {
    let mut close_braces = HashSet::new();
    let mut in_comment_end = HashSet::new();
    let mut depth: i32 = 0;
    let mut in_block_comment = false;

    for (idx, line) in lines.iter().enumerate() {
        let bytes = line.as_bytes();
        let n = bytes.len();
        let mut i = 0;
        let mut closed_top_level = false;

        while i < n {
            if in_block_comment {
                if bytes[i] == b'*' && i + 1 < n && bytes[i + 1] == b'/' {
                    in_block_comment = false;
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
                in_block_comment = true;
                i += 2;
                continue;
            }
            if bytes[i] == b'"' || bytes[i] == b'\'' {
                let quote = bytes[i];
                i += 1;
                while i < n {
                    if bytes[i] == b'\\' {
                        i += 2;
                        continue;
                    }
                    if bytes[i] == quote {
                        i += 1;
                        break;
                    }
                    i += 1;
                }
                continue;
            }
            if bytes[i] == b'{' {
                depth += 1;
            } else if bytes[i] == b'}' && depth > 0 {
                depth -= 1;
                if depth == 0 {
                    closed_top_level = true;
                }
            }
            i += 1;
        }

        if closed_top_level {
            close_braces.insert(idx + 1);
        }
        if in_block_comment {
            in_comment_end.insert(idx + 1);
        }
    }

    (close_braces, in_comment_end)
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::process::{Command, Stdio};

    use super::*;

    #[test]
    fn md5_matches_rfc1321_test_vectors() {
        let hex = |s: &[u8]| {
            let d = md5_digest(s);
            d.iter().map(|b| format!("{b:02x}")).collect::<String>()
        };
        assert_eq!(hex(b""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(hex(b"a"), "0cc175b9c0f1b6a831c399e269772661");
        assert_eq!(hex(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(hex(b"message digest"), "f96b697d7cb7938d525a2f31aaf161d0");
        assert_eq!(
            hex(b"abcdefghijklmnopqrstuvwxyz"),
            "c3fcd3d76192e4007dfb496cca67e13b"
        );
    }

    #[test]
    fn cumulative_hash_matches_stock_linux_cpp_pipeline_byte_for_byte() {
        // Prefer stock Linux `cpp -dD -P -fpreprocessed`; when `cpp` on PATH is Clang's
        // wrapper (which rejects `-fpreprocessed`), use GNU `g++ -E -dD -P -fpreprocessed -x c++ -`
        // (same GNU libcpp frontend).
        let probe = |cmd: &str| -> bool {
            let Ok(mut child) = Command::new("sh")
                .arg("-c")
                .arg(cmd)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
            else {
                return false;
            };
            if let Some(stdin) = child.stdin.as_mut() {
                let _ = stdin.write_all(b"int x = 1;\n");
            }
            child.wait().is_ok_and(|s| s.success())
        };

        let cpp_cmd = if probe("cpp -dD -P -fpreprocessed") {
            "cpp -dD -P -fpreprocessed"
        } else if probe("g++ -E -dD -P -fpreprocessed -x c++ -") {
            "g++ -E -dD -P -fpreprocessed -x c++ -"
        } else {
            return;
        };

        let sample_cpp = r##"
// Leading comment with tricky symbols: /* not a block comment */
struct ModInt {
    int v; // inline comment
    /* multi-line
       block comment with // inside
       and "quotes" */
    ModInt(long long x = 0) : v((x % 998244353 + 998244353) % 998244353) {}
    const char* tricky_str() const {
        return "http://example.com /* keep me */ \"escaped\"";
    }
    char tricky_char() const { return '/'; }
    const char* raw_str() const {
        return R"delim(// raw comment /* not stripped */)delim";
    }
    ModInt operator+(const ModInt& o) const {
        int s = v + o.v;
        return ModInt(s >= 998244353 ? s - 998244353 : s);
    }
};
"##;

        let pipeline = format!("{cpp_cmd} | tr -d '[:space:]' | md5sum | cut -c-6");
        let run_cpp_hash = |input: &str| -> String {
            let mut child = Command::new("sh")
                .arg("-c")
                .arg(&pipeline)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("spawn sh pipeline");
            child
                .stdin
                .as_mut()
                .unwrap()
                .write_all(input.as_bytes())
                .unwrap();
            let output = child.wait_with_output().unwrap();
            assert!(
                output.status.success(),
                "pipeline failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8(output.stdout).unwrap().trim().to_owned()
        };

        let printed = preprocess_printed_code(sample_cpp, 5);
        assert!(!printed.lines.is_empty());
        assert!(!printed.hash_blocks.is_empty());
        assert!(printed.hash_blocks.last().unwrap().is_final);

        for block in &printed.hash_blocks {
            let prefix = printed.lines[..block.line].join("\n") + "\n";
            let expected = run_cpp_hash(&prefix);
            assert_eq!(
                block.hash, expected,
                "mismatch at cumulative line {}:\n{}",
                block.line, prefix
            );
        }
        assert_eq!(printed.total_hash, run_cpp_hash(&(printed.code + "\n")));
    }

    #[test]
    fn preprocess_strips_pragma_once_and_snippet_includes() {
        let code = r#"#pragma once
#include "modint/code.hpp"
#include <vector>

struct Ntt {
    int n;
};
"#;
        let printed = preprocess_printed_code(code, 0);
        assert_eq!(printed.stripped_deps, vec!["modint"]);
        assert!(!printed.code.contains("#pragma once"));
        assert!(!printed.code.contains("modint/code.hpp"));
        assert!(printed.code.contains("#include <vector>"));
        // hash_interval = 0 emits only the final line.
        assert_eq!(printed.hash_blocks.len(), 1);
        assert!(printed.hash_blocks[0].is_final);
        assert_eq!(printed.hash_blocks[0].line, printed.lines.len());
    }
}
