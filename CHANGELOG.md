# 更新日志

所有值得注意的变更都会记录在本文件中。格式遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，版本号遵循 [语义化版本](https://semver.org/lang/zh-CN/)。

## [0.2.0-beta.1] - 2026-11-22

首次公开发布（Public Beta），涵盖完整 Phase 2（组卷、出版工作室、题库中心、校验与 MCP 工具集）与 Phase 2.5（产品化打磨、开放定制、实用子集 Vim 与高频写作工具）。

### 新增

- **组卷与冻结（Core / CLI / MCP）**：
  - 基于 `toml_edit` 实现保留注释与未知字段的语义级组卷操作（增删题、排序自动重排 `A–Z`、元数据覆写与还原、`xcpc contest compose|override|freeze|unfreeze`）。
  - 比赛冻结时将源题面与试题册/题解册 PDF 原子写入 `snapshot/` 并转为只读；解冻仅向 GUI/CLI 开放（不向 MCP 开放）并展示题库差异。
  - 新增 `xcpc-lint`（缺失题目/配图/编译错误阻止冻结；版心溢出、样例异常、缺章节/缺题解给出警告）与 `xcpc init [--example]`。
  - 完整 9 个 stdio MCP 工具（搜索、读写题面、组卷、覆写、冻结、编译诊断、页面渲染与 PDF 导出），支持 `--read-only`。
- **题库中心与出版工作室（Desktop）**：
  - 内存索引 + `notify` 文件监听实时联动，支持多维筛选、题解与代码预览、外部编辑无闪白原地刷新与未保存冲突二选一提示。
  - 出版工作室支持自绘顶栏、大纲页码跳转、可调分栏、**题面 / 样例 / 题解**三栏编辑、上下文右侧属性面板、预览 HUD 与 `Ctrl+点击` 预览定位源码。
  - `Cmd+K` 命令面板（Frecency 排序跳转 + 上下文动作）、按 PDF 新鲜度切换的分体导出按钮、冷启动预览快照即时恢复与 120Hz HiDPI 零闪白分块渲染。
- **产品化打磨与写作增强（Phase 2.5A）**：
  - 全新极简品牌 Icon（全尺寸 hicolor PNG/SVG、symbolic SVG、Windows `.ico`）与首启品牌引导页、内置示例工作区（「打开示例」/ `xcpc init --example`）。
  - 温润物理弹簧动效（`Animated.spring`）、分组式设置页（工作区 / 外观 / 快捷键 / 关于）、全局 `Ctrl+/` 快捷键速查与原生 `nsh/updater` 应用内更新。
  - `/` 斜杠命令菜单（公式、表格、图片、代码块及 XCPC 高频块）、居中轻量素材库 Gallery、```` ```typst ```` 原始块透传与专注写作模式。
  - PDF 写入完整元数据、默认导出文件名改为中文/标题名，并集中维护三段式中文错误文案。
- **开放定制、Vim 模式与高频工具（Phase 2.5B）**：
  - 开放定制三件套：`.xcpc/preamble.typ` 自定义 Typst 宏、`templates/*.typ` 自定义模板（附预置 `cnoi-classic` CNOI/OI 风格模板）、全局/工作区 `theme.toml` 外观与字体覆写。
  - 实用子集 Vim 模式（默认关闭，设置页一键开启；支持 `NORMAL / INSERT / VISUAL / V-LINE` 徽标、全角/半角自适应块状光标、`Esc` 与 `Ctrl+[`、常用移动/编辑与 `vi/ci/di/yi` 文本对象）。
  - 编辑器内悬浮迷你公式预览（复用 `xcpc-engine` 渲染，零打扰智能触发）与一键复制「带样例的完整 Markdown」。
- **多平台分发与 CI/CD**：
  - 新增 `packaging/` 打包工程与 GitHub Actions 发布流水线：支持 Windows x86_64（Inno Setup 每用户安装器 + 便携 `.zip`）与 Linux x86_64（`.deb`、Arch `.pkg.tar.zst`、`AppImage`、`.tar.gz`），桌面包均附带 `xcpc` CLI，并自动生成签名 `latest.json` 与 `SHA256SUMS.txt`。

### 变更

- 公开发布产物默认关闭 UPX 压缩，改由各封装格式自身压缩以消除 Windows 杀软误报并降低冷启动开销。
- 编辑器等宽字体栈补全跨平台等宽中文字体回退链（`Noto Sans Mono CJK SC` / `Source Han Sans HW SC` / `Sarasa Mono SC` / `NSimSun`）。

## [0.1.0] - 2026-11-20

### 新增

- **领域模型与存储层**（`xcpc-domain`、`xcpc-storage`）：
  - 定义 `Problem`、`Contest`（活引用与仅元数据覆写）、`Limits`、`SourceRef` 与工作区配置模型。
  - 实现基于 `workspace.toml` 的工作区发现、临时文件 + rename 原子写入、符号链接逃逸检测路径沙箱，以及冻结比赛只读守卫与快照读取。
- **格式转换与出版管线**（`xcpc-convert`）：
  - 基于 `pulldown-cmark`（GFM + Math）与 `mitex` 实现 Markdown + LaTeX 公式到 Typst 的胶水转换，支持按二级标题自动识别章节、从 `samples/` 组装样例表、映射题目 `assets/` 图片，以及中文关系符（`\le`/`\ge` → `⩽`/`⩾`）间距保留。
  - 实现 `publish` 管线与 `SourceMap`，将编译错误和版心溢出警告准确映射回工作区源文件行列。
- **排版引擎与 `olymp-classic` 模板**（`xcpc-engine`）：
  - 实现基于 `typst` + `comemo` 增量编译的原生 `Engine`，提供无 span 页面/区域指纹与分层局部光栅化支持。
  - 实现对齐 `polygon-chinese-template@10f9959` 的 `olymp-classic` 题面册模板（封面、试题列表、限制表、留白页、跨页重复表头超长样例表）及 A4 题解册模板（扉页、可跳转目录、书签、单色语法高亮代码块，中文粗斜体按 ctex founder 映射方正黑体/楷体）。
  - 在 release（及 `bundled-fonts` feature）构建中直接内嵌 Latin Modern 与 JetBrains Mono（24 个 OTF 文件），运行时按 `XCPC_FONT_DIRS` → 可执行文件旁 `fonts/`（未设 `XCPC_FONT_DIRS` 时）→ 内嵌字体 → 系统字体（含中文字体回退链）加载；其中 JetBrains Mono 专用于代码块，正常题面等宽字体保持与基准一致的 `Latin Modern Mono 12`。
- **构建与发布打包**：
  - 在 `.cargo/config.toml` 中集中配置 release 体积优化（`opt-level = "z"`、`lto = "fat"`、`codegen-units = 1`、`strip = true`、`panic = "abort"`）及 Windows 静态 CRT（`+crt-static`）。
  - 在 `flake.nix` 中接入 `cargo-xwin`、`lld`、`llvm`、`upx` 与 `x86_64-pc-windows-msvc` 目标标准库；新增 `scripts/package.sh` 统一打包 Linux/Windows 桌面端与 `xcpc` CLI 并用 UPX 压缩至 `target/dist/`。
- **CLI 与 MCP 服务**（`xcpc-mcp`）：
  - 提供 `xcpc build|check|render|list|fonts` 命令，支持 `--json` 机器可读输出、`--read-only` 与 `--system-fonts`。
  - 提供 `xcpc mcp` stdio JSON-RPC 服务，暴露 `compile_and_diagnose`、`render_page_png` 与 `export_pdf` 工具。
- **Linux 桌面端**（`apps/desktop`）：
  - 基于 NSH 0.6 构建题面/题解双栏编辑与实时预览应用，支持工作区选择器、记住上次打开工作区、比赛内按题号跳转、500ms 原子保存、编辑器诊断标记、PDF 导出、可见页分块光栅化与两阶段 GPU 合成器平滑缩放。
- **回归基准与文档**：
  - 建立 `contest-en`、`contest-zh`、`contest-zh-mixed` 三套 `tectonic` 基准回归用例及 `scripts/olymp-regress.sh` 自动化比对脚本，并补齐 `examples/workspace` 示例工作区与 `docs/` 全套规格文档。
