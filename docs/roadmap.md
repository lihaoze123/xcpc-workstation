# 路线图

## Phase 0：NSH + Rust Core 技术验证（PoC）

先验证风险最高的几个技术接缝，再开始铺功能：

1. **初始化 workspace**：搭建 Cargo + NSH 工作区，按 [architecture.md](./architecture.md#3-workspace-划分) 划好 crate 边界，并建立 `hero-nsh` 风格的三端桌面与 Web 脚手架。
2. **NSH + Typst Canvas 闭环**：用 Rust 编译一个最小的 `olymp-classic` Typst 页面，通过 `typst-render` 输出 HiDPI Pixmap，在 NSH Canvas 中流畅地缩放和平移。同时验证 `comemo` 增量编译的性能和输出清晰度。
3. **文本编辑与 IME**：验证 NSH 原生多行文本编辑能否正确处理中文 IME、撤销/重做和长文档。
4. **尽早接通 `xcpc mcp`**：在 Phase 1 初期就接好 `compile_and_diagnose` 和 `render_page_png`，以便自动把 `olymp-classic` 的渲染结果与真实的 `olymp.sty` PDF 做视觉比对。

## Phase 1：双层存储 + `olymp.sty` 出版核心 + 初版 CLI/MCP

- 开放目录工作区、`notify` 文件监听、SQLite FTS5 索引层；生成 JSON Schema 和工作区的 `AGENTS.md`。
- 集成 `tylax` + `mitex` + `pulldown-cmark` 转换管线；逐像素完成 `olymp-classic` 模板并建立视觉回归基准；打通整本题册、单题、题解册三种 PDF 导出。
- `xcpc mcp` 基础工具集：`compile_and_diagnose`、`render_page_png`、`export_pdf`、`search_problems`、`upsert_problem`。

## Phase 2：题库中心 + 活引用组卷 + 出版工作室 + Linter

- 题库中心：多维筛选、题解、代码、训练状态。
- 比赛组卷：活引用、局部覆写、难度均衡分析、一键冻结快照。
- 沉浸式出版工作室：结构化/源码双模编辑、双语并排校对与语义级 Diff、三层主题定制、`Cmd+K` 命令面板。
- `xcpc-lint` 题面体检与印刷前预检；补全 MCP 的组卷和体检工具，以及与桌面端的实时热联动。

## Phase 3：训练 Dashboard + OJ 生态 + 模板册 + 完整 MCP

- 训练看板（热力图、标签雷达、难度分布、VP 复盘、待补题队列）和训练类 MCP 工具。
- 6 平台外部 OJ 同步（CF / AtCoder / QOJ / 洛谷 / 牛客 / 力扣）。
- 代码模板册（TRD）+ KACTL 6 位哈希。
- Competitive Companion（`:10043`）本地收题；Polygon / DOMjudge / Hydro / FPS / QOJ 适配器；轻量 Git 集成；桌面端一键安装 MCP 配置；Web 端的 OPFS/ZIP 适配。

## Phase 4：性能压测、安装包与发布

- 用 1,000+ 题的本地题库和 20+ 题的超长题册做压测和优化。
- 输出 macOS / Windows / Linux 原生安装包（内含 `xcpc` CLI/MCP 二进制），以及 Web 轻量版。

## Phase 5（远期）：Hull 式声明式本地 Polygon

参考 [`rindag-devs/hull`](https://hull.aberter0x3f.top/)，在预留的 `problems/<id>/` 声明式 schema 上，用 Rust 原生的 WASI/WASM 确定性沙箱（`wasmtime`）实现跨平台、可复现的造数据、Subtask/Trait 校验、Checker 验证、多解法期望结果测试和对拍；兼容 Testlib / CPLib，支持与 Hull 工程互通，并扩展到 MCP 工具链。
