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

2026-11-21 讨论确认，排在 Phase 2 之后、Phase 3 之前，拆分为 **2.5A**（核心产品化、温润动效与写作体验）与 **2.5B**（开放定制、受约束的纸面直接操纵与高频小工具）。界面与品牌规格见 [design-system.md](./design-system.md)，Core 工艺规格见 [architecture.md](./architecture.md)。

### Phase 2.5A（核心产品化 + 温润感）

1. **Icon 最终交付（v3 极简版）与品牌页（1.53s）**：
   - 沿用 `/tmp/icon/` 的极简方向：Apple 风格哑光白到冷银连续超椭圆底板 + 向左大角度扇开的三张纯白/冷灰空白草稿纸（`-20° / -10° / 0°`）+ 扁平几何黄色铅笔（彻底去掉红气球、鸭青蓝元素与文字横线），微调构图与明暗对比后定稿纳入 `assets/brand/`（SVG 1.1 滤镜、禁用 `feDropShadow`、分尺寸像素对齐 `512/256/128/64/48/32/24/16` + Linux hicolor + `-symbolic.svg` + Windows `.ico` + 动效分层 SVG）。
   - 品牌页采用 **`1.53s`（0.75x 舒缓节奏）**：底板与纸叠淡入并扇开 → 铅笔滑入落定 → 字标、三个 `240×36` 纵向入口（「新建工作区 / 打开已有文件夹 / 打开示例」）与页脚依次错峰上浮（完全透明时不响应点击，可见后立即可点、任意输入快进、`prefers-reduced-motion` 直达终态）。
2. **温润物理动效 & 流畅度排查**：
   - 采用温润物理弹簧手感，用 NSH 原生平台线程 **`Animated.spring` / `Motion` / `spring(...)`**（阻尼比 `ζ = 0.86–0.90`，过冲 `≤ 1%`，自计时且全程可打断、保速度）替代干硬的线性 transition：Tab 指示条与同组相邻导航滑块平滑滑移（NavRail 进出「设置」改为 `120ms` 原地淡出/目标处淡入）、弹窗/菜单/补全浮层 `scale(0.94→1)` 温润浮出、页面/设置子页/进出工作室淡入微移、Inspector 抽屉阻尼、**仅纯色主按钮**按压 `scale(0.97)` 微回弹（描边/幽灵/图标按钮、导航项、菜单与列表行保持底色反馈）；同时排查高刷屏掉帧源头，高频操作在零滞后前提下补齐轻手感。
3. **分组式设置页、原生更新器与快捷键速查**：
   - 左侧分组（`工作区 / 外观 / 快捷键 / 关于`，选中滑块滑移）+ 右侧卡片区（切页淡入微移）：
     - **外观**：浅色 / 深色 / 跟随系统、编辑器字号/行高/连字；
     - **快捷键**：分组卡片 + 顶部搜索过滤 + 全局 `Ctrl+/` 浮层 + `Cmd+K` 可搜（只读，不做改键）；
     - **关于**：新 Icon（点击重播「扇开 → 铅笔落定」动效）、版本副标题（开发态 `v<版本>-dev (<7位commit>)`，发布态 `v<版本>`）、独立诊断信息卡片（「复制诊断信息」描边按钮）、许可证（`Apache-2.0 License`）、开源致谢（`olymp.sty`、Typst、NSH、MiTeX）与字体许可（Latin Modern、JetBrains Mono）、末行献词「献给每一个和 WA 较劲到深夜的人」、**基于 NSH 原生 `nsh/updater` 的检查更新与自动更新**（`checkForUpdate` / `downloadUpdate` / `installAndRestart` / `installOnQuit`，开发态体面提示）、重播首次引导入口。
