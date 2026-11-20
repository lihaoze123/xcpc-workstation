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
  - 分块光栅化：倍数不超过 1.5 时每页一张整页图；超过 1.5 时，底下是一张倍数 1.5 的整页图，上面盖 512px 的分块，只画视口内的部分和周围半块（`Compiled::render_region` 把页面帧平移后交给 `typst-render`，和整页光栅化逐像素一致）。分块按「其中绘制内容的指纹」缓存（`Compiled::region_fingerprints`：与分块相交的文字、形状、图片的哈希，包围盒取字形轮廓再放宽）。编辑一处只重画它碰到的分块，有「指纹相同则像素相同」的性质测试。只有需要的分块不在上次请求的范围里时才再请求。缩放时最慢一帧从约 80ms 降到约 21ms。
  - 编辑不再防抖：同一时间只有一个请求，期间的修改合并成下一个。改字到出图约 12ms，原来要先等 120ms 的防抖。
  - 长文档（release，样例的题目重复多遍）：5 页每次编辑增量编译 2ms，17 页 4–5ms，50 页 16–18ms，122 页 45–50ms，大致随页数线性增长（comemo 缓存了每段的排版，但分页和计数器仍要走一遍全文）。50 页以内在一两帧之内；更长的文档靠「同一时间一个请求」合并按键，暂不需要专用线程。编辑单题时预览的是单题，只有几页。
  - 闪烁的原因有两个：每次拿到结果都重建所有页面元素，新图片异步加载的那几帧是白页；换图时旧图先被丢掉。现在页面按序号保持（`<Each key="index">`），`PageImage` 在新图层全部加载完之前保留旧图层。
  - 平移的迟滞感：原来用 `onWheel` 在 JS 里改 `transform`，每次至少晚一帧，也没有惯性。现在竖向交给原生 `<Scroll>`（惯性和位移都在 UI 线程上），缩放改为按倍数布局。NSH 0.6 的 Linux 后端没有实现 `<Scroll>` 的缩放，所以 Ctrl + 滚轮和捏合仍由 JS 处理，横向平移（放大到比面板宽时）也还在 JS 里，没有惯性。
  - 复现：`cargo run --release -p xcpc-engine --example preview_bench`；应用内看日志里的 `[preview]` 行，帧开销用 `NSH_FPS=1`。
- 滚轮平移、Ctrl+滚轮 / 捏合缩放、中文输入法、撤销/重做和长文档均已人工验收通过（缩放采用手势期纯合成器 transform + 结束时单次提交，避免逐帧重排与滚动抖动）。
- 基准管线：3 套用例共 13 页，页数与基准一致，并排图看起来一样（`scripts/olymp-regress.sh`）。

## Phase 1：最短出版链路

- 工作区目录的读写，包括 `problem.toml`（含 `[[source]]`）、`statement.md`、`samples/`、`editorial.md`、`contest.toml`；原子写入、路径沙箱、冻结只读守卫。
- Markdown + Math → Typst（`pulldown-cmark` + `mitex`），包括按二级标题识别章节和生成样例表。
- `olymp-classic` 模板：题面页、封面和试题列表、A4 题解册，并与基准 PDF 做视觉回归。
- 导出单题、整本、题解册三种 PDF。
- 二进制 `xcpc`：`build` / `render` 等子命令（支持 `--json`），MCP 工具 `compile_and_diagnose`、`render_page_png`、`export_pdf`。
- 最小桌面窗口：打开一道题或一场比赛，左边是编辑器，右边是页面预览。

**进度**（2026-11-20）：以上各项都已有可用实现。

- Core：`xcpc-domain`、`xcpc-storage`（路径沙箱含符号链接、原子写入、冻结与只读守卫）、`xcpc-convert`（Markdown 转换和 `publish` 管线，诊断映射回 Markdown 行列，版心溢出检查）。冻结和解冻操作本身属于 Phase 2，这里只实现了守卫和从快照渲染。
- 视觉回归改为从用例的工作区走完整链路（`scripts/fixture-to-workspace.py`），3 套用例页数一致；与 Phase 0 手写 Typst 的输出逐像素相同。修正了中文模式 `⩽` 替换后丢失关系符间距的问题。
- 题解册、留白页、单题没有基准 PDF，只有单元测试和人工看图。
- 桌面端：选择题目或比赛，比赛内按题号切换编辑的文件并跳到对应页，题面 / 题解切换，编辑后 500ms 原子保存，诊断标在编辑器里，导出 PDF 到 `build/`。启动时打开 `XCPC_WORKSPACE` 或启动目录所在的工作区，都没有时显示「打开工作区」，用系统的文件夹选择器（nsh-core 的 `pick_paths`，只开放给 Rust）挑一个文件夹，它或它上方最近的带 `workspace.toml` 的目录就是工作区；顶栏的列表里也能换工作区。打开过的工作区会记在应用数据目录里，下次启动时（没有 `XCPC_WORKSPACE`、启动目录也不在工作区里）直接打开。

