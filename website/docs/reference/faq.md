---
title: 常见问题与排障
---

# 常见问题与排障

本页汇总安装运行、字体排版、题目构建与导出、算法模板验证、定稿保护与插件执行中的高频问题与排查步骤。反馈问题前，建议先在桌面端 **「设置 → 关于 → 复制诊断信息」** 复制环境摘要。

---

## 1. Linux 启动报错：缺少 GTK4 或 WebKitGTK 依赖

### 现象

在 Linux 上运行桌面端时提示找不到 `libgtk-4.so.1` 或 `libwebkitgtk-6.0.so.4`，或工具插件的 `<WebView>` 界面无法打开。

### 原因与解决

桌面端依赖系统原生 GTK4 与 WebKitGTK 6.0（用于纯 Web 插件沙箱）。请按发行版安装对应运行库：

- **Debian / Ubuntu**：
  ```bash
  sudo apt install libgtk-4-1 libwebkitgtk-6.0-4
  ```
- **Arch Linux**：
  ```bash
  sudo pacman -S gtk4 webkitgtk-6.0
  ```
- **Fedora**：
  ```bash
  sudo dnf install gtk4 webkitgtk6.0
  ```

若使用官方 `.deb` 或 Arch `.pkg.tar.zst` 安装包，包管理器会自动拉取上述依赖。

---

## 2. 中文题面字体未生效或出现方块字

### 现象

编译或预览中文题面时，汉字缺失、显示豆腐块，或未按预期使用方正/思源/系统衬线字体。

### 排查步骤

1. **检查引擎可见字体**：在终端运行以下命令查看排版引擎当前加载到的字体族列表：
   ```bash
   xcpc fonts
   # 如需同时扫描操作系统字体
   xcpc --system-fonts fonts
   ```
2. **了解字体加载优先级**：
   - Release 构建已内嵌 Latin Modern（西文/数学/行内等宽）与 JetBrains Mono（代码块），但**不包含中文字体**；
   - 运行时优先加载环境变量 `XCPC_FONT_DIRS` 指定的目录；未设置 `XCPC_FONT_DIRS` 时，自动扫描可执行文件旁的 `fonts/` 目录并回退至系统已安装字体（含中文字体回退链）。
3. **补充中文字体**：
   - 推荐在系统中安装思源宋体 / 思源黑体（如 Ubuntu/Debian 安装 `fonts-noto-cjk`，Arch 安装 `noto-fonts-cjk`），或将方正/自定义中文字体 `.ttf`/`.otf` 放入可执行文件同级的 `fonts/` 目录；
   - 若手动指定自定义字体目录，可设置 `XCPC_FONT_DIRS=/path/to/fonts1:/path/to/fonts2`（Windows 下用 `;` 分隔）。

---

## 3. 题目构建或模板验证提示未检出 C++ 编译器

### 现象

执行 `xcpc package build`、`xcpc snippet verify` 或在桌面端点击构建验证时报错：

```text
没有找到 C++ 编译器（g++ 或 clang++）。请安装后重试，或在「设置 → 工具链」中指定编译器路径。
```

### 解决方式

Core 按以下优先级自动定位 C++ 编译器：

1. 桌面端 **「设置 → 工具链」** 中手动指定的编译器绝对路径；
2. 环境变量 `XCPC_CXX`；
3. 系统 `PATH` 中的 `g++`（Windows 为 `g++.exe`），其次 `clang++`（`clang++.exe`）。

按平台安装建议：

- **Linux**：
  - Ubuntu / Debian：`sudo apt install g++`
  - Arch Linux：`sudo pacman -S gcc`
  - Fedora：`sudo dnf install gcc-c++`