4. **编辑器增强（第一批）**：
   - **`/` 斜杠命令菜单**（基于 `Editor` 原生 `caretBounds` / 补全浮层，模糊过滤 + 方向键回车）：行间公式、表格、插入图片、代码块，以及 XCPC 高频块（`cases` 分段函数、矩阵、子任务评分表、`## 提示`、`## 交互方式`、原始 Typst 块）。
   - **素材库（居中轻量 Gallery 弹窗）**：网格 ⇄ 放大两态（无右侧详情栏与工具条），入口位于属性抽屉「素材 N 个」、Slash「插入图片」与 `Cmd+K`；业务逻辑下沉至 Core（含缩略图缓存、基于 Markdown span 的精确引用定位与原子重命名改写），支持一键插入、重命名同步更新源码引用、批量清理未引用图片、放大态点击引用位置跳编辑器对应行及图片宽度预设（50/75/100%，供 2.5B 纸面手柄复用）。
   - **```` ```typst ```` 原始块透传**：直进 `olymp-classic` 作用域，支持 CeTZ/Fletcher 绘图与复杂版式，并保留源码映射。
   - **专注写作模式**：快捷键一键切换「专注模式」（折叠预览与大纲、编辑器居中列宽、开启打字机滚动），同时保留独立的「预览折叠」按钮。
5. **内置示例工作区**：
   - 示例工作区打包进二进制，由 Core 提供释放到指定目录接口；CLI 新增 `xcpc init --example`，GUI 品牌页「打开示例」释放后直达出版工作室。引导融入示例内容本身（含原始 Typst 块示例与做题人视角写作），删除示例后引导随之消失。
6. **冷启动快照即时恢复**：
   - 预览稳定后节流写入「视口内 1–2 页像素 + `{target, id, book, zoom, scrollY}`」到应用数据目录；冷启动首帧即绘出快照，编辑器立即可打字，新首帧到达后原地替换并接管交互（仅在内容哈希一致时有效）。
7. **Core 工艺升级**：
   - **PDF 元数据**：写入 `Title`、`Author`、`Creator`（`XCPC Workstation <版本>`）、`Producer`（保留 Typst 默认），复用 `olymp-classic.typ` 已有书签。
   - **导出默认文件名改为标题**：在 Core（`crates/xcpc-convert/src/publish.rs`）统一改为 `春季训练赛-试题册.pdf` / `春季训练赛-题解.pdf`（非法字符自动替换），GUI / CLI / MCP 一致，`-o`/`--output` 仍可覆盖。
   - **Core 错误文案集中化**：三段式中文人类可读错误文案集中维护在 Core，供 GUI 与 CLI 共用；`--json` 与 MCP 保留结构化错误码。

### Phase 2.5B（开放定制、Vim 模式、悬浮公式预览与导出复制）

**进度**（2026-11-22）：Phase 2.5A 与 2.5B 已全部完成并验收，进入 `v0.2.0-beta.1` 公开发布。

1. **开放定制三件套（文件驱动 + 极简入口）**：
   - **`templates/preamble.typ`**：工作区级自定义 Typst 宏与样式微调（随工作区进 Git），编译时自动注入模板作用域；
   - **自定义模板 `templates/*.typ`**：在 `workspace.toml` / `contest.toml` 指定 `template`，预置 `olymp-classic`，并参考 `Wallbreaker5th/fuzzy-cnoi-statement` 提供一个 CNOI/OI 风格示例模板与清晰接口；
   - **`theme.toml`**：支持用户全局 `<dataDir>/theme.toml` 覆写配色、字体与圆角（不设工作区级主题文件），在「设置 → 外观」提供「打开 `theme.toml`」入口。
2. **Vim 实用子集模式**：
   - 默认关闭，设置页一键开启；基于 NSH 0.6 原生 `captureKeys` 实现（无 `readOnly` hack）；
   - 编辑器顶部 38pt 栏右侧低调显示 `NORMAL / INSERT / VISUAL` 小徽标；`Insert` 下 `Esc` 回 `Normal`，`Normal` 下 `Esc` 走原有逐层关闭；
   - 支持 `hjkl / w / b / e / 0 / $ / gg / G`、`i / a / I / A / o / O`、`x / dd / yy / p / P / u / Ctrl+r`、`v / V`、`ci / di / yi` 配 `$ / ( / ) / " / ' / \``。
3. **编辑器内悬浮迷你公式预览**：
   - 智能零打扰触发：专注模式或预览折叠时，光标进入任意公式即就近浮出；双栏预览展开时，仅在块级公式 `$$...$$` 或含 LaTeX 命令的复杂行内公式停留 `250ms` 后浮出，复用 `xcpc-engine` 渲染（与 PDF 完全一致）。
4. **一键复制「带样例的完整 Markdown」**：
   - 走 `Cmd+K` 与右键菜单（零常驻按钮），在 Core 中将 `samples/` 组装为标准样例区块自动插入 `## 提示` / `## Note` 之前并复制到剪贴板，不修改源文件。

### Phase 2.5C（去预设特权、单入口模板契约与可视化版式定制）

2026-11-22 讨论确认，彻底去除了内置模板的「只读特权」与 Core/UI 写死的版式假设，让所有模板平等可编辑、可按工作区或单场比赛定制。规格见 [statement-style.md §2](./statement-style.md#2-模板与声明式定制)、[architecture.md](./architecture.md) 与 [design-system.md §9.1](./design-system.md#91-图标页面结构与组件规则2026-11-20-打磨)。

1. **预设降级为种子 + 比赛专属模板**：引擎只认磁盘上的 `.typ` 文件（`contests/<id>/template.typ` > `templates/<name>.typ`，无内置回退）；`olymp-classic`、`cnoi-classic`、`minimal` 作为预设种子在新建工作区或「从预设添加模板」时落盘到 `templates/`，全部可直接编辑。
2. **单入口 `#let render(doc)` 与全平权 `#let schema`**：模板统一导出 `#let render(doc)`（`section / samples / subsection / runin` 可选覆写、缺省走 `/xcpc/defaults.typ`）；`cover / blank-pages / show-metadata` 从 Core/UI 硬编码下沉到各模板 `#let schema`；schema 支持 9 种实用控件（新增 `image` 选图导入 `assets/`）。
3. **三级作用域与按模板名隔离持久化**：`workspace.toml` → `contest.toml` → `problem.toml` / `[[problem]]`（`[template-options.<template>]` 与 `[problem.options.<template>]`，比赛专属模板用 `local` 命名空间），统一走 `toml_edit` 局部写回；`notify` 监听 `workspace.toml`、`templates/*.typ`（含 `templates/preamble.typ`）与 `contests/*/template.typ`。
4. **「属性」抽屉双 Tab（`信息` | `版式`）与「设置 → 排版」**：「信息」只留纯内容元数据与语言；「版式」完全由当前模板 `schema` 动态渲染，支持一键「独立为本场专属模板」、直接打开任何模板文件编辑与从预设库添加模板。
5. **定稿完整归档与 CLI / MCP 同步**：`freeze_contest` 将生效的 `template.typ`、`templates/preamble.typ` 与合并参数固化进 `snapshot/`；CLI 新增 `xcpc template list|schema|add|fork`，MCP 新增 `list_templates`、`set_template_options` 并扩展 `set_override`。

**不做**：轮播引导、每次启动的启动页、slogan、常驻版本底栏、toast、行为触发的主动提示、改键、特定日期彩蛋、里程碑动效、icon 上的字母、ICPC 官方三色块组合、应用内 Typst IDE。

## Phase 3：本地 Polygon（原生题目包、严格验收与 Agent 协同）

详细规格见 [specs/local-polygon.md](./specs/local-polygon.md) 与 [product.md §7](./product.md#7-本地-polygon原生题目包与严格验收phase-3)（2026-11-23 调整确认，放弃 `wasi-sdk` / `wasmtime` fuel，改用本机原生 `g++` / `clang++` + 轻量进程监控）。

### Phase 3A：本地传统题（XCPC）完整闭环

1. **工具链与原生运行器（`crates/xcpc-judge`）**：
   - 自动探测本机 `g++` / `clang++`（支持 `XCPC_CXX` 与设置页路径覆写），固定 `-O2` 与后缀标准（默认 C++20），内置最新版 `testlib.h`（工作区自带优先），预留 `Language` 抽象层（多语言留待后续讨论）；
   - 轻量原生进程监控（Linux `rlimit` + `wait4 rusage` + wall-clock 强杀；Windows Job Object）；阶段差异化并发（编译/生成/Validator/Checker 多核并行，解法运行默认串行 `-j 1` 防计时失真）；
   - 编译 / 生成 / 运行三级内容哈希缓存（`~/.cache/xcpc/` 跨工作区共享），产物落盘 `build/problems/<slug>/`。
2. **完整传统题链路与双档门禁**：
   - `problem.toml` 支持 `[package].checker`（内置标准 Checker 家族 `wcmp`/`ncmp`/`fcmp`/`lcmp`/`rcmp4`/`rcmp6`/`rcmp9`/`yesno`/`nyesno` + 自定义 `checker.cpp`）、声明式 `[[test]]`（含 `repeat = N`、`tag`、`name`、`sample = true`）与 `[solutions.<name>]`（`main`、`expect`、`tags` 过滤）；
   - `samples/` 自动纳入前序测试点且不静默覆盖手工 `.ans`（天然兼容构造题）；自动检测手写数据 CRLF / UTF-8 BOM / 缺尾换行；
   - 目录约定自测：`tests/validator/{valid,invalid}/*.in` 与 `tests/checker/{ok,wa,pe}-*/{in,out,ans}`；
   - **双档门禁 + 动静分离**：默认开发态支持子集局部运行（Invocation），绝不签发 `verified`；`--strict` / MCP `verify` / 冻结 / 导出强制 Validator 自测（≥1 valid + ≥1 invalid）、自定义 Checker 自测（≥1 OK + ≥1 WA）、全量过检与标程 `≤ 50%` 时限（缺独立参考解/错解报 warning）；`xcpc check` 与冻结仅核对验收报告哈希，不暗中跑数据；
   - 报告含安全边际摘要与单步复现命令；CLI 提供 `xcpc package init|build|stress`。
3. **Agent / MCP 与官方出题 Skill**：
   - 5 个 MCP 工具：`inspect_package`、`inspect_test_case`（只读，`--read-only` 可用）、`init_package`、`build_package`（`verify`/`debug` 双模式）、`stress_package`；
   - 升级 `xcpc init` 内嵌的 `AGENTS.md`，并在仓库内建 `skills/xcpc-problem-creator/`（主 Skill + 3 个子 Skill + `mistake-taxonomy.md`，围绕 5 个 MCP 工具编写），`xcpc init --with-skills` 可一键装入工作区。
4. **桌面端轻量出题控制台与工具链设置**：
   - 出版工作室单题第 4 个 Tab「构建」（构建 / 严格验收 / 局部运行 / 测试×解法矩阵 / 点击单元格看诊断 / 一键脚手架 / 跳转外部编辑器；失败汇入 Preflight 徽章）；
   - 「设置 → 工具链」（检测编译器、路径覆写与缺失平台安装指引）。

### Phase 3B：生态数据包（`crates/xcpc-packages`）

2026-11-23 讨论确认，聚焦 Polygon ZIP 导入与 DOMjudge / Hydro 最小集合 ZIP 导出，提供 CLI、MCP 与桌面端轻量入口；stdio 交互题（`interactor.cpp`）与 OI 子任务 / 部分分延后至后续阶段。

1. **Polygon ZIP 导入**：
   - 同时支持单题（`problem.xml`）与整场比赛（`contest.xml`，展开全部子题并生成 `contest.toml`）。
   - **题面转换**：将 Polygon LaTeX 一次性转换为 `statement(.zh/.en).md`（Markdown + 原生 `$LaTeX$` 公式；复杂 LaTeX 环境经 `tylax` 转为 ```` ```typst ```` 原始块；配图迁入 `assets/`），保持 `.md` 为工作区单一事实源。
   - **题目包全量导入**：`samples/`、声明式 `[[test]]` 生成器（手写 / `from-file` 多测 / 缺源码时回退为静态 `tests/*.in`）、`validator.cpp`、`checker.cpp`（标准 `std::*` 映射到 `[package].checker`）、`solutions/*`（`main` / `expect`）以及 validator / checker 自测。
   - **冲突、兼容与安全**：目标 slug 冲突时直接拒绝（支持显式指定目标 `id`）；遇到交互题 / 非 C++ / 子任务时保留原始文件并输出结构化 `warnings`，不中止导入；内建 Zip Slip 路径穿越防护与原子暂存。