**明确推迟**：SQLite FTS5、`notify` 监听、LaTeX 输入、JSON Schema 和工作区 `AGENTS.md` 生成、`search_problems` / `upsert_problem`。

## Phase 2：组卷 + 出版工作室 + 题库中心

**原则：做减法。** 意义不明确的功能直接不做；尚未正式发布，不考虑迁移和兼容。以下是 2026-11-20 讨论确认的范围和顺序。

1. **修复现有桌面端隐患**（先做，后面的功能会放大它们）：
   1. 切题/切题解闪白：`apps/desktop/App.tsx` `choose()` 提前清空 `pages` 和 `zoom`，需改为 SWR 保留旧页直至新首帧到达，并按 `(target, id, book)` 记忆 `{ zoom, scrollY, panX }`。
   2. 滚动/缩放重复读盘与编译：`apps/desktop/src/lib.rs` `State::run` 在仅 `scale`/`view` 变化触发 `due()` 时未复用上次 `Compiled`，需缓存 `Compiled` 直进 `rasterize()`。
   3. 全文档指纹开销：`apps/desktop/src/lib.rs` 每次请求遍历全部页算 `page_fingerprint(i)`（122 页约 3ms），需改为仅对视口附近页按需懒计算。
   4. 像素缓存过小且无任务取消：`apps/desktop/src/lib.rs` `PIXELS` 仅保留最近 2 次结果，翻页或反复缩放易驱逐重画，需升级为字节预算 LRU（64–128MB，优先保 `BASE_SCALE` 整页层）+ `AtomicU64` 代数计数器取消过期瓦片任务。
   5. 缩放提交边角核验：核对 `apps/desktop/App.tsx` `commitZoom()` 在文档末尾附近放大时 `zoom.set` 与 `scrollTo` 是否稳定同帧提交。
