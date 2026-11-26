---
title: FAQ & Troubleshooting
---

# FAQ & Troubleshooting

This page covers common issues and fixes across installation, CJK typesetting fonts, Problem Authoring & Export, Algorithm Snippet verification, finalization protection, and plugin execution. Before filing an issue, copy your environment summary from **Settings → About → Copy Diagnostics**.

---

## 1. Linux Launch Error: Missing GTK4 or WebKitGTK Libraries

### Symptom

Launching the desktop app on Linux fails with missing `libgtk-4.so.1` or `libwebkitgtk-6.0.so.4`, or `<WebView>` tool plugin interfaces fail to open.

### Cause & Fix

The desktop app relies on native GTK4 and WebKitGTK 6.0 (used for the pure-Web plugin sandbox). Install the runtime packages for your distribution:

- **Debian / Ubuntu**:
  ```bash
  sudo apt install libgtk-4-1 libwebkitgtk-6.0-4
  ```
- **Arch Linux**:
  ```bash
  sudo pacman -S gtk4 webkitgtk-6.0
  ```
- **Fedora**:
  ```bash
  sudo dnf install gtk4 webkitgtk6.0
  ```

Official `.deb` and Arch `.pkg.tar.zst` packages declare these dependencies automatically.

---

## 2. Chinese Statement Fonts Not Applied or Showing Missing-Glyph Boxes

### Symptom

Compiling or previewing Chinese statements shows missing glyphs (tofu boxes) or falls back to an unexpected typeface instead of FounderType / Noto Serif CJK / system serif fonts.

### Troubleshooting

1. **Inspect fonts visible to the engine**: Run the following command to list the font families currently loaded by the typesetting engine:
   ```bash
   xcpc fonts
   # Include operating system fonts in the scan
   xcpc --system-fonts fonts
   ```
2. **Understand font loading priority**:
   - Release builds embed Latin Modern (body, math, and inline monospace) and JetBrains Mono (code blocks), but **do not embed CJK fonts**;
   - At runtime, the engine loads directories from `XCPC_FONT_DIRS` first. When `XCPC_FONT_DIRS` is unset, it automatically scans a `fonts/` directory next to the executable and falls back to installed system fonts (on Windows, core CJK fonts are loaded on demand by default to keep memory low, automatically expanding to a full system font scan if a template references another installed font).
3. **Install or supply CJK fonts**:
   - Install Noto Serif/Sans CJK on your system (e.g. `fonts-noto-cjk` on Ubuntu/Debian or `noto-fonts-cjk` on Arch), or place `.ttf`/`.otf` CJK fonts into a `fonts/` directory next to the executable;
   - To specify custom font directories explicitly, set `XCPC_FONT_DIRS=/path/to/fonts1:/path/to/fonts2` (use `;` as the separator on Windows).

---

## 3. C++ Compiler Not Found During Problem Authoring or Snippet Verification

### Symptom

Running `xcpc package build`, `xcpc snippet verify`, or clicking **Verify** in the desktop app reports that no C++ compiler was found (`g++` or `clang++`).

### Fix

Core locates a C++ compiler in the following priority order:

1. An explicit absolute compiler path configured under **Settings → Toolchain** in the desktop app;
2. The `XCPC_CXX` environment variable;
3. `g++` (`g++.exe` on Windows) on `PATH`, followed by `clang++` (`clang++.exe`).

Platform installation recommendations:

- **Linux**:
  - Ubuntu / Debian: `sudo apt install g++`
  - Arch Linux: `sudo pacman -S gcc`
  - Fedora: `sudo dnf install gcc-c++`
