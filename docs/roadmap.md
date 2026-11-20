# 路线图

**原则：出版优先，每个阶段都要先打通一条可用的端到端链路。** 平台只考虑 Linux。

## Phase 0：技术验证（PoC）

1. **初始化 workspace**：Cargo workspace 加 `nsh init --template mixed` 生成的 `apps/desktop`，按 [architecture.md](./architecture.md#3-workspace-划分) 划好 crate 边界。
2. **NSH + Typst Canvas 闭环**：用 Rust 编译一个最小的 `olymp-classic` 页面，通过 `typst-render` 生成 HiDPI 位图，在 `nsh/base` 的 `Canvas` 中流畅缩放和平移；同时验证 `comemo` 增量编译的耗时。
3. **编辑器**：验证 `nsh/base` 的 `Editor`（或 `RichEditor`）在中文 IME、撤销/重做、长文档下的表现。
4. **基准 PDF 流水线**：用 `tectonic` 和固定 commit 的参考模板生成第一批 `tests/fixtures/olymp-ref/`，并跑通 PNG 比对。

**Phase 0 结论**（2026-11-20，Linux，开发构建）：

- 预览性能（后续实测修正了最初「编译约 25ms、光栅化 170–340ms」的结论）：
  - 最初的数字来自**未优化**的 Typst。`nsh` 为桌面宿主另外生成了一个 Cargo workspace（`apps/desktop/.nsh/rust`），根 `Cargo.toml` 里的 profile 对它不生效。现在依赖的 `opt-level = 2` 写在 `.cargo/config.toml`，两边都能读到。
  - 优化后，5 页样例每次编辑的增量编译约 2ms（应用内 4–6ms），单页光栅化（2.5 px/pt）约 2ms，单页 PNG 编码约 15ms。**真正的瓶颈是 PNG 编码和写文件，不是光栅化**。所以不需要「局部重排快路径 + 全局编译慢路径」的双轨方案：完整的增量编译本来就在一帧之内。
  - 页面指纹改为不含 span 的内容哈希（`xcpc-engine` 的 `page_fingerprint`），编辑一处只有所在页需要重新光栅化。
  - 页面以像素缓冲（`ImageSrc::rgba`）交给 Rust 组件 `PageImage` 显示，不再编码 PNG、写文件、再解码；平移和缩放是容器的 `transform`，由 GSK 合成，缩放时先拉伸旧图，再换成新图。应用内改一个字到出图的往返：PNG + `Canvas` 约 60ms，像素缓冲约 14ms；平移时每帧不再有 `Canvas` 的 Cairo 重放（原来每帧 4–7ms）。
  - SVG 和 `Painter` 两条矢量通路在 Linux 上都由 Cairo 在 UI 线程上按显示缩放重放成位图，不随 `transform` 缩放（读 `nsh-platform-linux` 的 `canvas.rs` 得出，未实测），比在后台线程光栅化更差，暂不采用。
  - 只光栅化可见页（前后各多一页）：`Preview.compile` 带上可见范围，范围外的页沿用已有的、倍数最接近的图，没有就先留白。实测（120Hz 屏）平移满帧 120fps，每帧约 2ms；编辑可见页外的内容时不重画任何页。
  - 分块光栅化：倍数不超过 3 时每页一张整页图；超过 3 时，底下是一张倍数 2 的整页图，上面盖 512px 的分块，只画视口内的部分和周围半块（`Compiled::render_region` 把页面帧平移后交给 `typst-render`，和整页光栅化逐像素一致）。只有需要的分块不在上次请求的范围里时才再请求；视图连续变化时按帧节流，编辑仍然防抖。缩放时最慢一帧从约 80ms 降到约 21ms。
  - 闪烁的原因有两个：每次拿到结果都重建所有页面元素，新图片异步加载的那几帧是白页；换图时旧图先被丢掉。现在页面按序号保持（`<Each key="index">`），`PageImage` 在新图层全部加载完之前保留旧图层。
  - 平移的迟滞感：原来用 `onWheel` 在 JS 里改 `transform`，每次至少晚一帧，也没有惯性。现在竖向交给原生 `<Scroll>`（惯性和位移都在 UI 线程上），缩放改为按倍数布局。NSH 0.6 的 Linux 后端没有实现 `<Scroll>` 的缩放，所以 Ctrl + 滚轮和捏合仍由 JS 处理，横向平移（放大到比面板宽时）也还在 JS 里，没有惯性。
  - 复现：`cargo run --release -p xcpc-engine --example preview_bench`；应用内看日志里的 `[preview]` 行，帧开销用 `NSH_FPS=1`。
- 滚轮平移正常。Ctrl+滚轮或捏合缩放、中文输入法、撤销/重做和长文档要人工验收。
- 基准管线：3 套用例共 13 页，页数与基准一致，并排图看起来一样（`scripts/olymp-regress.sh`）。

## Phase 1：最短出版链路

- 工作区目录的读写，包括 `problem.toml`（含 `[[source]]`）、`statement.md`、`samples/`、`editorial.md`、`contest.toml`；原子写入、路径沙箱、冻结只读守卫。
- Markdown + Math → Typst（`pulldown-cmark` + `mitex`），包括按二级标题识别章节和生成样例表。
- `olymp-classic` 模板：题面页、封面和试题列表、A4 题解册，并与基准 PDF 做视觉回归。
- 导出单题、整本、题解册三种 PDF。
- 二进制 `xcpc`：`build` / `render` 等子命令（支持 `--json`），MCP 工具 `compile_and_diagnose`、`render_page_png`、`export_pdf`。
- 最小桌面窗口：打开一道题或一场比赛，左边是编辑器，右边是页面预览。

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

## 备选路线：真 TeX + rtex（已验证，暂不采用）

[realtime-tex（rtex）](https://github.com/HenryXiaoYang/realtime-tex) 在常驻的 LuaLaTeX 进程里只重排正在编辑的段落，输出与 LuaLaTeX 逐点一致。2026-11-20 用 `contest-zh` 用例和 rtex v0.0.7 做了验证：

- 参考模板（ctexart + `fontset=founder` + olymp.sty）可以用 LuaLaTeX 编译，页数与基准一致。但 **LuaLaTeX 和 XeTeX 基准之间也差 1–6bp**，与 Typst 版的差距同一量级。采用这条路线时，基准要改为同一模板的 LuaLaTeX 输出。
- rtex 接管后，5 页的页面显示列表都与 PDF 一致。但 olymp 的 `problem` 环境把整道题包成一个未知环境，**可实时更新的段落为 0**。把正文移出 `problem` 后，中文段落、行内公式和列表都能实时更新：一次编辑约 1.4ms，字形与完整编译一致。`\InputFile` 这类章节宏仍然要等后台编译。
- 换页或行数变化要等后台完整编译，5 页约 2.1s；冷启动约 0.9s。依赖 TeX Live（这次试验的环境有 5 GiB，可以裁剪）。宿主要自己绘制字形坐标列表，其中 CM Type1 数学字体没有字形索引，需要额外处理。

暂不采用的原因：

- Typst 版已经达到「看起来一样」，对 10–30 页的比赛，整份增量编译约 25ms，速度已经够用。
- 切换要推翻三项已确认的决定：只用 Typst 生成 PDF、Markdown → Typst 的转换方式、引擎设计。题面结构也要改，不能再整道题包在 `problem` 里。
- rtex 还很早期，API 不稳定。

如果 Typst 逼近模板的维护成本明显上升（比如题解册、复杂版式），按以上数据重新评估。