2. **DOMjudge / Hydro 最小集合 ZIP 导出**：
   - 支持单题 ZIP 与整场比赛批量导出（`A-<slug>.zip`、`B-<slug>.zip`……）。
   - **最小集合**：严格剔除 `validator.cpp`、`solutions/`、`generators/`、`editorial.md` 与 `tests/` 自测等内部文件。
   - **DOMjudge**（对齐 `cn-xcpc-tools/Polygon2DOMjudge`）：`domjudge-problem.ini`（比赛导出带 `short-name`，无气球颜色）、`problem.yaml`、`data/sample/`、`data/secret/`、`problem.pdf`（由 `xcpc-engine` 编译）、标准 Checker 映射至 `validation: default` + `validator_flags`、自定义 `checker.cpp` 打包进 `output_validators/`（附内置 `testlib.h` 与 Kattis 42/43 适配层）。
   - **Hydro**：`problem.yaml`、`additional_file/problem.pdf`（`xcpc-engine` 编译）与 `problem_zh.md` / `problem_en.md`（`@[pdf](file://problem.pdf?noDisposition=1)` 嵌入）、`testdata/config.yaml`、`testdata/*.in/.out`、自定义 `checker.cpp`（`checker_type: testlib`）。
   - **门禁（「动静分离」）**：核对最近一次严格构建报告（`verified == true` 且当前内容哈希未过期），直接打包 `build/problems/<slug>/` 已有数据，不暗中编译或跑数；未验收或已过期时拒绝导出，CLI `--force` 仅跳过门禁（仍要求已构建测试数据）。
3. **CLI、MCP 与轻量 GUI 入口**：
   - CLI：`xcpc import polygon <zip> [--id <slug>] [--json]`、`xcpc export <domjudge|hydro> <problem|contest> <id> [-o <path>] [--force] [--json]`；
   - MCP：`import_polygon`（`--read-only` 下禁止；明确允许只读打开任意本地绝对 `.zip` 路径，经 `.zip` 扩展名检查与 Zip Slip 防护，所有写入严格留在工作区内）与 `export_oj_package`（`format`、`target`、`id`，写入工作区 `build/`，遵守 `--read-only` 与路径沙箱）；
   - GUI：`Cmd+K` 新增导入/导出动作；题库与比赛顶栏新增「导入 Polygon」（调起原生文件选择器）；出版工作室导出分体按钮 `▾` 菜单在分割线下方追加「导出 DOMjudge 包」与「导出 Hydro 包」。

