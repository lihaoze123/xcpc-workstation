---
title: Keyboard Shortcuts
---

# Desktop Keyboard Shortcuts

Press `Ctrl+/` (`⌘+/` on macOS) anywhere in the desktop app to open the searchable **Keyboard Shortcuts** overlay, or browse the complete list under **Settings → Shortcuts**.

> **Cross-platform modifier keys**: The tables below use Linux/Windows `Ctrl` notation. On macOS, shortcuts marked with `Ctrl` automatically map to `⌘` (Command), except for Vim's `Ctrl+[`, `Ctrl+r`, and the diagnostics window shortcut `Ctrl+Shift+D`.

---

## Global & Navigation

| Action | Shortcut | Scope & Notes |
| :--- | :--- | :--- |
| Command Palette | `Ctrl+K` | Global. Supports English, Chinese, and Pinyin search, ranked by frecency. |
| Keyboard Shortcuts | `Ctrl+/` | Global. |
| Go to Problems | `Ctrl+1` | Home navigation. |
| Go to Contests | `Ctrl+2` | Home navigation. |
| Switch to Snippets | `Ctrl+3` | Home navigation (hidden if `snippets = false` in `workspace.toml`, shifting subsequent spaces up). |
| Switch to Training | `Ctrl+4` | Home navigation (hidden if `training = false` in `workspace.toml`, shifting subsequent spaces up). |
| Switch to Tools | `Ctrl+5` | Home navigation. |
| Go to Settings | `Ctrl+6` | Home navigation. |
| Open Workspace | `Ctrl+O` | Global. |
| Close overlay or go back | `Esc` | Global. |

---

## Problems & Contests

| Action | Shortcut | Scope & Notes |
| :--- | :--- | :--- |
| New Problem | `Ctrl+N` | Problem list (writable workspace). |
| New Contest | `Ctrl+Shift+N` | Contest list (writable workspace). |
| Focus Search | `Ctrl+F` | Problem list. |
| Next item | `j` | Problems, Contests, and Snippets lists (when no input is focused). |
| Previous item | `k` | Problems, Contests, and Snippets lists (when no input is focused). |

---

## Publishing & Problem Authoring

| Action | Shortcut | Scope & Notes |
| :--- | :--- | :--- |
| Switch to Statement | `Alt+1` | Publishing Studio. |
| Switch to Samples | `Alt+2` | Publishing Studio. |
| Switch to Editorial | `Alt+3` | Publishing Studio. |
| Switch to Authoring | `Alt+4` | Problem Publishing Studio. |
| Verify problem data | `Ctrl+B` | Problem Authoring tab (toggles Markdown **Bold** inside the source editor). |
| Toggle Properties | `Ctrl+I` | Publishing Studio / Snippets & Team Reference Document (TRD) Studio (Properties drawer). |
| Show / Hide Preview | `Ctrl+\` | Publishing Studio (outside the full-screen Authoring console). |
| Focus Mode | `Ctrl+Shift+F` | Publishing Studio. |
| Save current file | `Ctrl+S` | Editor (in addition to 500 ms atomic auto-save). |
| Export PDF | `Ctrl+E` | Publishing Studio / Team Reference Document (TRD) Studio. |
| Find & Replace | `Ctrl+F` | Source Editor. |
| Heading 1–4 | `Ctrl+1` / `Ctrl+2` / `Ctrl+3` / `Ctrl+4` | Source Editor. |
| Bold | `Ctrl+B` | Source Editor. |
| Italic | `Ctrl+Shift+I` | Source Editor. |
| Inline Code | `` Ctrl+` `` | Source Editor. |
| Insert Inline Math | `Ctrl+M` | Source Editor. |
| Insert Display Math | `Ctrl+Shift+M` | Source Editor. |
| Bulleted List | `Ctrl+Shift+8` | Source Editor. |
| Numbered List | `Ctrl+Shift+7` | Source Editor. |
| Blockquote | `Ctrl+Shift+.` | Source Editor. |

---

## Preview

| Action | Shortcut / Gesture | Notes |
| :--- | :--- | :--- |
| Zoom In | `Ctrl++` | Preview canvas. |
| Zoom Out | `Ctrl+-` | Preview canvas. |
| Fit Width | `Ctrl+0` | Preview canvas. |
| Locate Preview from Caret | `Ctrl+Shift+L` | Forward sync from cursor to preview page. |
| Jump to Source from Preview | `Ctrl+Click` (macOS: `⌘+Click`) | Reverse sync to the corresponding source file and line. |

---

## Vim Mode (Practical Subset)

Vim Mode is off by default and can be toggled under **Settings → Appearance → Editor Typography → Vim Mode**. When enabled, the active mode badge (`NORMAL` / `INSERT` / `VISUAL` / `V-LINE`) appears on the right side of the editor toolbar.

| Action | Keys | Modes |
| :--- | :--- | :--- |
| Return to Normal mode | `Esc` / `Ctrl+[` | Insert / Visual |
| Move cursor & word motions | `h j k l` / `w b e` / `0 $` / `gg G` | Normal / Visual |
| Enter Insert mode | `i a I A o O` | Normal |
| Delete, yank & paste | `x` / `dd` / `yy` / `p P` | Normal |
| Undo & redo | `u` / `Ctrl+r` | Normal |
| Character / line visual selection | `v` / `V` (with `d` `y` `c`) | Normal |
| Inner text objects | `vi` / `ci` / `di` / `yi` + `w` `$` `()` `""` `''` ``` `` ``` | Normal / Visual |

---

## Developer Diagnostics

| Action | Shortcut | Notes |
| :--- | :--- | :--- |
| Open (or focus) standalone Diagnostics window | `Ctrl+Shift+D` | Opens or focuses the standalone `nsh-o11y` multi-window diagnostics panel (uses `Ctrl+Shift+D` on macOS as well); also accessible from the `Ctrl+K` Command Palette by searching for `Diagnostics`. |
