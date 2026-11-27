# Download & Installation

Download the latest release for your platform from [GitHub Releases](https://github.com/lihaoze123/xcpc-workstation/releases). All desktop packages include the `xcpc` CLI binary; if you only need the tool on a server or in a headless Agent environment, download the standalone CLI package.

## Platforms & Packages

| Platform | Recommended Artifact | Notes |
| :--- | :--- | :--- |
| **macOS (Apple Silicon / arm64)** | `xcpc-workstation-*-macos-arm64.dmg` | Drag-to-Applications disk image; bundles the `xcpc` CLI inside the `.app` and supports in-app auto-updates. |
| **macOS (Apple Silicon / arm64)** | `xcpc-workstation-*-macos-arm64.tar.gz` | Portable `.app` archive used for in-app auto-updates or manual extraction. |
| **Windows (x86_64)** | `xcpc-workstation-*-windows-x86_64-setup.exe` | Per-user Inno Setup installer; requires no administrator privileges, adds `xcpc` to user `PATH` by default (optional), and supports in-app auto-updates. |
| **Windows (x86_64)** | `xcpc-workstation-*-windows-x86_64.zip` | Portable archive; extract to any user-writable directory. Includes `XCPC Workstation.exe` and `xcpc.exe`, with in-app auto-update support. |
| **Debian / Ubuntu (amd64)** | `xcpc-workstation_*_amd64.deb` | System package; declares GTK4 and WebKitGTK 6.0 dependencies automatically and installs the desktop app plus `/usr/bin/xcpc`. |
| **Arch Linux (x86_64)** | `xcpc-workstation-*-x86_64.pkg.tar.zst` | Local `pacman` package; declares `gtk4` and `webkitgtk-6.0` dependencies automatically and installs the desktop app plus `/usr/bin/xcpc`. |
| **Linux Generic (x86_64)** | `xcpc-workstation-*-linux-x86_64.AppImage` / `.tar.gz` | Make the AppImage executable and run directly; `.tar.gz` runs after extraction and supports in-app auto-updates. |
| **CLI / MCP Only** | `xcpc-cli-*-{macos-arm64,linux-x86_64,windows-x86_64}.{tar.gz,zip}` | Contains only the headless `xcpc` binary with no GUI library dependencies; ideal for servers, CI, and MCP clients. |

---

## Platform Installation Steps

=== "macOS (Apple Silicon)"

    **Option 1: `.dmg` Disk Image (Recommended)**

    1. Download and open `xcpc-workstation-<version>-macos-arm64.dmg`.
    2. Drag `XCPC Workstation.app` into the `Applications` folder.
    3. To expose the bundled `xcpc` CLI on your `PATH`, create a symlink:

    ```bash
    ln -sf "/Applications/XCPC Workstation.app/Contents/MacOS/xcpc" /opt/homebrew/bin/xcpc
    xcpc --version
    ```

    **Option 2: Portable `.tar.gz` Archive**

    ```bash
    tar -xzf xcpc-workstation-*-macos-arm64.tar.gz -C /Applications
    ```

    !!! note "First-Launch Gatekeeper Notice"
        Unnotarized builds are ad-hoc signed. When downloaded via a browser, macOS attaches a quarantine attribute that may display `"XCPC Workstation.app" is damaged and can't be opened`. After copying the app to `/Applications`, clear the quarantine flag once:

        ```bash
        xattr -cr "/Applications/XCPC Workstation.app"
        ```

=== "Windows"

    **Option 1: Per-User Installer (Recommended)**

    1. Download and run `xcpc-workstation-<version>-windows-x86_64-setup.exe`.
    2. The installer defaults to a per-user installation (no UAC elevation required) so in-app auto-updates work smoothly.
    3. Adding `xcpc` to the user `PATH` environment variable is enabled by default (you may uncheck it during setup).
    4. Open a new terminal after installation to verify:

    ```powershell
    xcpc --version
    ```

    **Option 2: Portable ZIP Archive**

    1. Download `xcpc-workstation-<version>-windows-x86_64.zip` and extract it to any writable directory.
    2. Launch the desktop app via `XCPC Workstation.exe`, or invoke `xcpc.exe` from the same directory in a terminal.

=== "Debian / Ubuntu"

    Download `xcpc-workstation_<version>_amd64.deb` and install with `apt` (automatically resolves and installs missing `libgtk-4-1` and `libwebkitgtk-6.0-4` dependencies):

    ```bash
    sudo apt install ./xcpc-workstation_*_amd64.deb
    ```

    After installation:

    - Application files are located in `/opt/xcpc-workstation/`
    - CLI entry points are `/usr/bin/xcpc` and `/usr/bin/xcpc-workstation`
    - A desktop entry is registered in the application menu

=== "Arch Linux"

    Download `xcpc-workstation-<pkgver>-1-x86_64.pkg.tar.zst` and install with `pacman`:

    ```bash
    sudo pacman -U ./xcpc-workstation-*-x86_64.pkg.tar.zst
    ```

    The package declares `gtk4`, `webkitgtk-6.0`, and `glibc` dependencies. Once installed, launch the desktop app from your application menu or run `xcpc` directly in a terminal.

=== "Linux AppImage / .tar.gz"

    **AppImage:**

    ```bash
    chmod +x xcpc-workstation-*-linux-x86_64.AppImage
    ./xcpc-workstation-*-linux-x86_64.AppImage
    ```

    **Archive (`.tar.gz`):**

    ```bash
    tar -xzf xcpc-workstation-*-linux-x86_64.tar.gz
    cd "XCPC Workstation"
    ./XCPCWorkstation
    ```

    !!! warning "Linux Runtime Dependencies"
        The desktop app relies on native **GTK4** and **WebKitGTK 6.0**. `.deb` and `.pkg.tar.zst` packages install these automatically via your package manager. When using AppImage or `.tar.gz`, make sure the runtime libraries are installed:

        - **Debian / Ubuntu**: `sudo apt install libgtk-4-1 libwebkitgtk-6.0-4`
        - **Arch Linux**: `sudo pacman -S gtk4 webkitgtk-6.0`
        - **Fedora**: `sudo dnf install gtk4 webkitgtk6.0`

=== "CLI / MCP Only"

    If you only need the command-line tool or MCP server (for remote servers, CI pipelines, or terminal-only Agent environments), download the `xcpc-cli-*` archive and place `xcpc` (or `xcpc.exe`) on your `PATH`:

    ```bash
    tar -xzf xcpc-cli-*-linux-x86_64.tar.gz
    install -Dm755 xcpc ~/.local/bin/xcpc
    xcpc --version
    ```

## In-App Auto-Update

The desktop client includes built-in update checks, which can be triggered manually under **Settings → About**.

- **In-place auto-update supported**: macOS `.app` (installed from `.dmg` into `/Applications` or extracted from `.tar.gz`), Windows per-user installer (`setup.exe`), Windows portable `.zip` (extracted to a user-writable directory), and Linux `.tar.gz` (extracted to a user-writable directory).
- **System packages & read-only bundles**: Debian / Ubuntu `.deb` and Arch Linux `.pkg.tar.zst` install under `/opt/xcpc-workstation/`; upgrade by downloading the new package and installing via `apt` or `pacman`. Linux `AppImage` runs from a read-only mount; replace it with the new release from [GitHub Releases](https://github.com/lihaoze123/xcpc-workstation/releases).
- **Development builds**: Builds run from source have automatic updates disabled by default.

## Optional Environment Dependencies

- **C++ Compiler (for problem authoring & snippet verification)**: If you plan to use [Problem Authoring & Export](../problem-build/index.md) or [Three-Source Automated Verification](../templates-trd/verification.md), ensure `g++` or `clang++` is installed locally (configurable via the `XCPC_CXX` environment variable or **Settings → Toolchain** in the desktop app). On macOS, installing GNU GCC via `brew install gcc` is recommended for full `<bits/stdc++.h>` support; the desktop app automatically discovers versioned `g++-*` binaries in `/opt/homebrew/bin` and prefers them over Apple Clang. Pure statement authoring and PDF publishing do not require a C++ compiler.
- **CJK Fonts**: Release packages bundle Latin Modern and JetBrains Mono, and Chinese text automatically falls back to system fonts. To match the Founder font typography of the `olymp.sty` reference template, see [Font Configuration](./fonts.md).