### Phase 3B+（Phase 3C 前置）：题库界面全面升级与命令面板优化

2026-11-23 讨论确认，排在 Phase 3B 之后、Phase 3C 之前，详细规格见 [specs/library-upgrade-and-command-palette.md](./specs/library-upgrade-and-command-palette.md)。围绕大规模题库（几十至数百题）的组织、批量运维、视觉密排与快捷检索做一次集中升级：

1. **持久化单层分组 + 动态视图分组（两者结合）**：
   - **数据模型**：`problem.toml` 新增可选字段 `group = "<名称>"`（不移动 `problems/<slug>/` 目录，不破坏比赛活引用，多人 Git 协同零冲突）；`workspace.toml` 新增可选 `groups = [...]` 维护分组顺序与空分组。Core 提供分组的创建、重命名、删除、排序与批量归组原子写回。
   - **Polygon 导入联动**：Polygon 整场导入（`contest.xml`）默认以比赛名自动创建并归入同名分组。
   - **UI 布局**：题库表格左侧新增可折叠「分组侧栏」（约 `180pt`，窄窗可收起），顶置「全部题目」「未分组」，下接自定义分组与题数，支持右键重命名/删除与新建分组；表格工具栏同时支持「按维度临时分组折叠」（无 / 分组 / 难度 / 来源 / 比赛 / 数据状态）。
2. **多选与四类批量操作**：
   - **多选交互**：表格最左侧增加勾选框列（表头全选当前筛选结果），支持 `Shift+点击` 连续区间选、`Ctrl+点击` 逐项增减、`Ctrl+A` 全选、`Esc` 退出多选；多选态右键任意选中行直接弹出批量菜单。
   - **操作入口与右侧面板联动**：选中 ≥2 题时，表格底部浮出紧凑操作条（已选 N 题 + 高频动作 + 取消），右侧面板同步切换为「批量摘要 / 任务面板」（选中概览、全量批量动作，以及批量构建/导出时的实时逐题进度与结果汇总，零弹窗打断）。
   - **批量操作范围**（全部下沉 Core，遵循原子写入与只读/引用守卫）：
     - 组织与元数据：批量加入比赛（遵守 26 题上限与非冻结/非归档守卫）、批量移入/移出分组、批量增删标签、批量设置难度；
     - 批量删除题目：二次确认，被未冻结比赛引用的题由 Core 拦截并逐题列出拒绝/跳过原因；
     - 批量构建 / 严格验收题目包：顺序执行并在右侧任务面板实时汇总通过/失败/耗时；
     - 批量导出：批量导出 DOMjudge / Hydro 包（遵循「动静分离」验收门禁）或合并导出为 PDF。
3. **题库表格视觉与排版重构**：
   - 修复 ID 列与来源列超长文本硬截断无省略号的问题（补齐 `flexShrink: 1, minWidth: 0`）；
   - 将「数据未验 / 数据过期 / 验收失败」从标题文字末尾彻底拆离，与「题解」「代码」合并为紧凑的「状态」图标列（悬停显示详情，减少视觉噪音并释放宽度）；
   - 精简高信息密度列结构（勾选框 | 标题 / 紧凑 ID | 难度 | 标签 | 状态 | 宽屏来源），优化表头排序指示器对齐与加侧栏后的响应式降级。
4. **`Ctrl+K` 命令面板配额与排序优化**：
   - 取消空查询下最多 30 条对象铺顶导致动作需长距离滚动的问题；
   - **空查询**：只展示「最近访问」最多 5 条对象（比赛工作室内保留本场题号），紧接着展示常用上下文操作；
   - **有输入查询**：按分组设上限（题目最多 8 条、比赛最多 4 条、操作不截断）；
   - 支持 **`>` 前缀**仅过滤并检索操作。
5. **CLI / MCP 最小闭环**：
   - `search_problems` / `write_problem` 支持 `group` 字段筛选与写入；
   - 补齐 `xcpc group` 与批量元数据/删除/导出操作的 Core 语义接口、CLI 与 MCP 入口。

### Phase 3C：交互题、stdio 多轮通信题与 OI 子任务 / 部分分

2026-11-24 讨论确认，详细规格见 [specs/phase-3c-interactive-communication-subtasks.md](./specs/phase-3c-interactive-communication-subtasks.md)。补齐竞赛出题核心高阶题型并打通本地构建、门禁、对拍、生态数据包与 GUI/MCP/Skill 全链路：

1. **stdio 交互题（`interactor.cpp`）**：
   - 对齐 Polygon 标准：默认由 `interactor.cpp` 直接判定；若显式配置 `[package].checker` 或存在 `checker.cpp`，则将 `interactor` 写出的 `tout` 再交给 `checker` 复判；
   - 双管道代理录制截断交互日志（失败时展示 `→`/`←` 对话），区分 CPU `TLE` 与死锁 `ILE`（wall-clock 超限但 CPU 未满）；`stress` 对拍同步支持交互题；
   - 交互题默认不把 `samples/*.in` 自动纳入评测（需测样例在 `[[test]]` 显式声明）；`--strict` / `verify` 仍强制 `validator` 自测，`interactor` 通过标程 + 错解验证（缺错解报 warning，不强制单独 `tests/interactor/` 目录）。
2. **stdio 多轮通信题（`runs = N`）**：
   - 对齐 Polygon `run-twice`（`<judging run-count="N">`），在 `[package]` 增加 `runs = N`（默认 `1`）；第 $k$ 轮 `tout` 作为第 $k+1$ 轮输入，支持「有 `interactor.cpp`」与「无 `interactor.cpp`（纯多轮串联 + `checker`）」两种形态，总 CPU 时间按各轮累计计入时限。
3. **OI 子任务 / 部分分（`[[subtask]]`）**：
   - 顶层 `[[subtask]]`（`id` / `score` / `depends` / `scoring`）+ 现有 `[[test]]` 新增可选单归属字段 `subtask = "<id>"`（数据复用靠 `depends`）；
   - 支持 `min`（默认捆绑）、`sum`（按点均分）与 testlib `quitp` 部分分比例；`[solutions.<name>]` 支持 `score`（整数或 `[min, max]` 区间）与 `subtasks = { "<id>" = ExpectVerdict }`；
   - 归属子任务的测试自动向 `validator` 传 `--group <id>`（向后兼容），自测支持可选 `tests/validator/{valid,invalid}/<subtask_id>/` 子目录；严格门禁对环依赖/悬空引用/空子任务/无归属非样例测试/标程未满分报 error，总分 ≠ 100 报 warning。
