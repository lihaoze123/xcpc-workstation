# 更新日志

所有值得注意的变更都会记录在本文件中。格式遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，版本号遵循 [语义化版本](https://semver.org/lang/zh-CN/)。

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
