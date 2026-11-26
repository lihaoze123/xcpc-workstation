#!/usr/bin/env python3
"""Anti-drift verification for XCPC Workstation plugin documentation."""

from __future__ import annotations

import argparse
import json
import re
import sys
import urllib.parse
from pathlib import Path

REQUIRED_DOCS = (
    "docs/plugin-development.md",
    "packages/plugin-sdk/README.md",
    "website/docs/plugins/development.md",
)

LINK_CHECK_FILES = (
    "docs/plugin-development.md",
    "packages/plugin-sdk/README.md",
    "README.md",
    "README_EN.md",
)

CLI_CHECK_FILES = (
    "docs/plugin-development.md",
    "packages/plugin-sdk/README.md",
)

WEBSITE_DOCS_DIR = "website/docs"

SDK_PACKAGE_JSON = "packages/plugin-sdk/package.json"
CLI_SOURCE_PATH = "crates/xcpc-mcp/src/main.rs"

IGNORED_LINK_SCHEMES = (
    "http://",
    "https://",
    "mailto:",
    "tel:",
    "data:",
    "javascript:",
)

FENCE_RE = re.compile(r"^\s*(`{3,}|~{3,})")
INLINE_CODE_RE = re.compile(r"(`+)(?:(?!\1).)*?\1")
MD_LINK_RE = re.compile(r"\[(?:[^\[\]\n]|\[[^\[\]\n]*\])*\]\(([^)\n]+)\)")
CLI_PLUGIN_CMD_RE = re.compile(
    r"\bxcpc(?:\s+(?:-w|--workspace)\s+\S+)?\s+plugin\s+([a-z][a-z0-9-]*)(?:\s+([a-z][a-z0-9-]*))?"
)


def pascal_to_kebab(name: str) -> str:
    """Convert a Rust enum variant name (PascalCase) to clap's default kebab-case."""
    step1 = re.sub(r"(.)([A-Z][a-z]+)", r"\1-\2", name)
    return re.sub(r"([a-z0-9])([A-Z])", r"\1-\2", step1).lower()


def extract_clap_subcommands(source: str, enum_name: str) -> set[str]:
    """Extract clap subcommand names from a Rust `#[derive(Subcommand)] enum`."""
    match = re.search(rf"\benum\s+{re.escape(enum_name)}\s*\{{", source)
    if not match:
        return set()

    pos = match.end()
    depth = 1
    end_pos = len(source)
    while pos < len(source) and depth > 0:
        ch = source[pos]
        if ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
            if depth == 0:
                end_pos = pos
                break
        pos += 1

    body = source[match.end() : end_pos]
    subcommands: set[str] = set()
    pending_names: list[str] = []
    pending_aliases: list[str] = []
    brace_depth = 0
    paren_depth = 0

    for raw_line in body.splitlines():
        line = re.sub(r"//.*$", "", raw_line).strip()
        if not line:
            continue

        if brace_depth == 0 and paren_depth == 0:
            if line.startswith("#["):
                name_m = re.search(r'\bname\s*=\s*"([^"]+)"', line)
                if name_m:
                    pending_names.append(name_m.group(1))
                for alias_m in re.finditer(r'\balias\s*=\s*"([^"]+)"', line):
                    pending_aliases.append(alias_m.group(1))
                continue

            var_m = re.match(r"^([A-Z][A-Za-z0-9_]*)\b", line)
            if var_m:
                if pending_names:
                    subcommands.update(pending_names)
                else:
                    subcommands.add(pascal_to_kebab(var_m.group(1)))
                subcommands.update(pending_aliases)
                pending_names.clear()
                pending_aliases.clear()

        brace_depth += line.count("{") - line.count("}")
        paren_depth += line.count("(") - line.count(")")

    return subcommands


def extract_markdown_links(text: str) -> list[tuple[int, str]]:
    """Extract (1-based line_number, raw_target) pairs from Markdown, skipping code blocks."""
    links: list[tuple[int, str]] = []
    in_fence = False
    fence_char = ""
    fence_len = 0

    for line_no, line in enumerate(text.splitlines(), start=1):
        fence_m = FENCE_RE.match(line)
        if fence_m:
            marker = fence_m.group(1)
            if not in_fence:
                in_fence = True
                fence_char = marker[0]
                fence_len = len(marker)
            elif marker[0] == fence_char and len(marker) >= fence_len:
                in_fence = False
            continue

        if in_fence:
            continue

        # Strip inline code spans so `[foo](bar)` inside backticks is ignored,
        # while `[`code`](./real.md)` still preserves `[](./real.md)`.
        cleaned = INLINE_CODE_RE.sub("", line)
        for match in MD_LINK_RE.finditer(cleaned):
            raw_target = match.group(1).strip()
            links.append((line_no, raw_target))

    return links


def normalize_link_target(raw_target: str) -> str | None:
    """Return local path portion of a Markdown link target, or None if external/anchor."""
    target = raw_target.strip()
    if not target:
        return None

    # Handle `<path with spaces>` syntax.
    if target.startswith("<"):
        end_angle = target.find(">")
        if end_angle != -1:
            target = target[1:end_angle].strip()
    else:
        # Strip optional title: `./path.md "title"` or `./path.md 'title'`
        target = target.split()[0]

    lower = target.lower()
    if not target or target.startswith("#") or lower.startswith(IGNORED_LINK_SCHEMES):
        return None

    # Strip #fragment and ?query.
    target = target.split("#", 1)[0].split("?", 1)[0]
    if not target:
        return None

    return urllib.parse.unquote(target)