4. **生态包、题面 Lint、GUI 与 MCP / Skill 联动**：
   - Polygon 导入完整解析 `<interactor>`、`run-count` 与 `<groups>`/`<group>`；Hydro 导出原生支持 `interactive` 与 `subtasks` `config.yaml`；DOMjudge 导出原生支持 `custom interactive`（Kattis 适配），遇 OI 子任务平铺测试点并附 warning；
   - 保持 `statement.md` 单一事实源（`xcpc-lint` 补充缺 `## 交互方式` / `## 子任务` 的非阻断 warning）；桌面端「构建」Tab 补齐交互日志、子任务分组折叠矩阵与配置控件；同步升级 `xcpc package init --interactive --runs --subtasks`、MCP 工具与 `skills/xcpc-problem-creator/`。

### Phase 3D：整场验题与质量体检（Contest Readiness）

2026-11-24 讨论确认，详细规格见 [specs/phase-3d-contest-readiness.md](./specs/phase-3d-contest-readiness.md)。将单题验收升维至整场比赛定稿前的质量巡检与外部验题协作：

1. **Sanitizer（ASan / UBSan）UB 体检模式**：显式触发（`--sanitize` / MCP `mode: "sanitize"` / GUI 按钮），覆盖 `generator`、`validator`、`checker`、`interactor` 与所有预期 AC 解法；独立缓存零污染、自动放宽时限（如 `5× TL`），报告标出 UB / 越界诊断，不强制阻断 `--strict`。
2. **验题人外部提交试跑（零污染）**：支持单题单文件、整场单目录（按题号或 slug 自动匹配）与多验题人二级目录；临时目录编译运行不改写工作区文件，支持导出 Markdown 验题报告，并对满意解法提供一键「收编进 `solutions/`」。
3. **比赛级「验题体检」闭环**：比赛内新增「验题体检」视图 + `xcpc contest audit` CLI + `audit_contest` MCP 工具，聚合每题验收状态、标程耗时占比、AC/TLE 安全余量、数据体积与评测总耗时预警、错解覆盖、lint 与 Sanitizer 状态；支持一键验收过期题、整场 UB 体检、验题人目录试跑与导出整场 Markdown 报告。

### Phase 3 界面收敛：出题与比赛检查

2026-11-24 实机评审后讨论确认，排在 Phase 3D 之后、候选 Phase 3E 之前，详细规格见 [specs/authoring-ui-refactor.md](./specs/authoring-ui-refactor.md)（评审证据见 [review-phase3-ux.md](./review-phase3-ux.md)）。**不新增功能**，只重组结构、统一用词、补齐出错反馈：

1. **统一词表**：「检查」取代验证 / 验收 / 构建；题目状态只有「未检查 / 有问题 / 已通过」；「内存检查」「验题人代码」「加入解法」「可以定稿」；CLI 中文输出同步，命令名、MCP 与错误码不变。
2. **出错与环境缺失**：错误文案取自 Core `human_message`，执行类操作不再落到「操作没有完成」；没有编译器时在状态位提前说明并直达「设置 → 工具链」；失败原因写具体原因而不是检查项标题。
3. **出题面板（单题与比赛共用）**：标题栏管对象、面板工具行管题目动作；侧栏「题型」条目统一配置交互 / 轮数 / 计分并按题型显隐；矩阵颜色只表达「不符合预期」；诊断面板合并重复输出、交互日志按轮分组。
4. **比赛工作室**：取消独立的「验题体检」页；大纲显示每题状态，选中题目后就地「出题」与检查；「检查 ▾」收纳内存检查、试跑验题人代码、导出检查报告；批量进度原位显示；验题人代码结果拆回各题；比赛内检查使用比赛覆写后的时限与内存。
5. **视觉工艺**：颜色走 Token，字号收敛为 12 / 13 / 14，代码编辑器默认关闭连字，状态只用一种编码。

**不做**：新增检查维度、界面上的「验题人 × 题目」矩阵、改 CLI 命令名与 MCP 接口。

**不做**（Phase 3 整体）：`wasi-sdk` / `wasmtime` / `xcpc.lock` / wasm64、Nix / Hull 互通、CPLib 专门支持、函数式 `grader.cpp` 通信题 / 提交答案题 / 自定义 judger、应用内 C++ IDE。

## Phase 4：插件体系、个人训练看板与选手工具箱（已完成，v0.4.0-beta.1）

2026-11-24 对齐启动、2026-11-25 全部收尾（4A → 4B → 4C → 4D），按「**混合沙箱插件 + 纯 Web `<WebView>` 工具界面 + 数据看板驱动 + 只记状态与题解**」落地，详细规格见 [specs/phase-4-training-and-ecosystem.md](./specs/phase-4-training-and-ecosystem.md) 与 [specs/phase-4d-plugin-sdk-and-ecosystem.md](./specs/phase-4d-plugin-sdk-and-ecosystem.md)：

### Phase 4A：插件底座、官方 OJ 插件、训练状态与题库联动

1. **混合式沙箱插件引擎（`crates/xcpc-plugin`）**：
   - Rust Core 内嵌 QuickJS 沙箱运行目录包 `manifest.toml` + `index.js`（零构建步骤，GUI / CLI / MCP 共用），宿主提供域名白名单 HTTP、HTML 选择器、`keyring` 凭据托管（手动粘贴 + `verify()` 验证，不做登录 WebView）、可选 `workspace_read` 受控只读权限与声明式 UI Schema 渲染；协议开放 `oj_source`、`tool`、`commands`、`importer`/`exporter` 与 `credentials` 五类扩展点（4A 先落 `oj_source` 与 `credentials`）；
   - 官方 `codeforces` 与 `atcoder` 两个 `oj_source` 插件采用「内置默认 + `<dataDir>/plugins/<id>/` 本地同名覆盖」（每平台单个主 handle，AtCoder 仅同步 Algorithm Rating，含跨平台难度统一换算、提交/Rating 增量同步、近期比赛日程与 URL 解析）；支持本地目录/`.zip` 安装、去中心化 `index.json` 仓库订阅（SHA-256 校验）、`xcpc plugin init/test`（含 `xcpc-plugin.d.ts`）与桌面端热重载。
