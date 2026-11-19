# 路线图

**原则：出版优先，每个阶段都要先打通一条可用的端到端链路。** 平台只考虑 Linux。

## Phase 0：技术验证（PoC）

1. **初始化 workspace**：Cargo workspace 加 `nsh init --template mixed` 生成的 `apps/desktop`，按 [architecture.md](./architecture.md#3-workspace-划分) 划好 crate 边界。
2. **NSH + Typst Canvas 闭环**：用 Rust 编译一个最小的 `olymp-classic` 页面，通过 `typst-render` 生成 HiDPI 位图，在 `nsh/base` 的 `Canvas` 中流畅缩放和平移；同时验证 `comemo` 增量编译的耗时。
3. **编辑器**：验证 `nsh/base` 的 `Editor`（或 `RichEditor`）在中文 IME、撤销/重做、长文档下的表现。
4. **基准 PDF 流水线**：用 `tectonic` 和固定 commit 的参考模板生成第一批 `tests/fixtures/olymp-ref/`，并跑通 PNG 比对。

## Phase 1：最短出版链路

- 工作区目录的读写，包括 `problem.toml`（含 `[[source]]`）、`statement.md`、`samples/`、`editorial.md`、`contest.toml`；原子写入、路径沙箱、冻结只读守卫。
- Markdown + Math → Typst（`pulldown-cmark` + `mitex`），包括按二级标题识别章节和生成样例表。
- `olymp-classic` 模板：题面页、封面和试题列表、A4 题解册，并与基准 PDF 做视觉回归。
- 导出单题、整本、题解册三种 PDF。
- 二进制 `xcpc`：`build` / `render` 等子命令（支持 `--json`），MCP 工具 `compile_and_diagnose`、`render_page_png`、`export_pdf`。
- 最小桌面窗口：打开一道题或一场比赛，左边是编辑器，右边是 Canvas 预览。

**明确推迟**：SQLite FTS5、`notify` 监听、LaTeX 输入、JSON Schema 和工作区 `AGENTS.md` 生成、`search_problems` / `upsert_problem`。

## Phase 2：题库中心 + 组卷 + 出版工作室 + Linter

- 引入 `notify` 监听、FTS5 索引、JSON Schema 和工作区 `AGENTS.md`。
- 题库中心：多维筛选，查看题解和代码，显示派生出的训练状态。
- 比赛组卷：活引用、元数据覆写、难度和页数统计、冻结与人工解冻。
- 完整的出版工作室：大纲、预览 HUD、`Cmd+K`。
- `xcpc-lint`；补齐 MCP 的题库、组卷和体检工具；GUI 实时联动。

## Phase 3：训练与生态

- Codeforces / AtCoder / QOJ 同步、训练看板、复盘与待补队列；训练类 MCP 工具。
- Polygon 导入（同时启用 `tylax` 处理 LaTeX 输入），以及 Competitive Companion 收题。

## 待定（尚未排期）

- 洛谷 / 牛客 / LeetCode 的 Cookie 同步。
- DOMjudge / Hydro / FPS / QOJ 包的导入导出。
- 代码模板册（TRD）。
- 双语并排校对与语义级 Diff。
- 更多主题预设和定制体系。
- 打包发布与 1,000+ 题规模的压测。
- Web 版。
- Phase 5：参考 Hull 的声明式本地 Polygon（`wasmtime` 沙箱）。