- **Windows**:
  - Install [MSYS2](https://www.msys2.org/), run `pacman -S mingw-w64-ucrt-x86_64-gcc` inside the UCRT64 terminal, and either add `C:\msys64\ucrt64\bin` to `PATH` or select `g++.exe` directly in **Settings → Toolchain**.

---

## 4. Library Checker Verification Timeout or Offline Usage

### Symptom

Verifying an Algorithm Snippet with a `kind = "library_checker"` source times out or fails while fetching test data from `yosupo06/library-checker-problems`.

### Fix

Previously fetched problem data is cached locally and does not require network access again. For first-time fetches or fully offline environments, configure **Library Checker Source** under **Settings → Toolchain**:

1. **Mirror Prefix (`mirror_url`)**: Provide a GitHub Raw mirror prefix (for example, `https://ghproxy.net/https://raw.githubusercontent.com`) instead of connecting directly to `raw.githubusercontent.com`.
2. **Local Clone Directory (`local_repo_path`)**: Clone `yosupo06/library-checker-problems` to local disk and point to its absolute path for completely offline verification.

> Note: To prevent Typst from fetching external `@preview` packages when typesetting offline, set `XCPC_OFFLINE=1` (common packages `cetz`, `pinit`, `mannot`, `oxifmt`, and `tiptoe` are bundled with offline fallbacks).

---

## 5. How Do I Reopen a Finalized Contest or Team Reference Document (TRD)?

### Security Policy

By design, **finalized (`frozen = true`) contests and Team Reference Document (TRD) handbooks are strictly read-only**, and **unfreezing is only available through the desktop GUI and `xcpc` CLI (with explicit confirmation)—never through MCP tools**, preventing AI agents from accidentally altering finalized deliverables.

### Reopen Steps

- **Desktop app**: Open the finalized contest or Team Reference Document (TRD), click **Reopen** in the top-right bar or Properties panel, review the diff between the snapshot and the current library, and confirm (this removes `snapshot/` and restores live references).
- **CLI (Contest)**:
  ```bash
  # Interactive confirmation (shows diff between snapshot and problem library)
  xcpc contest unfreeze <contest-id>

  # Non-interactive confirmation for scripts
  xcpc contest unfreeze <contest-id> --yes
  ```
- **CLI (Team Reference Document)**:
  ```bash
  # Interactive confirmation (shows diff between snapshot and snippet library)
  xcpc trd unfreeze <trd-id>

  # Non-interactive confirmation for scripts
  xcpc trd unfreeze <trd-id> --yes
  ```

> If the item is **Archived** (`archived = true`) rather than finalized, unarchive it first from the sidebar's Archived section or via `xcpc contest unarchive <id>`.

---

## 6. Plugin Errors: `PLUGIN_UNTRUSTED` / `TIMEOUT` / `SYMLINK_ESCAPE`

### `PLUGIN_UNTRUSTED`

- **Cause**: An external plugin declares sensitive permissions (`[permissions].domains`, `[[credentials]]`, or `workspace_read = true`) and is waiting for approval (`pending_trust = true`), or an upgrade expanded its permissions / switched repository origin, automatically resetting trust.
- **Fix**: Review its permissions and approve it under **Settings → Plugins**, or run:
  ```bash
  xcpc plugin trust <plugin-id>
  ```

### `TIMEOUT` or `MEMORY_LIMIT`

- **Cause**: A single plugin invocation in the QuickJS sandbox exceeded its execution time limit (`TIMEOUT`) or the 64 MiB memory cap (`MEMORY_LIMIT`).
- **Fix**: Check for infinite loops, oversized allocations, or unpaginated heavy computation; reproduce and debug locally with `xcpc plugin test <dir>`.

### `SYMLINK_ESCAPE`

- **Cause**: A development plugin directory linked via `xcpc plugin link <dir>` contains a symlink pointing outside the plugin root directory and was blocked by the path sandbox.
- **Fix**: Remove external symlinks from the plugin directory, or bundle all dependencies into a self-contained `dist/` directory using `@xcpc-workstation/plugin-sdk/vite` and link `dist/` instead. See [Security Boundaries & Trust Gate](../plugins/security.md).