2. **全局同步与工作区隔离（`crates/xcpc-sync`）**：
   - 插件目录（`<dataDir>/plugins/`）、账号配置、凭据与同步缓存 `<dataDir>/oj_sync.db` 放用户全局目录，多工作区共享；
   - `training/logs.jsonl` 留在工作区进 Git，**只记状态变更**（`todo` / `attempted` / `ac` / `upsolved` / `clear`），思路与总结直接写进题目自己的 `editorial.md`；`workspace.toml` 支持 `training = false` 供多人出题仓隐藏训练入口与题库状态列。
3. **题库联动、应用内收题与导航**：
   - 运行时合并 `logs.jsonl` 与 `<dataDir>/oj_sync.db` 派生题目状态（不写进 `problem.toml`），题库支持筛选与右键/详情一键切换状态；支持按 URL 快速新建题目（走插件解析与本地缓存补全）；
   - 左侧导航升级为 **5 项**：`题库 (Cmd+1)` / `比赛 (Cmd+2)` / `训练 (Cmd+3)` / `工具 (Cmd+4)` / `设置 (Cmd+5)`；「设置 → 插件」统一管理插件安装/启停、OJ handle、凭据与同步；CLI 提供 `xcpc plugin`、`xcpc sync`、`xcpc training status`、`xcpc problem from-url`。

### Phase 4B：高密度训练看板、比赛日历、提交检索与训练 MCP

1. **「训练」空间三视图（`总览` | `比赛日历` | `提交`）**：
   - **总览**：顶部指标（各平台 Rating、周期过题/提交、Streak）+ 全局时间窗口（近 30 天 / 90 天 / 今年 / 全部）+ **双模式年度热力图**（每日题数 / 每日最高难度，跨年）+ 带段位色带的 Rating 曲线 + **有效难度突破趋势**（每周/月 AC 难度 P75 / P90）+ 难度分桶与支持按最低难度过滤的高难标签短板（点击钻取）；
   - **比赛日历**：聚合已启用 `oj_source` 插件的近期比赛，本地时间排期与倒计时，一键打开赛页；
   - **提交**：支持「按提交 / 按题目去重」切换（便于筛出 WA/TLE 未过题），点击「收入题库」静默入库（带 `title / rating / tags / [[source]]` 与空 `statement.md`）+ 右下角 Toast 提供「打开」。
2. **MCP 与 CLI**：精简 3 工具 `get_training_dashboard`、`set_training_status`、`ingest_problem` 及对应 CLI `--json`。

### Phase 4C：独立「工具」Tab（纯净插件容器 + 4 个官方预装工具插件 + `commands` & `importer`/`exporter`）

1. **纯净插件容器与双层运行时**（左侧导航 `工具 (Cmd+4)`，多人出题仓亦常驻可用，宿主零硬编码业务小工具）：
   - 无头 `index.js` 插件复用宿主文本/Markdown/表格/只读图视图/`svg`&`image`（`resvg` 直出预览与导出 PNG，支持 CodeSnap 类插件）与 CLI `xcpc tool list|run`；
   - 桌面 UI `ui.js` 插件采用 Obsidian / VS Code 信任模型直跑 NSH 渲染层（安装前显式提示非沙箱边界，自由度拉满）；
   - `commands` 挂载至 `Cmd+K`；`importer`/`exporter` 接入导入导出入口并由 Core 统一执行沙箱与门禁落盘。
2. **4 个官方内置默认工具插件**（4C 阶段先内置验证双层 Plugin SDK，4D 迁出至独立官方收录仓库并以新 SDK + Vite 重构）：
   - `graph-editor`（`ui.js` 标杆，对标 `another_graph_editor`：文本边表 ↔ 画布双向联动、拖拽与树/力导向/坐标系布局、着色/重标号/导出图片）；
   - `math-bitwise`（64 位按位翻转、进制转换、质因数分解、`gcd/exgcd`、模逆元/快速幂、CRT、常用模数速查）；
   - `complexity-mem`（运算量安全边界与数组精确 MB 估算）；
   - `string-lab`（KMP 前缀函数、Z 函数、Manacher、SA / LCP 表格对齐展示）。

### Phase 4D：Plugin SDK、纯 Web `<WebView>` 沙箱与官方收录仓库

2026-11-25 完成并验收，详细规格见 [specs/phase-4d-plugin-sdk-and-ecosystem.md](./specs/phase-4d-plugin-sdk-and-ecosystem.md)：

1. **纯 Web `<WebView>` 沙箱插件界面（`ui_entry = "ui.html"`）**：
   - 自定义界面插件由 NSH `<WebView incognito>` + 宿主受控外层壳 + `<iframe sandbox="allow-scripts" srcdoc>` 渲染，叠加 `capabilities.webview.hosts = []` 禁导航与严格 CSP（`default-src 'none'; connect-src 'none'`），通过随机令牌鉴权的 `window.xcpc` 仅提供受控宿主能力与可选 `--xcpc-*` 主题变量，不限制插件前端框架与样式；无界面表单工具保留原生 `HeadlessToolRunner`。
   - Core 层统一强制 `pending_trust` 信任门与 `source_repo` 来源隔离，仓库订阅强制 HTTPS（豁免本地回环地址）。
2. **仓内单一 SDK 包 `packages/plugin-sdk`（`@xcpc/plugin-sdk` v0.2.0）**：
   - 多入口导出 `/host`（QuickJS 沙箱契约）、`/ui`（`window.xcpc` 桥接类型）、`/vite`（单文件 IIFE `dist/ui.html` + `dist/index.js` 打包、QuickJS 禁用模块编译期护栏、`--watch` 联动）与 `manifest.schema.json`。
3. **按插件粒度热重载与 `xcpc plugin` 工具链**：
   - 完善 `xcpc plugin init [--kind tool|oj] [--vite]`、`install <path-or-id>`、`uninstall`、`trust`、`link` / `unlink`、`check`、`pack`、`test` 与 `repo list|add|remove|catalog`；桌面端按 `changedPluginIds` 粒度免重启热重载（兼容 Linux / Windows）。
