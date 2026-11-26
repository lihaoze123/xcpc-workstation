# Templates & Theme Customization

All publishing templates in XCPC Workstation are plain, directly editable `.typ` files inside your workspace or contest directory. Built-in templates act only as **preset seeds** copied into `templates/<name>.typ` when initializing a workspace or running `xcpc template add`; there are no hidden, unmodifiable privileged templates at runtime.

## Comparison of the Three Built-in Statement Templates

| Template ID | Style & Positioning | Key Layout Characteristics | Typical Template Options (`#let schema`) |
| :--- | :--- | :--- | :--- |
| **`olymp-classic`** | XCPC / ICPC Classic (Default) | Closely aligned with `lihaoze123/polygon-chinese-template` (`olymp.sty` + `ctexart` 12pt): A4 portrait, left-aligned problem header and two-column limits table, ruled header and `Page X of Y` footer, two-column sample tables (repeating header on page breaks), cover page and problem list page, clean A4 editorial booklet. | `cover`, `logo`, `show-metadata`, `blank-pages`, `body-size`, `margin-x`, `margin-y`, `cjk-body-font`, `cjk-heading-font`; per-problem `show-metadata`, `sample-layout` (two-column / wide). |
| **`cnoi-classic`** | CCF CSP-J/S & NOI Official Style | 2.4cm margins, official cover with source-file table / compiler-options table / notices, `【Section】` headings, Chinese dotted emphasis, line-numbered blue-bordered sample boxes, three-line constraint tables, `第 N 页 共 M 页` footer. | `cover`, `logo`, `compile-opts`, `show-submit-table`, `show-compile-table`, `notices`, `sample-border-color`, `sample-line-numbers`, `dotted-emphasis`, `blank-pages`; per-problem `prob-type`, `source-file`. |
| **`minimal`** | Minimal Blank Starter | ~90 lines of concise source preserving only the minimal page setup, fonts, and problem loop—ideal for building a custom university or organization layout from scratch. | `body-size`, `margin`, `show-cover`. |

!!! note "Template Resolution Precedence"
    When compiling a contest, the engine resolves the active template in the following order:
    1. **Contest-specific template**: `contests/<id>/template.typ` (if present, overrides the `template` field in `contest.toml`).
    2. **Workspace template**: `templates/<name>.typ` (from `contest.toml`'s `template`, falling back to `workspace.toml`'s `template`).
    3. If the target file does not exist, compilation fails with a missing-template error rather than silently falling back to an embedded copy.

## Declarative `#let schema` & Single-Entry Contract

A template file only needs to export `#let render(doc)` to take full control over cover pages, headers, footers, columns, and section layout. An optional `#let schema` dictionary declares configurable options for the desktop **Properties → Layout** panel, CLI, and MCP:

```typst
#let schema = (
  name: (zh: "我的校赛模板", en: "My Contest Template"),
  description: "Custom contest layout based on olymp-classic",
  groups: (
    (id: "cover", label: (zh: "封面设置", en: "Cover")),
  ),
  fields: (
    (
      key: "cover",
      label: (zh: "显示封面", en: "Show Cover"),
      type: "enum",
      scope: "book",           // book (whole booklet) or problem (per-problem)
      group: "cover",
      default: "auto",
      choices: (
        (value: "auto", label: "Auto"),
        (value: "on", label: "On"),
        (value: "off", label: "Off"),
      ),
    ),
    (
      key: "logo",
      label: "Cover Logo",
      type: "image",
      scope: "book",
      group: "cover",
      default: "",
      when: (cover: "on"),     // Simple equality visibility condition
    ),
  ),
)
```

`schema` supports 9 field types: `bool`, `enum`, `string`, `lines`, `number`, `length`, `color`, `font`, and `image`. User-facing labels accept either a plain string or a bilingual `(zh: "...", en: "...")` dictionary.

### Three-Level Parameter Scopes & Namespace Isolation

Template options are stored **namespaced by template ID** (contest-specific `contests/<id>/template.typ` uses the `local` namespace), so switching templates never erases previously configured options:

1. **Workspace defaults**: `[template-options.<template>]` in `workspace.toml`
2. **Contest overrides**: `[template-options.<template>]` in `contest.toml` (`scope: "book"`)
3. **Per-problem options**: `[template-options.<template>]` in `problem.toml`, and `[problem.options.<template>]` under `[[problem]]` in `contest.toml` (`scope: "problem"`)

## `templates/preamble.typ` & Offline Built-in Typst Packages

### Shared Custom Macros (`templates/preamble.typ`)

Create `templates/preamble.typ` in your workspace (a reserved filename that is not listed as a standalone contest template). Its definitions are injected automatically into scope before compiling any template or statement—ideal for workspace-wide math macros, color constants, or shared package imports:

```typst
// templates/preamble.typ
#import "@preview/cetz:0.5.3"
#let mod(m) = $space (op("mod") m)$
```

The desktop app watches `templates/preamble.typ` and hot-reloads previews on save. When finalizing a contest, `preamble.typ` is also copied into `snapshot/`.

### Offline Built-in Packages & Typst Universe

The engine embeds offline mirrors of 5 frequently used `@preview` drawing and annotation packages for zero-latency compilation in air-gapped contest venues or intranets:

- `@preview/cetz:0.5.3` (vector drawing)
- `@preview/oxifmt:1.0.0` (string formatting, dependency of `cetz`)
- `@preview/pinit:0.2.2` (relative-position annotations)
- `@preview/mannot:0.4.0` (math formula highlighting and annotations)
- `@preview/tiptoe:0.4.0` (arrow tips)

Other `@preview` packages are resolved via standard Typst paths (`TYPST_PACKAGE_PATH`, `TYPST_PACKAGE_CACHE_PATH`) and downloaded on demand when online. Set `XCPC_OFFLINE=1` to disable all network package downloads.

## Managing Templates with `xcpc template`

```bash
# List workspace templates and installable built-in presets
xcpc template list

# Inspect parsed #let schema fields and defaults for a template
xcpc template schema olymp-classic

# Copy a built-in preset into templates/<name>.typ
xcpc template add cnoi-classic
xcpc template add minimal my-school

# Fork an existing workspace template as a custom starting point
xcpc template fork olymp-classic icpc-regional-2026
```

## App Appearance Theme Customization (Global `theme.toml`)

Publishing templates control **PDF layout** (Git-tracked in the workspace), whereas desktop UI colors, corner radius, and editor typography are **personal global preferences** stored in `<dataDir>/theme.toml` (outside the workspace).

Click **Settings → Appearance → Open theme.toml** in the desktop app to generate and open the file; changes apply automatically on save without restarting:

```toml
# <dataDir>/theme.toml (all fields are optional)

[colors.light]
primary = "#0B6989"
background = "#FAFAFA"
card = "#FFFFFF"
sidebar = "#E4E4E7"

[colors.dark]
primary = "#0E7FA6"
background = "#09090B"
card = "#111113"
sidebar = "#141417"

[editor]
font_family = "JetBrains Mono"
font_size = 14
line_height = 22

[radius]
base = 8
```

- `[colors.light]` / `[colors.dark]` allow overriding 22 semantic color tokens (hex values), including `background`, `foreground`, `card`, `muted`, `border`, `primary`, `accent`, `secondary`, `sidebar`, `ring`, `destructive`, and more.
- `[editor]` configures `font_family`, `font_size` (11–22), and `line_height` (16–36).
- `[radius]` `base` sets the global base corner radius (0–20).