- **Windows**：
  - 推荐安装 [MSYS2](https://www.msys2.org/)，在 UCRT64 终端执行 `pacman -S mingw-w64-ucrt-x86_64-gcc`，将 `C:\msys64\ucrt64\bin` 加入系统 `PATH`，或在 **「设置 → 工具链」** 中直接选择 `g++.exe`。

---

## 4. Library Checker 模板验证网络超时或离线环境使用

### 现象

运行包含 `kind = "library_checker"` 验证源的算法模板时，拉取 `yosupo06/library-checker-problems` 题面数据超时或失败。

### 解决方式

已拉取过的题目数据会自动缓存在本地，后续无需重复联网。针对首次拉取或完全离线环境，提供两种配置方式（在桌面端 **「设置 → 工具链」** 中配置 Library Checker 选项）：

1. **配置 GitHub Raw 镜像前缀（`mirror_url`）**：填入可用的 GitHub Raw 镜像前缀（例如 `https://ghproxy.net/https://raw.githubusercontent.com`），替代直连 `raw.githubusercontent.com`。
2. **指向本地仓库克隆（`local_repo_path`）**：预先将 `yosupo06/library-checker-problems` 克隆至本地磁盘，并配置本地仓库绝对路径，即可完全离线完成验证。

> 另注：若 Typst 排版时需要在离线环境禁止拉取 `@preview` 外部包，可设置环境变量 `XCPC_OFFLINE=1`（常用包 `cetz`、`pinit`、`mannot`、`oxifmt`、`tiptoe` 已内置离线回退包）。

---

## 5. 已定稿冻结的比赛或团队参考手册如何解冻？

### 安全规则说明

出于安全边界设计，**已定稿冻结（`frozen = true`）的比赛与团队参考手册（TRD）严格只读**，且**解冻操作仅开放给桌面 GUI 与 `xcpc` CLI（需二次确认），不向 MCP 工具开放**，避免 AI Agent 误改已交付归档。

### 解冻步骤

- **桌面端**：打开对应比赛或团队参考手册，在右上角或属性面板点击 **「重新编辑」**，确认弹窗中展示的快照与当前题库/算法库的差异列表后即可解除冻结（同时移除 `snapshot/`，恢复活引用跟随）。
- **CLI（比赛）**：
  ```bash
  # 交互式确认（会先展示快照与题库的差异）
  xcpc contest unfreeze <contest-id>

  # 脚本场景跳过交互确认
  xcpc contest unfreeze <contest-id> --yes
  ```
- **CLI（团队参考手册）**：
  ```bash
  # 交互式确认（会先展示快照与算法库的差异）
  xcpc trd unfreeze <trd-id>

  # 脚本场景跳过交互确认
  xcpc trd unfreeze <trd-id> --yes
  ```

> 若对象处于「已归档（`archived = true`）」状态而非冻结状态，请先在侧栏「已归档」区块或通过 `xcpc contest unarchive <id>` 取消归档。

---

## 6. 插件调用报错 `PLUGIN_UNTRUSTED` / `TIMEOUT` / `SYMLINK_ESCAPE`

### `PLUGIN_UNTRUSTED`

- **原因**：该外部插件声明了敏感权限（外网域名 `[permissions].domains`、凭据 `[[credentials]]` 或工作区只读 `workspace_read = true`），处于强制待信任状态（`pending_trust = true`）；或升级时扩大了权限范围/发生跨仓库覆写导致信任状态被自动重置。
- **解决**：在桌面端 **「设置 → 插件」** 中核对权限并点击信任，或在终端执行：
  ```bash
  xcpc plugin trust <plugin-id>
  ```

### `TIMEOUT` 或 `MEMORY_LIMIT`

- **原因**：插件在 QuickJS 沙箱中的单次执行触发了时间上限（`TIMEOUT`）或 64 MiB 内存上限（`MEMORY_LIMIT`）。
- **解决**：检查插件脚本是否存在死循环、超大数组分配或未分页的大规模计算；开发阶段可用 `xcpc plugin test <dir>` 复现并排查。

### `SYMLINK_ESCAPE`

- **原因**：通过 `xcpc plugin link <dir>` 链接的开发插件目录内部包含指向插件根目录之外的符号链接，被路径沙箱拦截。
- **解决**：移除插件目录内的外部符号链接，或使用 `@xcpc/plugin-sdk/vite` 将所有依赖打包为自包含的 `dist/` 产物后再链接 `dist/`。详见 [安全边界与信任门控](../plugins/security.md)。
