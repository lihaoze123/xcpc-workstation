# Font Configuration

XCPC Workstation embeds Latin and monospace fonts so complete PDFs can be generated right after installation without a TeX environment. This page covers the bundled fonts, the CJK font fallback chain, how to configure Founder fonts to match the `olymp.sty` reference layout, and how to diagnose font loading with `xcpc fonts`.

## Built-in Fonts

Release binaries embed the following fonts directly; no operating-system installation is required:

| Role | Built-in Font | Usage |
| :--- | :--- | :--- |
| Latin Body / Headings | **Latin Modern Roman / Sans** (10pt / 12pt / 17pt) | Latin statement body text, section headings, headers and footers (matching XeLaTeX defaults) |
| Math Expressions | **Latin Modern Math** | Inline and display mathematical formulas |
| Inline Monospace & Samples | **Latin Modern Mono** (10pt / 12pt) | Sample input/output, filenames, inline code |
| Code Blocks | **JetBrains Mono** (Regular / Italic / Bold / BoldItalic) | Multi-line code blocks in editorials and documents, Team Reference Document (TRD) code listings |

## CJK Font Roles & Fallback Chain

Chinese typography matches fonts across four traditional typeface roles. After scanning font directories, the engine searches for each role in the following priority order:

| Role (Typst Symbol) | Primary Font (Reference) | Automatic Fallback Chain |
| :--- | :--- | :--- |
| **Songti Body** (`cjk-song`) | `FZShuSong-Z01` (Founder Shusong) | `Source Han Serif SC` → `Noto Serif CJK SC` → `SimSun` → `NSimSun` → `Songti SC` |
| **Heiti Headings / Bold** (`cjk-hei`) | `FZHei-B01` (Founder Heiti) | `Source Han Sans SC` → `Noto Sans CJK SC` → `SimHei` → `Microsoft YaHei` → `PingFang SC` |
| **Kaiti Author / Italic** (`cjk-kai`) | `FZKai-Z03` (Founder Kaiti) | `KaiTi` → `STKaiti` → `Kaiti SC` → `Source Han Serif SC` → `Noto Serif CJK SC` → `SimSun` |
| **Fangsong** (`cjk-fangsong`) | `FZFangSong-Z02` (Founder Fangsong) | `FangSong` → `STFangsong` → `Source Han Serif SC` → `Noto Serif CJK SC` → `SimSun` |

When the primary Founder fonts are unavailable and a fallback is used, typesetting still completes normally and the engine emits a non-fatal warning in the compilation diagnostics noting the font substitution.

## Font Loading Rules & Precedence

The engine determines its font search scope using two modes:

1. **`XCPC_FONT_DIRS` is not set (default for standalone packages)**:
   - Scans the **`fonts/` directory alongside the executable** first (if present).
   - Then layers the embedded Latin Modern and JetBrains Mono fonts with operating-system font fallback. On **Windows**, to keep cold startup fast and memory usage low, only core CJK system fonts (`SimSun`, `SimHei`, `KaiTi`, `FangSong`, etc.) are loaded on demand by default; if a custom template references another installed system font, the engine automatically expands the scan to all system fonts and retries transparently on first miss.
2. **`XCPC_FONT_DIRS` is set (deterministic typesetting mode)**:
   - Scans only the directories listed in `XCPC_FONT_DIRS` (`:`-separated on Linux, `;`-separated on Windows) plus the embedded fonts. **System fonts are excluded by default**, ensuring identical PDF output across machines and CI environments.
   - To include system fonts alongside `XCPC_FONT_DIRS`, pass the global CLI flag `--system-fonts`.

## Configuring Founder Fonts to Match `olymp.sty`

The visual reference for the `olymp-classic` template uses `ctexart` with `fontset=founder` (Founder Shusong, Heiti, Kaiti, and Fangsong). Due to font licensing restrictions, Founder font files are not bundled in release packages. If you need print-grade PDFs that match the Chinese `olymp.sty` reference template, obtain these four Founder fonts (`.ttf` / `.otf`) legally:

- `FZShuSong-Z01` (Founder Shusong)
- `FZHei-B01` (Founder Heiti)
- `FZKai-Z03` (Founder Kaiti)
- `FZFangSong-Z02` (Founder Fangsong)

Then configure them using either method below:

=== "Option 1: Place in the fonts/ Directory Next to the Executable (Recommended for Desktop)"

    Copy the font files into a `fonts/` folder alongside the executable and restart the app for automatic detection:

    - **Windows Installer / Portable**: `<install-dir>\fonts\` (next to `XCPC Workstation.exe` / `xcpc.exe`)
    - **Linux `.deb` / `.pkg.tar.zst`**: `/opt/xcpc-workstation/fonts/`
    - **Linux `.tar.gz` Extracted**: `<extract-dir>/fonts/`

    Alternatively, install the Founder fonts into your OS system font directory (`C:\Windows\Fonts` on Windows, or `~/.local/share/fonts/` followed by `fc-cache -fv` on Linux); they are discovered automatically whenever `XCPC_FONT_DIRS` is unset.

=== "Option 2: Set XCPC_FONT_DIRS (Recommended for CI / Isolated Environments)"

    Place the fonts in a dedicated directory (e.g., `~/fonts/founder`) and set the environment variable before launching the desktop app or CLI:

    ```bash
    # Linux / macOS
    export XCPC_FONT_DIRS="$HOME/fonts/founder"
    xcpc -w ./my-xcpc build contest welcome-round
    ```

    ```powershell
    # Windows PowerShell
    $env:XCPC_FONT_DIRS = "D:\fonts\founder"
    xcpc -w .\my-xcpc build contest welcome-round
    ```

    Join multiple directories with `:` on Linux or `;` on Windows.

## Diagnosing Fonts with `xcpc fonts`

Run `xcpc fonts` to list every font family currently visible to the typesetting engine:

```bash
# List visible font families in the current environment
xcpc fonts

# Output as JSON for scripting
xcpc fonts --json

# Check combined results with system fonts when XCPC_FONT_DIRS is set
xcpc --system-fonts fonts
```

Verify that the output includes `FZShuSong-Z01`, `FZHei-B01`, `FZKai-Z03`, and `FZFangSong-Z02` (or your intended Source Han / Noto CJK families). If a font does not appear, check that:

1. The file has a `.ttf`, `.otf`, `.ttc`, or `.otc` extension and is readable.
2. `XCPC_FONT_DIRS` is not accidentally set to an empty directory or overriding system font discovery.
3. Any font family name set in template options (such as `cjk-body-font` or `cjk-heading-font` in `olymp-classic`) matches the exact family name reported by `xcpc fonts`.
