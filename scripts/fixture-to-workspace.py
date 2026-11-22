#!/usr/bin/env python3
"""Turn an olymp fixture's reference input into the same content as a workspace.

    scripts/fixture-to-workspace.py tests/fixtures/olymp-ref/<case>

Reads <case>/contest.md and <case>/problems/*.md (the reference template's Pandoc
Markdown) and writes <case>/workspace/: problems/<case>-<label>/ with problem.toml,
statement.md and samples/, and contests/<case>/contest.toml. The regression
(scripts/olymp-regress.sh) builds that workspace with `xcpc` and sets it beside
reference.pdf, so the whole chain (workspace → Markdown → Typst) is what is compared.
"""
import re
import shutil
import sys
from pathlib import Path

case = Path(sys.argv[1])
name = case.name


def front_matter(text):
    m = re.match(r"---\n(.*?)\n---\n", text, re.S)
    meta, items, item = {}, [], None
    for line in m.group(1).splitlines():
        if re.match(r"\s+- ", line):
            item = {}
            items.append(item)
            line = line.replace("-", " ", 1)
        k, _, v = line.strip().partition(":")
        v = v.strip().strip('"')
        if line.startswith(" "):
            item[k] = v
        elif k != "problems":
            meta[k] = v
    return meta, items, text[m.end():]


def toml_str(s):
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def limit(value, kind):
    n, unit = re.match(r"([\d.]+)\s*(.*)", value).groups()
    if kind == "time":
        return f"{n}s" if unit in ("秒", "second", "seconds", "s") else value
    return f"{n}MB" if unit in ("MB", "megabytes", "megabyte") else value


IO = {"input", "output", "interaction"}
contest, items, _ = front_matter((case / "contest.md").read_text())
out = case / "workspace"
shutil.rmtree(out, ignore_errors=True)
(out / "contests" / name).mkdir(parents=True)
(out / "templates").mkdir(parents=True)
repo_root = Path(__file__).resolve().parent.parent
shutil.copyfile(
    repo_root / "crates/xcpc-engine/templates/olymp-classic.typ",
    out / "templates/olymp-classic.typ",
)
(out / "workspace.toml").write_text("")
toml = [f"name = {toml_str(contest['name'])}"]
for k in ("location", "date", "author", "language"):
    toml.append(f"{k} = {toml_str(contest[k])}")
toml.append(f"show-metadata = {contest.get('show-metadata', 'true')}")
for item in items:
    label = item["index"]
    meta, _, body = front_matter((case / item["source"]).read_text())
    pid = f"{name}-{label.lower()}"
    pdir = out / "problems" / pid
    (pdir / "samples").mkdir(parents=True)
    ptoml = [f"title = {toml_str(meta['title'])}", "", "[limits]"]
    ptoml.append(f"time = {toml_str(limit(meta['time-limit'], 'time'))}")
    ptoml.append(f"memory = {toml_str(limit(meta['memory-limit'], 'memory'))}")
    (pdir / "problem.toml").write_text("\n".join(ptoml) + "\n")
    toml += ["", "[[problem]]", f"id = {toml_str(pid)}", f"label = {toml_str(label)}"]
    if meta.get("show-metadata") == "false":
        toml.append("show-metadata = false")

    # Sections become second-level headings; the samples block goes to samples/, with
    # an explicit heading only where the automatic place (after the input and output)
    # is not where it is.
    lines, section, n = [], None, 0
    rest = body.splitlines()
    i = 0
    while i < len(rest):
        line = rest[i]
        h = re.match(r"# (.*?) \{#(\w+)\}$", line)
        if h:
            section = h.group(2)
            lines.append(f"## {h.group(1)}")
        elif line.startswith("::: {.samples}"):
            if section not in IO:
                lines += ["## 样例" if contest["language"] == "chinese" else "## Examples", ""]
            i += 1
            while not rest[i].startswith(":::"):
                kind = re.match(r"``` \{\.(input|output)\}", rest[i])
                if kind:
                    i += 1
                    block = []
                    while rest[i] != "```":
                        block.append(rest[i])
                        i += 1
                    while block and block[0] == "":
                        block.pop(0)
                    if kind.group(1) == "input":
                        n += 1
                    ext = "in" if kind.group(1) == "input" else "ans"
                    (pdir / "samples" / f"{n:02}.{ext}").write_text("\n".join(block) + "\n")
                i += 1
            while lines and lines[-1] == "":
                lines.pop()
        else:
            lines.append(line)
        i += 1
    text = "\n".join(lines).strip() + "\n"
    text = re.sub(r"\n{3,}", "\n\n", text)
    (pdir / "statement.md").write_text(text)
(out / "contests" / name / "contest.toml").write_text("\n".join(toml) + "\n")
print(f"wrote {out}")