def check_repo(repo_root: Path) -> tuple[list[str], dict[str, int]]:
    errors: list[str] = []
    stats = {
        "required_docs": len(REQUIRED_DOCS),
        "links_checked": 0,
        "cli_refs_checked": 0,
        "plugin_subcommands": 0,
        "repo_subcommands": 0,
    }

    # 1. Verify required docs exist and are non-empty.
    for rel_path in REQUIRED_DOCS:
        doc_path = repo_root / rel_path
        if not doc_path.is_file():
            errors.append(f"Missing required plugin doc: {rel_path}")
        elif not doc_path.read_text(encoding="utf-8").strip():
            errors.append(f"Required plugin doc is empty: {rel_path}")

    # 2. Verify packages/plugin-sdk/package.json includes README.md in `files`.
    pkg_json_path = repo_root / SDK_PACKAGE_JSON
    if not pkg_json_path.is_file():
        errors.append(f"Missing SDK package manifest: {SDK_PACKAGE_JSON}")
    else:
        try:
            pkg_data = json.loads(pkg_json_path.read_text(encoding="utf-8"))
        except json.JSONDecodeError as exc:
            errors.append(f"Invalid JSON in {SDK_PACKAGE_JSON}: {exc}")
        else:
            files = pkg_data.get("files")
            if not isinstance(files, list) or "README.md" not in files:
                errors.append(
                    f"{SDK_PACKAGE_JSON} `files` array must include \"README.md\""
                )

    website_docs_root = repo_root / WEBSITE_DOCS_DIR
    website_md_files = (
        sorted(
            p.relative_to(repo_root).as_posix()
            for p in website_docs_root.rglob("*.md")
        )
        if website_docs_root.is_dir()
        else []
    )

    # 3. Verify all relative local Markdown links resolve to existing files or directories.
    for rel_path in (*LINK_CHECK_FILES, *website_md_files):
        md_path = repo_root / rel_path
        if not md_path.is_file():
            if rel_path not in REQUIRED_DOCS:
                errors.append(f"Missing Markdown file for link check: {rel_path}")
            continue

        content = md_path.read_text(encoding="utf-8")
        for line_no, raw_target in extract_markdown_links(content):
            local_target = normalize_link_target(raw_target)
            if local_target is None:
                continue

            stats["links_checked"] += 1
            if local_target.startswith("/"):
                resolved = repo_root / local_target.lstrip("/")
            else:
                resolved = md_path.parent / local_target

            if not resolved.exists():
                errors.append(
                    f"Broken relative link in {rel_path}:{line_no}: "
                    f"({raw_target}) -> {resolved.relative_to(repo_root) if resolved.is_relative_to(repo_root) else resolved}"
                )

    # 4. Verify `xcpc plugin <subcmd>` mentions match real CLI subcommands.
    cli_path = repo_root / CLI_SOURCE_PATH
    if not cli_path.is_file():
        errors.append(f"Missing CLI source file: {CLI_SOURCE_PATH}")
    else:
        cli_source = cli_path.read_text(encoding="utf-8")
        plugin_subcommands = extract_clap_subcommands(cli_source, "PluginCommand")
        repo_subcommands = extract_clap_subcommands(cli_source, "PluginRepoCommand")
        stats["plugin_subcommands"] = len(plugin_subcommands)
        stats["repo_subcommands"] = len(repo_subcommands)

        if not plugin_subcommands:
            errors.append(
                f"Failed to extract PluginCommand subcommands from {CLI_SOURCE_PATH}"
            )
        if not repo_subcommands:
            errors.append(
                f"Failed to extract PluginRepoCommand subcommands from {CLI_SOURCE_PATH}"
            )

        valid_plugin_cmds = plugin_subcommands | {"help"}
        valid_repo_cmds = repo_subcommands | {"help"}

        for rel_path in (*CLI_CHECK_FILES, *website_md_files):
            md_path = repo_root / rel_path
            if not md_path.is_file():
                continue

            for line_no, line in enumerate(
                md_path.read_text(encoding="utf-8").splitlines(), start=1
            ):
                for match in CLI_PLUGIN_CMD_RE.finditer(line):
                    subcmd = match.group(1)
                    nested = match.group(2)
                    stats["cli_refs_checked"] += 1

                    if subcmd not in valid_plugin_cmds:
                        errors.append(
                            f"Unknown CLI command in {rel_path}:{line_no}: "
                            f"`xcpc plugin {subcmd}` (valid: {', '.join(sorted(plugin_subcommands))})"
                        )
                    elif subcmd == "repo" and nested and nested not in valid_repo_cmds:
                        errors.append(
                            f"Unknown CLI subcommand in {rel_path}:{line_no}: "
                            f"`xcpc plugin repo {nested}` (valid: {', '.join(sorted(repo_subcommands))})"
                        )

    return errors, stats


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Verify plugin documentation links, SDK packaging metadata, and CLI command references."
    )
    parser.add_argument(
        "--root",
        type=Path,
        default=Path(__file__).resolve().parent.parent,
        help="Repository root directory (defaults to parent of scripts/).",
    )
    args = parser.parse_args(argv)

    repo_root = args.root.resolve()
    errors, stats = check_repo(repo_root)

    if errors:
        for err in errors:
            sys.stderr.write(f"ERROR: {err}\n")
        sys.stderr.write(
            f"FAILED: plugin docs check found {len(errors)} error(s).\n"
        )
        return 1

    print(
        f"OK: plugin docs check passed "
        f"({stats['required_docs']} required docs, SDK package.json files, "
        f"{stats['links_checked']} local links, "
        f"{stats['cli_refs_checked']} CLI refs across "
        f"{stats['plugin_subcommands']} plugin + {stats['repo_subcommands']} repo subcommands)."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
