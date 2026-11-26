---
title: Home
hide:
  - navigation
  - toc
---

<div class="xcpc-hero" markdown>

<img class="xcpc-hero-logo" src="assets/logo.png" alt="XCPC Workstation" />

# XCPC Workstation

<p class="xcpc-hero-lead">
A local-first personal XCPC workstation built on an open-directory problem library — uniting Typst PDF publishing, problem authoring & strict verification, algorithm snippets & Team Reference Documents (TRD), a multi-platform training dashboard, a hybrid sandboxed plugin ecosystem, and native MCP / CLI support.
</p>

<div class="xcpc-hero-actions" markdown>
[Get Started](getting-started/index.md){ .md-button .md-button--primary }
[Installation](getting-started/installation.md){ .md-button }
[CLI & MCP Reference](cli-mcp/cli.md){ .md-button }
[GitHub Releases](https://github.com/lihaoze123/xcpc-workstation/releases){ .md-button }
</div>

![XCPC Workstation Publishing Studio: outline and difficulty stats on the left, Markdown/LaTeX/Typst editor in the center, and live olymp-classic preview on the right](assets/images/studio.png){ .xcpc-hero-shot }

</div>

---

## Core Capabilities

<div class="xcpc-grid">
  <a class="xcpc-card" href="publishing/workspace/">
    <div class="xcpc-card-title">Library & Publishing</div>
    <p class="xcpc-card-desc">Plain-text open-directory storage, live-reference contest assembly with frozen snapshots, and built-in <code>olymp-classic</code> / <code>cnoi-classic</code> / <code>minimal</code> declarative Typst templates with millisecond live preview and no TeX installation required.</p>
  </a>
  <a class="xcpc-card" href="problem-build/">
    <div class="xcpc-card-title">Problem Authoring & Export</div>
    <p class="xcpc-card-desc">Native <code>g++</code> / <code>clang++</code> with bundled <code>testlib.h</code> covering traditional, interactive, multi-run communication, and OI subtask problems — with strict verification gates, stress testing, ASan checks, and Polygon / DOMjudge / Hydro package exchange.</p>
  </a>
  <a class="xcpc-card" href="templates-trd/">
    <div class="xcpc-card-title">Algorithm Snippets & TRD</div>
    <p class="xcpc-card-desc">Open-directory management of code, notes, and verification scripts — with dependency DAGs, single-file C++ bundling (<code>xcpc bundle</code>), cumulative verification hashes, three-source verification, and <code>trd-compact</code> / <code>trd-book</code> templates.</p>
  </a>
  <a class="xcpc-card" href="training/">
    <div class="xcpc-card-title">Training Dashboard</div>
    <p class="xcpc-card-desc">Built-in incremental sync for Codeforces and AtCoder, merging external submissions with local logs at runtime without polluting problem files — featuring a rolling 12-month heatmap, rating curves, difficulty breakthrough trends, and editorial aggregation.</p>
  </a>
  <a class="xcpc-card" href="cli-mcp/cli/">
    <div class="xcpc-card-title">CLI & AI Agent</div>
    <p class="xcpc-card-desc">Desktop GUI, <code>xcpc</code> CLI, and <code>xcpc mcp</code> (stdio) share one Rust Core, supporting <code>--read-only</code> guards and three official Skills for end-to-end problem authoring, snippet curation, and plugin development.</p>
  </a>
  <a class="xcpc-card" href="plugins/">
    <div class="xcpc-card-title">Plugins</div>
    <p class="xcpc-card-desc">Core QuickJS sandbox plus a three-layer isolated <code>&lt;WebView&gt;</code> sandbox with Core-enforced <code>pending_trust</code> gating, paired with <code>@xcpc-workstation/plugin-sdk</code> and a hot-reloading CLI workflow.</p>
  </a>
</div>

---

## Community & Support

- **GitHub Repository**: [lihaoze123/xcpc-workstation](https://github.com/lihaoze123/xcpc-workstation)
- **Official Plugin Registry**: [lihaoze123/xcpc-plugins](https://github.com/lihaoze123/xcpc-plugins)
- **QQ User Group**: `1134941278`
- **Issue Tracker**: Open a [GitHub Issue](https://github.com/lihaoze123/xcpc-workstation/issues) — use **Settings → About → Copy Diagnostics** in the desktop app to attach environment details.
