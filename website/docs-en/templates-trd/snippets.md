---
title: Algorithm Snippets & Single-File Bundler
---

# Algorithm Snippets & Single-File Bundler

Each algorithm snippet lives under `snippets/<slug>/`, where the directory name `slug` is its immutable ID. A snippet can be either a reusable C++ header with `code.hpp` or a formula-only reference card containing only `notes.md`.

## Directory Structure and `snippet.toml`

A standard code snippet directory contains `snippet.toml`, `code.hpp` (optional), `notes.md` (optional), and verification scripts (`test.cpp`, `verify_*.cpp`):

```toml
title = "Number Theoretic Transform (NTT)"
tags = ["poly", "math"]
time_complexity = "O(n log n)"
space_complexity = "O(n)"
description = "Polynomial multiplication modulo 998244353"
source_problem = "poly-mul"          # Optional: workspace problem slug this snippet was extracted from

[[verify]]
kind = "self"
file = "test.cpp"

[[verify]]
kind = "library_checker"
problem = "convolution_mod"
adapter = "verify_conv.cpp"
```

| Field | Required | Description |
| :--- | :--- | :--- |
| `title` | Yes | Display title (used in search and TRD section headings) |
| `tags` | No | String array for tag filtering |
| `time_complexity` | No | Time complexity note (also accepts `time-complexity`) |
| `space_complexity` | No | Space complexity note (also accepts `space-complexity`) |
| `description` | No | Short summary (shown in metadata, indexed for search, and passed to Typst templates as `item.description`) |
| `source_problem` | No | Linked workspace problem slug (also accepts `source-problem`) |
| `[[verify]]` | No | Verification sources: `self`, `problem`, `library_checker` (see [Three-Source Automatic Verification](./verification.md)) |

### Formula Cards

When a snippet directory **does not contain `code.hpp`** (only `snippet.toml` and `notes.md`), it is recognized as a **formula card** (e.g., number theory identities, inversion formulas, or geometry derivations). Formula cards render directly as Markdown + LaTeX math in the TRD, do not participate in code hashing, and do not require code verification during strict finalization.

### Common Management Commands

```bash
# List all snippets (filterable by tag, verification status, or formula-only)
xcpc snippet list
xcpc snippet list --tag poly --status passed
xcpc snippet list --formula-only

# Inspect a single snippet (direct deps, reverse deps, linked problem, total checksum, and status)
xcpc snippet show ntt

# Create a new snippet (add --formula-only for a formula card)
xcpc snippet new segtree --title "Lazy Segment Tree" --tag ds --time "O(log n)" --space "O(n)"
xcpc snippet new mobius-notes --title "Mobius Inversion Identities" --formula-only

# Delete a snippet
xcpc snippet delete old-template --yes
```

## Dependency DAG and Header Include Convention

Dependencies between snippets are **never duplicated by hand in `snippet.toml`**; instead, the engine derives them directly from standard C++ `#include` directives in `code.hpp`:

```cpp
// snippets/ntt/code.hpp
#pragma once
#include "modint/code.hpp"
#include <algorithm>
#include <vector>
```

- **Include format**: Use `#include "<slug>/code.hpp"` (relative paths such as `"../<slug>/code.hpp"` are also supported).
- **Natively compilable**: The verification engine passes `-I <workspace>/snippets` automatically when compiling, so both snippets and local solution files compile directly with `g++` or `clang++` without any custom preprocessor.
- **Topological sort and cycle detection**: The engine scans `snippets/*/code.hpp` to build a DAG, maintaining direct dependencies (`deps`), transitive dependencies (`transitive_deps`), and reverse dependencies (`reverse_deps`) for every snippet. Any cyclic `#include` produces an exact cycle path error during bundling, verification, or TRD pre-checks.
- **Automatic print-view stripping**: When rendering a Team Reference Document, `#pragma once` and `#include "<slug>/code.hpp"` lines are automatically stripped from the code body and turned into concise dependency badges in the snippet header to save page space.

## Single-File Bundler (`xcpc bundle`)

During contests or practice, local code can freely `#include` workspace snippets or local headers. Before submitting to an OJ, run `xcpc bundle` to inline all local dependencies in topological order into a single self-contained C++ file:

```bash
# Bundle a local solution file and copy directly to the system clipboard
xcpc bundle main.cpp --copy

# Bundle to an output file with extra header search paths
xcpc bundle main.cpp -I ./include -I ../lib -o submit.cpp

# Bundle a workspace snippet and all its transitive dependencies (printed to stdout)
xcpc bundle --snippet ntt

# Compact mode: strip comments, collapse blank lines, and omit boundary markers
xcpc bundle main.cpp --compact --copy
```

| Flag | Description |
| :--- | :--- |
| `<file.cpp>` | Local C++ source file to bundle (mutually exclusive with `--snippet`) |
| `--snippet <slug>` | Bundle a workspace snippet and all its transitive dependencies |
| `-I, --include-dir <DIR>` | Additional search directory for `#include "..."` (repeatable) |
| `-o, --output <PATH>` | Write bundled output to a file (prints to stdout if neither `-o` nor `--copy` is given) |
| `--copy` | Copy bundled code to the system clipboard |
| `--compact` | Strip C/C++ comments, collapse consecutive blank lines, and omit `// --- <rel> ---` file boundary comments |

During bundling, the engine automatically:

1. Recursively resolves and inlines all local `#include "..."` directives in depth-first topological order, respecting `#pragma once` so each header is inlined once.
2. Collects and deduplicates all system headers `#include <...>`, hoisting them to the top of the output file.
3. Preserves `// --- <rel> ---` boundary comments outside `--compact` mode for easier review and debugging.

## KACTL++ Cumulative Line Checksums

When transcribing code from a printed handbook during a contest, a single typo can waste significant debugging time. XCPC Workstation includes a pure-Rust **cumulative 6-character MD5 checksum** engine compatible with the KACTL ecosystem:

### Preprocessing and Computation

1. Starting from the print-view code (`#pragma once` and `#include "<slug>/code.hpp"` already stripped; if `strip-comments` is enabled, comments are stripped and blank lines collapsed beforehand), the engine accumulates line prefixes `1..=N`.
2. C/C++ comments are stripped (properly preserving string literals, character literals, and C++11 raw strings `R"delim(...)delim"`), and all POSIX whitespace characters (spaces, tabs, newlines, etc.) are removed.
3. An RFC 1321 MD5 digest is computed over the compacted byte stream, taking the first 6 lowercase hexadecimal characters as the short checksum.
4. By default, cumulative checksums are printed every `hash-interval` lines (default `10`), at top-level function/struct closing braces `}`, and on the final line of code (lines inside an unclosed block comment `/* ...` are skipped automatically).

### One-Line Terminal Verification During Contests

This algorithm is **byte-for-byte identical** to the standard Linux contest terminal command (which is also printed below the TRD table of contents whenever `hash-interval > 0`):

```bash
sed -n '1,Np' a.cpp | cpp -dD -P -fpreprocessed | tr -d '[:space:]' | md5sum | cut -c-6
```

After typing up a snippet, first set `N` to the last line to check the full digest. If it does not match, binary-search `N` across the printed checkpoint line numbers to isolate any typo within 10 lines in 3–4 checks.
