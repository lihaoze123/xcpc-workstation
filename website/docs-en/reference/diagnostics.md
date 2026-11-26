---
title: Developer Diagnostics & Accessibility
---

# Developer Diagnostics & Accessibility

XCPC Workstation keeps the main workspace free of technical noise by routing all live performance metrics, cross-thread traces, and structured logs into a dedicated multi-window diagnostics panel. The desktop app also provides full keyboard focus rings and a three-tier `prefers-reduced-motion` degradation policy.

---

## 1. Standalone Multi-Window `nsh-o11y` Diagnostics Panel

### Opening the Panel

- **Shortcut**: `Ctrl+Shift+D` (opens or focuses the standalone **XCPC Diagnostics** window);
- **Command Palette**: Press `Ctrl+K` and search for **Open Performance & Diagnostics Window** (matches keywords `o11y`, `diagnostics`, `perf`, `trace`);
- **Launch environment variable**: Start the desktop app with `XCPC_O11Y=1` to open the diagnostics window automatically on startup.

### Two-Tier Sampling & Zero Steady-State Overhead

The diagnostics subsystem operates in two tiers so everyday editing and previewing incur no overhead:

1. **Always-On Ring (enabled by default)**: **Zero `rAF` polling and zero disk writes.** Maintains an in-memory `LogStore` capped at 5,000 entries that captures slow IPC calls (`> 50 ms`), uncaught exceptions, and slow/error logs from Rust. Entries are fetched once when the diagnostics window opens.
2. **Live Diagnostics**: Activated while the diagnostics window is open (or launched with `XCPC_O11Y=1`). Setting `XCPC_TRACE=1` at launch additionally enables Rust and frontend trace recording. While open, the panel polls every `1000 ms` (1 second) and pushes samples into four synchronized time-series charts; closing the window stops polling immediately (and stops trace and frame-rate sampling unless `XCPC_TRACE=1` is set).

### Three Diagnostic Views

#### Metrics

Provides four time-series charts synchronized via `createChartSync()` (supporting `1m` / `5m` / `15m` time windows, drag-to-zoom, scroll-to-zoom, and double-click or **Reset Zoom** to reset all panels together):

1. **Frame Time & Jank**: Frame interval (`ms`, filtering idle gaps `> 120 ms`, clamping active spikes at `50 ms` while displaying the true peak in the top-right) and missed frame counts, with `120Hz (8.33 ms)` and `60Hz (16.67 ms)` reference lines.
2. **Typesetting & Preview Pipeline**: Discrete impulses for `build_document` (compilation) and `rasterize_cached` (rasterization) durations, showing the latest value and `P95` in the top-right with a `50 ms` slow-threshold line.
3. **IPC & Interaction Latency**: Discrete impulses for frontend↔Rust `IPC` calls and interaction latency `ux.inp.*` durations with `P95`.
4. **Cache & Memory**: Pixel cache footprint (`MB`, with a `48 MB` budget reference line) and cache hit rate (`%`). The desktop app automatically reclaims idle pixel caches, incremental typesetting caches, and idle plugin `<WebView>` instances when switching preview targets, leaving Studio, or closing the Diagnostics window to keep resident memory low.

#### Traces

- **Cross-thread Span waterfall**: Merges Rust Core thread spans and frontend UI thread spans into a unified span tree with a minimap, critical-path highlighting, and per-span attribute inspection.
- **Stable list & filtering**: Filter by **All** / `>50ms` / `INP` / **Error**. When traces are already listed, newly arriving traces queue behind a top banner (`N new trace(s) — click to load`) so rows never jump while you inspect them.
- **Named baselines & same-scenario diff**: Click **Save Baseline** to pin the current trace as a named baseline, then switch to **Compare** to diff span durations side by side before and after an optimization (with built-in scenario matching to prevent comparing unrelated workloads).

#### Logs

- Includes a log-volume histogram and a virtualized log list filterable across eight subsystems: `engine`, `convert`, `judge`, `plugin`, `sync`, `ipc`, `ui`, and `desktop`;
- Select any log entry and click **Copy Entry** to copy formatted JSON containing timestamp, level, subsystem, message, and attributes.

### Exporting Perfetto JSON

Click **Export Perfetto** in the diagnostics toolbar to save the unified Rust + frontend timeline as a Chrome Trace JSON file that can be dropped directly into [Perfetto UI](https://ui.perfetto.dev/) for offline flame-graph analysis. On desktop exit, workspace switch, or benchmarks (such as `preview_bench`), you can also write Rust-side trace events directly to disk via `XCPC_TRACE=1 XCPC_TRACE_OUT=trace.json`.

---

## 2. About Page: One-Click Diagnostics Summary & Font Licenses

When reporting bugs or troubleshooting environment issues, you do not need to gather system details manually:

1. Open **Settings → About**;
2. Click **Copy Diagnostics** to copy a standardized plain-text summary to the clipboard:
   ```text
   XCPC Workstation <version>
   Commit: <short-commit>
   Build Date: <build-date>
   Platform: <os> (<arch>)
   Fonts: UI=<ui-font-source>; Typesetting=<typesetting-font-source>
   ```
3. The same page lists **Open Source** acknowledgments and **Font Licenses** for the bundled fonts (Inter, JetBrains Mono, and Latin Modern).

---

## 3. Accessibility & Motion Degradation

### Full Keyboard Navigation & Focus Rings

- Every interactive control in the desktop app (buttons, list rows, segmented controls, menu items, tabs, and diagnostics controls) renders a high-contrast `FOCUS_RING` for full keyboard Tab navigation and activation.
- Truncated text uses `TruncText`, exposing the full string on hover or keyboard focus.

### Three-Tier `prefers-reduced-motion` Policy

Desktop motion is centralized in `tokens/motion.ts` (using damped physical springs by default with damping ratio $\zeta \in [0.872, 0.895]$ and overshoot $\le 1\%$) and responds live to the operating system's `prefers-reduced-motion` setting across three semantic tiers:

| Motion Category | Default Behavior | With `prefers-reduced-motion` Enabled |
| :--- | :--- | :--- |
| **Spatial translation & scaling** (studio transitions, side drawers, modal scale, primary button press scale, toast entrance slide) | Physical spring translation / `scale(0.96 → 1)` / `scale(0.97)` | **Degrades to pure opacity fade** (`120 ms`) or color-only feedback; all translation and scaling are removed. |
| **Indicators & layout reflow** (navigation/segmented sliders, chevron rotation, `animateLayout` list reordering, programmatic scroll) | Spring slide or smooth transition | **Instant snap** (`0 ms`, no animation) to reduce visual tracking strain. |
| **Color & opacity feedback** (hover/selected state tinting, backdrop fade) | `100–120 ms` standard easing | **Preserved** so interactive state changes remain clearly distinguishable. |

> For local motion debugging in development builds, set `XCPC_MOTION_SLOW=<factor>` (for example, `XCPC_MOTION_SLOW=4`) to scale all durations and springs proportionally while keeping the damping ratio $\zeta$ unchanged. This factor is locked to `1` in release builds.