4. **官方插件边界收敛与独立官方收录仓库（`xcpc-plugins`）**：
   - 主仓库仅保留 `codeforces`、`atcoder`（Rust 构建零 Node 依赖）；4 个官方工具迁出至独立官方仓库 `xcpc-plugins`（`math-bitwise` 与 `graph-editor` 为纯 Web `ui.html`），CI 使用 `XCPC_SKIP_EMBED_FONTS=1` 自动构建并原子发布 `official-latest` Release（含全部插件 zip、`index.json`、`registry.toml` 社区条目与 `xcpc-plugin-sdk.tgz`）。

**不做**（Phase 4 整体）：比赛复盘页（`training/reviews/*.md`）、独立待补队列页、耗时/VP/错因标签录入表单、单平台多账号合并、Competitive Companion 端口监听、题单（`sheets/*.toml`）、能力雷达图/平台环形图、盲盒抽题、间隔复习提醒、年度总结幻灯片、编译期响应式语法糖与独立 HMR 守护进程。

## Phase 5：Living TRD（活模板册）与可验证算法库（已完成，v0.5.0）

2026-11-26 深度对齐确认，按 **5A / 5B / 5C** 三步推进，详细规格见 [specs/phase-5-living-trd.md](./specs/phase-5-living-trd.md)：

### Phase 5A：算法库底座、通用单文件展开器与本地验板

1. **双层活引用底层（`snippets/<slug>/`）**：每条算法独立目录（`snippet.toml`、可选 `code.hpp`、`notes.md`、`test.cpp` 等），支持无代码的**纯公式/结论卡片**；通过扫描 `code.hpp` 中的 `#include "<slug>/code.hpp"` 自动构建依赖 DAG（外部编辑器 `-I snippets` 直接可用，无需 TOML 重复声明）。
2. **通用单文件展开器（C++ Bundler）**：支持展开任意本地 `.cpp` 赛时代码（同时解析工作区 `snippets/` 与相对路径 / `-I` 本地自定义头文件）或单个算法条目，自动拓扑内联、去重 `#pragma once` 与标准库头文件，提供 `xcpc bundle` CLI（支持 `--copy`）供编辑器一键绑定提交。
3. **考场累计分块校验哈希（KACTL++）**：印刷前自动隐藏 `#pragma once` 与本地 `#include`（改以依赖徽标标注），严格基于最终印出代码计算逐块 / 每隔 $N$ 行的去注释去空白 6 位累计 MD5 哈希，与考场零依赖 Linux 命令（`cpp -dD -P -fpreprocessed | tr -d '[:space:]' | md5sum | cut -c-6`）逐字节对齐。
4. **本地验板与依赖级联**：支持独立 `test.cpp` 自测与关联工作区 `problems/<slug>/` 验板；改底层板子自动标脏全部下游依赖并支持一键级联复验。

### Phase 5B：Library Checker 全局直验与 TRD 出版引擎

1. **Library Checker 全局按需缓存直验**：支持在 `snippet.toml` 中声明 `yosupo06/library-checker-problems` 题名，首次验板按需拉取几十 KB 轻量生成器与 `testlib` checker 至 `~/.cache/xcpc/library-checker/`（支持镜像/本地克隆路径配置，缓存后离线复用），直接由 `xcpc-judge` 原生造数跑测，零污染工作区 Git。
2. **模板册活引用组册与冻结（`trds/<id>/`）**：按章节活引用算法条目，支持多本册子、缺失上游依赖 Preflight 检查、柔性冻结门禁（默认对未验/过期/失败报 warning，提供可选 `--strict` 严格阻断）与快照归档。
3. **平权 Typst 模板种子**：复用 Phase 2.5C 的 `#let render(doc)` + `#let schema` 契约，提供 `trd-compact`（横版 A4 三栏、灰度印刷 + 「行号 哈希」边栏）与 `trd-book`（竖版 A4 单/双栏讲义风格）；内置示例工作区预置 3–5 条示范条目与 1 本示例模板册。

### Phase 5C：桌面端「模板」空间、题解双向联动与 MCP / Skill

1. **桌面端「模板」一级导航**：左侧导航升级为 `题库 / 比赛 / 模板 / 训练 / 工具 / 设置`（多人纯出题仓可通过 `workspace.toml` 隐藏），顶栏切换 `算法库 | 模板册`，复用出版工作室实时预览底座，并提供「展开提交代码」弹窗/抽屉入口。
2. **题解双向联动与 Agent 赋能**：支持从题库代码与 `editorial.md` 代码块一键「沉淀为板子」，板子详情页展示反向关联题目；补齐 CLI、MCP 工具集与工作区 Skill。

**不做**（Phase 5 整体）：25 页页数红线预算仪表盘、强制冻结硬门禁（仅作可选严格开关）、应用内 C++ IDE、离线盲打练习小游戏。

## Phase 6：增强 o11y 与全面整体打磨（已完成，v0.6.0-beta.1）

2026-11-26 对齐确认，按 **6A / 6A.5 / 6B / 6C** 四步推进，详细规格见 [specs/phase-6-o11y-and-polish.md](./specs/phase-6-o11y-and-polish.md)：

### Phase 6A：升级 NSH v0.7.1、清理自造轮子与原生 `<Editor>` 复测

1. **宿主框架升级**：`flake.nix` 与打包链路从 NSH `v0.6.0` 升级至 `v0.7.1`，启用 `nshm` / `nsh.lock` 依赖管理，精简 Linux 打包中不再需要的 GStreamer 强依赖。
2. **清理 6 项自造轮子**：用 `nsh-icon` + `nsh-icon-lucide` 替代手维护的 60+ SVG 与静态图标表；用 `keymap.bind({ overFocused: true })` 删除 `vim_bridge.rs`；用 `nsh/base` `TitleBar` 简化自绘顶栏并复测是否可删除 `linux_chrome.rs`；用 `nsh-o11y` `TimeSeriesChart` 替换训练看板手写折线图（保留年度日历热力图）；用原生 `promptForUpdate({ quiet })` 替换「设置 → 关于」手写更新下载 UI。
3. **原生 `<Editor>` 复测**：先留在 NSH 0.7.1 原生 `<Editor>`，实测粗体标题度量、中文 IME 跟随、长文档滚动与去掉 `vim_bridge.rs` 后的 Vim 键位；仅在存在无法绕过的根因问题时再评估引入 `huacnlee/nsvim` 的 `editor-core`。