2. **组卷与冻结**（Core + CLI + MCP）：
   - TOML 写回一律用 `toml_edit` 局部修改，保留注释、顺序和未知字段。Core 提供语义操作（增删题、移动、设置/删除覆写、比赛元数据、冻结），GUI 只走这些操作。
   - 题号由顺序决定，增删和移动后由 Core 重排为 A、B、C……；超过 26 题报错。
   - 组题统计：每题起止页取真实编译结果；难度分布（按 rating 段位着色的横条，无 rating 记为未定）和标签计数。**不做按条件抽题**，交给 Agent 组合 `search_problems` + `compose_contest`。
   - 冻结、解冻和删题规则见 [product.md](./product.md#3-比赛contest)。
3. **出版工作室**：
   - 自绘顶栏（`TitleBarStyle::Hidden` + `.window_drag(true)`）；大纲（含每题起始页码，`scrollTo({ animated: true })` 跳页）；编辑器与预览之间用 `<ResizablePanelGroup>`，比例记到应用数据目录。
   - 编辑区三个 Tab：**题面 | 样例 | 题解**。样例面板左右两栏编辑 `.in` / `.ans`，增删、拖拽排序（原子批量重命名 `01..NN`），粘贴时清理 BOM 和 `\r`。不做结构化分区编辑。
   - 右侧面板按上下文写入：比赛上下文里改标题、限制、`show-metadata` 一律写成覆写（细竖条标出，可还原，改回题库值时自动删除覆写）；单题上下文里改的是 `problem.toml`。冻结的比赛面板只读，只留解冻入口。
   - 预览 HUD：缩放组、页码组、Preflight 徽章（无问题时不显示）。`Ctrl+点击` 预览跳到源码对应行（只做预览 → 源码这一个方向）。
   - `Cmd+K`：跳转（题目、比赛、本比赛题号，Frecency 排序）和当前上下文可执行的动作（导出、冻结/解冻、新建题目/比赛、加入比赛、切 Tab、切换工作区）。新建题目时生成 `statement.md` 骨架和空 `samples/`。
4. **题库中心与实时联动**：
   - Core 内存索引（启动扫描 + `notify` 增量更新；CLI/MCP 每次启动现扫），结构化过滤 + 规范化子串匹配，`Cmd+K` 用模糊匹配。**不引入 SQLite / FTS5。**
   - 列表（ID、标题、rating、标签、来源、有无题解/代码）、多维筛选、详情（题面与题解预览、`solutions/` 只读查看）、在出版工作室打开、加入比赛。**不显示训练状态**（Phase 4 与同步一起做），不显示「题单」分段。
   - 外部修改只靠 `notify`（不做 IPC）。保存时比对内容哈希，磁盘已变则拒绝写入；编辑器无未保存改动时经 `EditorRef` 原地更新并保留光标和滚动，有未保存改动时显示一条冲突提示，二选一（载入外部版本 / 保留我的版本），不做合并。
   - 导航只有 3 个空间：`Cmd+1` 题库、`Cmd+2` 比赛（只有本地组卷，不显示分段）、`Cmd+3` 设置（工作区、最近打开的工作区、MCP 接入配置片段）。启动时回到上次所在的空间和对象。
5. **`xcpc-lint`、`xcpc init` 与 MCP**：
   - lint 只收会让题册出错的规则。error（阻止冻结）：引用的题目不存在、编译错误、配图不存在。warning：溢出版心、样例不成对或编号不连续、缺输入/输出章节（有交互章节除外）、导出题解册或冻结时有题缺题解。不做风格检查、规则开关和忽略注释。lint 结果并进 `xcpc check` 和 `compile_and_diagnose`，不单设命令和工具。
   - `xcpc init [dir]`：创建 `workspace.toml`、`problems/`、`contests/`、`.gitignore`（`build/`、`.xcpc/`）和一份内嵌的静态 `AGENTS.md`，之后不再覆盖。GUI 打开非工作区目录时可以在此新建。**不做 JSON Schema。**
   - MCP 工具见 [architecture.md](./architecture.md#72-mcp-tools)；CLI 对应 `xcpc search`、`xcpc show`、`xcpc contest compose|override|freeze|unfreeze`。
6. 全面落地 [design-system.md §8](./design-system.md#8-动效与流畅感fluidity) 的零抖动微交互规范。
7. **衔接 Phase 2.5 的 5 项既有控件细节与 3 处接缝**（2026-11-20 确认，避免 `desktop-ui` 返工，细则见 [design-system.md](./design-system.md)）：
   - 导出按钮直接做成按 PDF 新鲜度切换的分体按钮（与当前指纹一致时显示「在文件夹中显示」，过期时显示「导出 PDF」，重启按 `mtime` 恢复；`▾` 菜单固定 3 项；耗时 `> 150ms` 按钮内显示进度）；
   - 窗口创建即设背景色 `bg-background`（防 GTK 白底闪屏）、窗口标题显示当前对象（无工作区显示 `XCPC Workstation`）、记住窗口位置/大小/最大化与分栏比例（多屏断开时主屏居中）；
   - 被动层隐形引导：tooltip（悬停 `600ms`）与 `Cmd+K` 右侧显示快捷键、占位符淡色 `Ctrl K`、空态入口标快捷键；
   - 冻结态在比赛名旁显示锁形图标（锁扣约 `160ms` 合上，解冻反向打开淡出），面板淡为只读语气、只留解冻入口；
   - 新写的 GUI 错误提示按三段式与克制语气编写（规则拒绝用次级色陈述事实 + 入口）。
   - **为 Phase 2.5 预留 3 处接缝**：无工作区时显示的页面做成独立组件；应用数据目录预留快照与窗口状态存放位置；错误展示组件预留「复制详情」。

**不做**：结构化分区编辑、纸张反色、双页对开、HUD 自动淡出、嵌套横向 `<Scroll>` 惯性平移、编辑器 → 预览的行级跟随、按条件抽题、专门的 fork 功能、JSON Schema、SQLite 索引、本地 IPC。

**验收**：release 构建下人工走完以下剧本；Core 语义操作（`toml_edit` 往返保留注释、题号重排、冻结/解冻、冲突检测、lint 规则）有单元测试，9 个 MCP 工具有集成测试，GUI 的 `*.test.tsx` 只覆盖关键状态逻辑。

1. `xcpc init` 新建工作区；GUI 里用 `Cmd+K` 新建 4 道题，写题面、在样例面板加样例并拖拽排序、写题解。
2. 题库中心按标签和 rating 筛选，加入新建的比赛。
3. 出版工作室里拖拽调题序（题号重排）、给 B 题覆写时限再还原、用大纲和 HUD 跳页、`Ctrl+点击` 预览定位源码。
4. Agent 通过 MCP 搜索、`write_problem` 改题面、`compose_contest` 加题；GUI 无闪白、不丢光标地刷新；有未保存输入时出现冲突提示。
5. 删一张配图：HUD 徽章出现，冻结被拒绝；修复后冻结，面板只读，快照和两本 PDF 生成。
6. 改题库里某道题后解冻，差异列表正确。
7. 全程满足 §8 的流畅度铁律。

## Phase 2.5：打磨

2026-11-20 讨论确认，排在 Phase 2 之后、Phase 3 之前（界面与品牌规格见 [design-system.md](./design-system.md)，Core 工艺规格见 [architecture.md](./architecture.md)）。日常使用对熟手零打扰，只在首次体验与细节工艺上补齐质感。

1. **品牌页与首次体验**：
   - 无可打开的工作区时（首次启动或记住的工作区已失联）即显示品牌页；有工作区时冷启动直达上次对象。
   - 实现 `144px` icon、`26px`「**XCPC** Workstation」字标、三个 `240×36` 纵向入口（「新建工作区 / 打开已有文件夹 / 打开示例」）、固定失联错误槽、页脚署名（`v0.1.0　chumeng 用 ♥ 制作`，间距分隔不用 `·`），以及约 `1.9s` 的 5 段开场动效（第 1 帧起可点击并快进同时生效、遵守 `prefers-reduced-motion`、终帧 = icon、只动 `transform`/`opacity`）。不做独立启动页、轮播或 slogan。
2. **Icon 最终交付（v2）**：
   - 届时将参数化源码（`icon/src/gen.py` 等）纳入仓库（如 `assets/brand/`），按最终版 v2 规范（「`≥ 128` 有绳，`≤ 64` 一律无绳」，投影仅用 SVG 1.1 滤镜、**禁用 `feDropShadow`**，底板 `<use>` 复用）补齐全部交付物：单独绘制像素对齐的 `48 / 24px`、Linux hicolor 各档 PNG + scalable SVG + `-symbolic.svg`、Windows `.ico`（`16/20/24/32/40/48/256`）与动效用分层 SVG。
3. **内置示例工作区**：
   - 示例工作区打包进二进制，由 Core 提供释放到指定目录接口；CLI 新增 `xcpc init --example`，GUI「打开示例」释放后直达出版工作室。
   - 引导融入示例内容本身（如题面中提示修改实时更新、`Ctrl+点击` 预览跳回源码），按做题人视角写作，题解里藏一句作者的话；删除示例后引导随之消失。
4. **署名与「关于」页**：
   - 分层放置（品牌页页脚一行、「设置 → 关于」、`Cmd+K` 可搜「关于」），不做常驻底栏。
   - 「关于」页展示 icon 与字标、版本、commit、构建日期、许可证、致谢（`polygon-chinese-template`、Typst、NSH、MiTeX）、字体许可证（Latin Modern 为 GUST，JetBrains Mono 为 OFL，**方正字体授权条款待核实**），以及致谢末行献词「献给每一个和 WA 较劲到深夜的人」。
   - 点击版本号复制诊断信息（版本、commit、系统、字体来源）并短暂变为「已复制」；点击「关于」页 icon 重播品牌页动效，气球颜色从红 / 黄 / 绿 / 蓝 / 紫 5 色中随机。
5. **冷启动快照即时恢复**：
   - 预览稳定后节流写入「视口内 1–2 页像素 + `{target, id, book, zoom, scrollY}`」到应用数据目录。
   - 冷启动首帧即绘出快照，编辑器立即可打字；新首帧到达后原地替换并接管预览交互。快照仅在内容哈希一致时有效，否则丢弃。
6. **快捷键速查**：
   - 设置新增「快捷键」分组按场景列出；全局 `Ctrl+/` 呼出同一张表的浮层（`Esc` 关闭）；`Cmd+K` 可搜。只读，不做改键。
7. **Core 工艺升级**：
   - **PDF 元数据**：写入 `Title`（`春季训练赛 试题册` / `春季训练赛 题解` / `A. Two Sum`）、`Author`（比赛作者字段，缺失不写）、`Creator`（`XCPC Workstation <版本>`）、`Producer`（保留 Typst 默认），复用 `olymp-classic.typ` 已有书签（`bookmarked: true`）。
   - **导出默认文件名改为标题**：在 Core（`crates/xcpc-convert/src/publish.rs`）统一改为 `春季训练赛-试题册.pdf` / `春季训练赛-题解.pdf`（非法字符自动替换），GUI / CLI / MCP 一致，CLI `-o`/`--output` 与 MCP `output` 参数仍可覆盖；同步更新 `xcpc-mcp` 中断言 `build/a-plus-b.pdf` 的测试。
   - **Core 错误文案集中化**：三段式中文人类可读错误文案集中维护在 Core，供 GUI 与 CLI 共用；`--json` 与 MCP 保留结构化错误码。

**不做**：轮播引导、每次启动的启动页、slogan、常驻版本底栏、toast、行为触发的主动提示、改键、特定日期彩蛋、里程碑动效、icon 上的字母、ICPC 官方三色块组合。

**待核实**：方正字体授权条款（用于「关于」页字体许可证）。

## Phase 3：本地 Polygon（可复现的题目包）

规格见 [product.md §7](./product.md#7-本地-polygon可复现的题目包phase-3)（2026-11-20 讨论确认）。

1. **PoC 先行**，任何一项不过都回头重新讨论：
   1. wasi-sdk 编译 testlib 和常见选手代码（`bits/stdc++.h` shim、`__int128`、wasm 异常）；
   2. 同一个 wasm 在 x86_64 / aarch64、Linux / Windows 上的 tick 逐位相同；
   3. 影子栈设为内存上限后，10^6 层递归能跑通；
   4. fuel 计量相对原生的减速倍数（目标：生成 10^6 规模的数据在几秒之内）；
   5. 交互调度的确定性。
2. `Toolchain`（`xcpc.lock`、下载、校验、缓存）与 C/C++ `Language`。
3. 运行器：wasmtime + fuel tick、内存上限、testlib 协议的 checker / validator、stdio 交互。
4. `xcpc package build` / `stress`，三级内容哈希缓存，样例写回 `samples/`；lint 加入「标程超过时限 50%」和单独 `long` 的 warning。
5. Polygon 导入（同时启用 `tylax` 处理 LaTeX 输入）。
6. GUI 构建报告 Tab；MCP `build_package` / `stress_package`（`--read-only` 下不可用）。

**不做**：Nix / Hull 互通、子任务与部分分、CPLib 专门支持、文件交互 / 通信题 / 提交答案题、自定义 judger、单独的 compile / run 子命令、判外部提交、wasm64。

## Phase 4：训练与生态

- Codeforces / AtCoder / QOJ 同步、训练看板、复盘与待补队列；训练类 MCP 工具。
- Competitive Companion 收题。

## 待定（尚未排期）

- 方正字体授权条款核实（Phase 2.5「关于」页展示前确认）。
- 洛谷 / 牛客 / LeetCode 的 Cookie 同步。
- DOMjudge / Hydro / FPS / QOJ 包的导入导出。
- 代码模板册（TRD）。
- 双语并排校对与语义级 Diff。
- 更多主题预设和定制体系。
- 打包发布与 1,000+ 题规模的压测。
- Web 版。
- 选手开发调试环境：复用 `Toolchain` 层帮选手配好本机编译器、clangd、调试器（只保证版本一致，不在可复现契约之内）。
- 题目包的更多语言（Python 等，经 `Language` 层加入）。
- 预览性能的遗留项（2026-11-20 的优化之后，延迟已经感知不到）：
  - 100 页以上的文档每次编辑的增量编译要 45ms 以上：看 Typst 能否只从改动的那页往后重新分页，或者整本预览时只编译当前题（单题聚焦预览）。
  - 若以后要矢量预览：tinymist 的做法是 reflexo 矢量中间表示加 WGPU/Vello 绘制（`tinymist-viewer`），NSH 在 Linux 上的 `svg()`/`Painter` 是 UI 线程上的 Cairo 重放，不适合。

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
