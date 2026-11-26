# 下载与安装

前往 [GitHub Releases](https://github.com/lihaoze123/xcpc-workstation/releases) 下载对应平台的最新版本。所有桌面端安装包均同时附带 `xcpc` 命令行工具；如果仅在服务器或无头 Agent 环境中使用，可直接下载纯 CLI 包。

## 平台与安装包总览

| 平台 | 推荐产物 | 说明 |
| :--- | :--- | :--- |
| **Windows (x86_64)** | `xcpc-workstation-*-windows-x86_64-setup.exe` | Inno Setup 每用户安装器，免管理员权限，默认将 `xcpc` 加入用户 `PATH`（可选取消），支持应用内自动更新。 |
| **Windows (x86_64)** | `xcpc-workstation-*-windows-x86_64.zip` | 绿色便携包，解压至用户可写目录即用，内置 `XCPC Workstation.exe` 与 `xcpc.exe`，支持应用内自动更新。 |
| **Debian / Ubuntu (amd64)** | `xcpc-workstation_*_amd64.deb` | 系统级安装包，自动声明 GTK4 与 WebKitGTK 6.0 依赖，安装桌面端与 `/usr/bin/xcpc`。 |
| **Arch Linux (x86_64)** | `xcpc-workstation-*-x86_64.pkg.tar.zst` | `pacman` 本地包，自动声明 `gtk4` 与 `webkitgtk-6.0` 依赖，安装桌面端与 `/usr/bin/xcpc`。 |
| **Linux 通用 (x86_64)** | `xcpc-workstation-*-linux-x86_64.AppImage` / `.tar.gz` | AppImage 赋予执行权限即可运行；`.tar.gz` 解压后运行，支持应用内自动更新。 |
| **纯 CLI / MCP** | `xcpc-cli-*-{linux,windows}-x86_64.{tar.gz,zip}` | 仅包含无头 `xcpc` 二进制，无需图形库依赖，适合服务器、CI 与 MCP 客户端。 |

---

## 分平台安装步骤

=== "Windows"

    **方式一：每用户安装器（推荐）**

    1. 下载 `xcpc-workstation-<version>-windows-x86_64-setup.exe` 并运行。
    2. 安装器默认采用每用户安装（无需 UAC 管理员提权），以便后续使用应用内自动更新。
    3. 安装向导默认勾选将 `xcpc` 加入用户 `PATH` 环境变量（也可按需取消勾选）。
    4. 安装完成后打开新终端验证：

    ```powershell
    xcpc --version
    ```

    **方式二：绿色便携包**

    1. 下载 `xcpc-workstation-<version>-windows-x86_64.zip` 并解压到任意可写目录。
    2. 双击目录中的 `XCPC Workstation.exe` 启动桌面端，或在终端调用同目录下的 `xcpc.exe`。

=== "Debian / Ubuntu"

    下载 `xcpc-workstation_<version>_amd64.deb` 后使用 `apt` 安装（自动解析并安装缺失的 `libgtk-4-1` 与 `libwebkitgtk-6.0-4` 依赖）：

    ```bash
    sudo apt install ./xcpc-workstation_*_amd64.deb
    ```

    安装完成后：

    - 程序文件位于 `/opt/xcpc-workstation/`
    - 命令行入口为 `/usr/bin/xcpc` 与 `/usr/bin/xcpc-workstation`
    - 桌面快捷方式已自动注册到应用菜单

=== "Arch Linux"

    下载 `xcpc-workstation-<pkgver>-1-x86_64.pkg.tar.zst` 后使用 `pacman` 安装：

    ```bash
    sudo pacman -U ./xcpc-workstation-*-x86_64.pkg.tar.zst
    ```

    包内已声明 `gtk4`、`webkitgtk-6.0` 与 `glibc` 依赖，安装后即可在应用菜单启动桌面端，或在终端直接使用 `xcpc`。

=== "Linux AppImage / .tar.gz"

    **AppImage：**

    ```bash
    chmod +x xcpc-workstation-*-linux-x86_64.AppImage
    ./xcpc-workstation-*-linux-x86_64.AppImage
    ```

    **压缩包（`.tar.gz`）：**

    ```bash
    tar -xzf xcpc-workstation-*-linux-x86_64.tar.gz
    cd "XCPC Workstation"
    ./XCPCWorkstation
    ```

    !!! warning "Linux 运行时依赖"
        桌面端基于原生 **GTK4** 与 **WebKitGTK 6.0**。`.deb` 与 `.pkg.tar.zst` 会由包管理器自动安装依赖；使用 AppImage 或 `.tar.gz` 时，请确保系统已安装对应运行库：

        - **Debian / Ubuntu**：`sudo apt install libgtk-4-1 libwebkitgtk-6.0-4`
        - **Arch Linux**：`sudo pacman -S gtk4 webkitgtk-6.0`
        - **Fedora**：`sudo dnf install gtk4 webkitgtk6.0`

=== "纯 CLI / MCP"

    如果你只需要命令行工具或 MCP 服务（例如在远程服务器、CI 流水线或仅使用终端的 Agent 环境中），下载 `xcpc-cli-*` 压缩包并把 `xcpc`（或 `xcpc.exe`）放入 `PATH` 目录即可：

    ```bash
    tar -xzf xcpc-cli-*-linux-x86_64.tar.gz
    install -Dm755 xcpc ~/.local/bin/xcpc
    xcpc --version
    ```

## 应用内自动更新

桌面端内置自动更新检查功能，可在 **「设置 → 关于」** 中手动触发检查更新。

- **支持应用内就地更新的形态**：Windows 每用户安装器（`setup.exe`）、Windows 绿色便携包（`.zip` 用户可写目录解压版）与 Linux `.tar.gz` 用户可写目录解压版。
- **系统包与只读运行形态**：Debian / Ubuntu `.deb` 与 Arch Linux `.pkg.tar.zst` 安装在 `/opt/xcpc-workstation/` 下，更新时请下载新版本包并通过 `apt` 或 `pacman` 升级；Linux `AppImage` 运行在只读挂载环境，请从 [GitHub Releases](https://github.com/lihaoze123/xcpc-workstation/releases) 下载新版替换。
- **开发构建**：从源码运行的开发构建默认不启用自动更新。

## 可选环境依赖

- **C++ 编译器（用于题目构建与算法模板验证）**：如果需要使用 [题目构建与导出](../problem-build/index.md) 或 [三源自动验证](../templates-trd/verification.md)，请确保本机已安装 `g++` 或 `clang++`（可通过环境变量 `XCPC_CXX` 或桌面端 **「设置 → 工具链」** 指定路径）。纯题面编写与 PDF 出版不需要 C++ 编译器。
- **中文字体**：安装包已内置 Latin Modern 与 JetBrains Mono，中文默认自动回退到系统字体。若需要与 `olymp.sty` 基准一致的方正字体效果，请参阅 [字体配置](./fonts.md)。