### Phase 6A.5：构建体验与 Core 边界优化

1. **Core 依赖解耦与 `apps/desktop` 瘦身**：将 `xcpc-storage` 引用的静态模板文本下沉至 `xcpc-domain` 以斩断 `xcpc-storage → xcpc-engine`（及 `typst-*`）依赖倒挂；将 `apps/desktop/src/` 中与 NSH 无关的纯业务编排与单元测试下沉为 `crates/xcpc-app`，使 `apps/desktop` 回归薄壳且 `cargo test -p xcpc-app` 可秒跑。
2. **本地与 Release 构建加速**：在 `flake.nix` 中接入 `sccache` 跨 workspace 共享第三方重依赖缓存；`.cargo/config.toml` 的 `[profile.release]` 切换为 `lto = "thin"`；CI/Release 工作流缓存 `~/.nsh/nshms`；业务代码保持普通 Cargo crate（不自建本地 `--path` `nshm`）。

### Phase 6B：基于 `nsh-o11y` 的独立诊断窗口与分层采集底座

1. **独立开发者诊断窗口**：基于 NSH 0.7 多窗口能力独立唤起（`Cmd+K` / 快捷键 / `XCPC_O11Y=1`），零侵入主界面，保留 Perfetto JSON 导出。
2. **三大核心信号视图**：`TimeSeriesChart` 联动实时指标（帧耗时/掉帧、编译与光栅化耗时、IPC 延迟、缓存占用）、`TraceWaterfall` + `TraceComparison`（消费 `xcpc-trace` / `trace.ts` 并支持基线前后对比）与 `LogView`（结构化日志柱状图与模块/级别筛选）。
3. **分层采集生命周期**：常态下不跑 `rAF`、不写盘，仅将结构化日志与慢事件/错误写入轻量内存环形缓冲以便回溯；打开诊断窗口或设 `XCPC_TRACE=1` 后激活高频实时帧监控与全量 Span 采集。

### Phase 6C：数据驱动的全面整体打磨

1. **核心链路量化优化**：以真实 Perfetto Trace 与 `TraceComparison` 基线对比驱动，消除重复并发 `preview.compile`、将训练看板与工具链探测异步化（`training_dashboard_async` / `toolchain_status_async` + SWR 缓存）、为冷启动快照与偏好落盘增加指纹去重防抖、缓存命令面板插件/模板索引并常驻挂载 `CommandPalette`、在诊断窗口非 Metrics 页暂停图表重绘；60Hz 实机最慢帧由 `355.5ms` 降至 `97.7ms`（-72.5%），连续打字端到端预览 `9–24ms`。
2. **NSH 0.7 体验红利**：接入 `quitRequest` 未保存改动退出保护（统一刷盘注册表 `flushAllBeforeQuit`）、启用 `smoothWheel` 滚动手感、利用 `DataTable` `grow` 与单图标状态列优化题库表格列宽并完善主/诊断窗口生命周期联动。

### Phase 6D：底层设计系统收敛与重点界面精修（待实施）

2026-11-26 对齐确认，详细规格见 [specs/phase-6d-ui-polish.md](./specs/phase-6d-ui-polish.md)：

1. **底层系统先行**：字号收敛为 6 档 Token（字重只用 400/600）、间距/圆角/控件高度（`24/28/32`，工具行降到 28）刻度化、图标 3 档尺寸加恒定 1.5px 线宽，并以 `tokens.test.tsx` 护栏防止回退；表面层次改为「外壳 + 内嵌浮层面板」（出版工作室同样套用）；页面收敛为列表-详情 / 单页 / 工作室三种原型，页头只能用 `Toolbar` / `DetailHeader`；下划线 Tab 只用于导航级、胶囊只用于参数级；空值不画、整列空则隐藏、复选框按需显露；语义色只做小面积标记。
2. **重点界面精修**：题库从表格改为列表（筛选合并、「导入 Polygon」收进「+」分体菜单、详情预览改为适合宽度的连续滚动阅读器）；比赛详情去掉区块标题与卡片外框，「挑题」改为列表末行；出版工作室标题栏改为面包屑 + 两个图标开关 + 导出主按钮，去掉「专注」按钮（保留快捷键）；训练总览合并为一条概览带，热力图固定为最近 12 个月。其余界面只做 Token 迁移与原型套用。

**不做**（Phase 6 整体）：`ServiceMap` / `FlameGraph` / `LatencyHeatmap`、滚动磁盘日志文件、独立 1,000 题 CI 自动化压测基建、未经实测直接用 `editor-core` 重写编辑器。

## 待定与远期（尚未排期）

- **远期（原候选 Phase 3E）：Lemon-like 本地比赛求解器与 XCPC 反作弊审计**（概念 Spec 见 [specs/local-contest-solver-concept.md](./specs/local-contest-solver-concept.md)，2026-11-24 确认排到远期）：定义开放提交归档协议（`submissions.jsonl` + `codes/`，兼容传统 Lemon 选手目录），Linux 原生轻量沙箱（`bwrap` / namespace + 断网 + 只读工作目录，fail-closed）运行不可信选手代码，支持 ICPC / OI / IOI 赛制与断点续测；引入三道防线反作弊与分层审计模型，产出离线 HTML 成绩单与审计报告、CSV 及 XCPCIO / Resolver 滚榜 JSON。
- **更多社区 OJ 插件**（洛谷 / 牛客 / QOJ / LeetCode 等，基于 Phase 4A 的 `oj_source` + `credentials` 插件协议按需扩展，无需宿主硬编码）。
- **Competitive Companion（`:10043`）按需监听**（若后续有从杂牌 OJ 批量抓样例的刚需，可作为默认关闭的可选开关独立补充）。
- **题单（`sheets/*.toml`）**。
- 跨工作区轻量流转：从外部目录导入题目/比赛（复制到当前工作区，ID 冲突时提示重命名或拒绝覆盖；不做跨工作区活引用与血缘追踪）。
- DOMjudge / Hydro 包的导入，以及 FPS / QOJ 包的导入导出。
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
