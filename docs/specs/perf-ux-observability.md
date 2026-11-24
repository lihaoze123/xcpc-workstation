# 交互、动效、流畅性、可观测性与性能演进规格

本文沉淀 XCPC Workstation 在 **UI/UX 交互、动效、流畅性、可观测性与性能** 五个维度的对齐结论与分期实施规格。整体原则：**先建可观测性底座，拿真实火焰图与量化指标说话；以「编辑 ↔ 预览」与「出题控制台（`BuildConsole`）」两条核心链路为主战场，在工程可行范围内做到极致。**

---

## 1. 定位与规范取舍

| 维度 | 保留的既有原则 | 突破与升级点 |
| :--- | :--- | :--- |
| **可观测性** | 主工作区零技术指标、零实时 HUD 干扰 | 建立 **Rust + JS 统一时间轴离线 Trace**（Perfetto / Chrome Trace `.json`）与**自动化性能预算门禁** |
| **编辑 ↔ 预览** | 单页纯白纸张、GPU 合成器两阶段缩放、零闪白 | 打破「整批编译完再整页硬替换」：改为**视口页优先 + 后台预热相邻页 + 自适应输入节拍与任务取消 + 新旧帧极轻交叉溶变** |
| **出题控制台** | 状态词与克制反馈规范（`authoring-ui-refactor.md`） | 拆解 3300+ 行单一大组件 `BuildConsole.tsx` 为细粒度信号隔离架构；升级为**用例级流式点亮（温和脉冲）+ 异常一键过滤 + 键盘零延迟切换详情** |
| **动效哲学** | 温润物理弹簧底色（`ζ = 0.86–0.90`，过冲 `≤ 1%`）、Compositor-First、可打断、`prefers-reduced-motion` | 重点打通**空间连续性**：消除进出工作室、面板/抽屉、列表筛选与增删的生硬切屏，强化手势速度继承与因果层次 |
| **性能目标** | 60fps / 16ms 响应底线 | **在可能的情况下做到极致**：对齐高刷（120Hz / ≤8.3ms 交互帧预算）与中大型比赛/题库压力基线，消除一切可测的主线程抖动与冗余重绘 |

---

## 2. 可观测性底座（离线全链路 Trace + 性能门禁）

### 2.1 统一时间轴架构（默认零开销）

不做常驻实时 HUD，保持主界面纯净；通过统一结构化 Trace 实现端到端火焰图归因：

1. **Rust 侧（`tracing`）**：
   - 在 `xcpc-convert`、`xcpc-engine`、`xcpc-storage`、`xcpc-judge` 及 `apps/desktop/src/{state,cache,snapshot,lib}.rs` 关键路径补齐结构化 `Span`：
     - 编辑态：`convert.markdown` → `convert.typst_emit` → `engine.compile` → `engine.render_page(page_idx, dpi)` → `cache.lookup/store`
     - 出题态：`judge.compile` → `judge.run_case(case_id)` → `judge.batch_emit`
     - 数据与 I/O：`storage.scan_workspace`、`storage.read_problem`、`watcher.debounce_flush`、`ipc.command(name)`
   - 默认不挂载 Trace Layer（零分配、零锁开销）；启用时写入内存/临时缓冲。
2. **JS / UI 侧（轻量采集器）**：
   - 在 `apps/desktop/src/ui/` 增加极轻量的 `trace.ts`（关闭态为单次布尔判断 no-op）：
     - 记录 IPC 往返耗时（`invoke` 开始/返回，与 Rust 侧 `ipc.command` 通过 `req_id` 对齐）；
     - 记录高频信号提交（`store` / `BuildConsole` / `PreviewCanvas`）与长帧（`frame > 8.3ms` / `> 16.6ms` 掉帧标记）；
     - 记录用户交互锚点（按键、切题、滚动、手势起止）。
3. **单份 Chrome Trace / Perfetto `.json` 导出**：
   - Rust 与 JS 共享单调时钟偏移校准，导出时合并为标准 Chrome Trace Event Format（`traceEvents`），按 `pid/tid` 清晰分为 `UI Main`、`IPC Bridge`、`Typst Worker`、`Judge Pool`、`File Watcher` 泳道，可直接拖入 [Perfetto UI](https://ui.perfetto.dev) 分析。
4. **触发方式**：
   - 环境变量：`XCPC_TRACE=1`（可选 `XCPC_TRACE_OUT=<path>` 自动落盘）；
   - 应用内：「设置 → 关于 / 诊断」提供「录制性能 Trace（开始 / 停止并导出）」入口（默认关闭，不污染主工作区）。

### 2.2 极致导向的性能预算门禁

以中大型真实工作负载为基准（同时覆盖常规 15 题完整 ICPC 题面册 ~35 页 & 500+ 题工作区压力测试）：

| 链路 | 极致目标（P95） | 门禁红线（阻断回归） | 验证手段 |
| :--- | :--- | :--- | :--- |
| 编辑器按键回显 / 键盘列表移动 | `≤ 8.3ms`（120Hz 1 帧） | `≤ 16ms` | JS 无头 + 真机 Trace |
| 切题 / 切 Tab / 切空间（视觉激活态） | `≤ 8.3ms`（乐观 1 帧） | `≤ 16ms` | UI 单测 + Trace |
| 单题增量预览首帧（视口页） | `≤ 60ms` | `≤ 120ms` | `xcpc-engine` 基准 |
| 15 题比赛增量预览首帧（视口页） | `≤ 120ms` | `≤ 250ms` | `preview_bench` 扩展 |
| 单页栅格化（A4 @ 2x） | `≤ 12ms` | `≤ 25ms` | `xcpc-engine` 基准 |
| `BuildConsole` 单次用例进度推送 UI 提交 | `≤ 2ms` | `≤ 6ms` | Trace + 组件重绘计数 |
| 连续缩放 / 平移 / 拖拽掉帧率 | `0%`（60Hz） / `< 1%`（120Hz） | `< 2%`（60Hz） | 真机 `NSH_FPS=1` + Trace |
| 冷启动到快照首帧可打字 | `≤ 150ms` | `≤ 300ms` | 启动计时脚本 |

---

## 3. 核心链路 1：「编辑 ↔ 预览」渐进式管线

目标：让高频输入、长题面册预览与页面切换达到「跟手、无阻塞、无突变」的极致流体感。

1. **自适应输入节拍与任务取消（Generation Supersede）**：
   - 废弃固定死板的单一长防抖：引入基于输入节奏的自适应调度（连续快打时短窗口合并、停顿瞬间立即触发）；
   - Rust 侧编译/栅格化管线引入单调递增 `generation` 令牌：新版本到达时立刻取消/抛弃过期的编译结果与未开始的页面栅格化任务，不浪费 CPU 渲染注定被覆盖的中间帧。
2. **视口页优先 + 投机预热相邻页（Viewport-First & Speculative Pre-warm）**：
   - 编译完成后，优先按当前视口可见页索引排序，**第一时间栅格化并推送视口内 1–2 页**；
   - 视口页落地后，再利用空闲 worker 后台预热 `current ± 1`（及 `± 2`）相邻页进缓存，滚轮翻页零白屏等待；
   - 缩放倍率变化时只优先重绘当前视口页，非视口页懒失效、按需升级清晰度。
3. **新旧帧极轻交叉溶变（Subtle Cross-Fade）**：
   - 同页内容更新时保留旧纹理作为底图，新纹理就绪后执行极短（`~90–120ms`）GPU `opacity` 交叉溶变，消除生硬跳变与闪白；
   - 连续快速更新时可无缝打断、直接接管最新帧，避免多层叠加拖影。
4. **底层热点压榨（由 Phase 1 Trace 定位后精准落地）**：
   - 优化 `xcpc-engine` 增量世界复用（避免重复加载字体/包/未变题目 AST）；
   - 优化像素缓冲区传递与 `PageImage` 更新路径，减少主线程内存拷贝与不必要重排。

---

## 4. 核心链路 2：「出题控制台（`BuildConsole`）」拆解与流式体验

当前 `apps/desktop/src/ui/studio/BuildConsole.tsx` 单组件超过 3300 行，极易在高频跑测事件下引发大范围信号重算与主线程卡顿。

### 4.1 细粒度响应式拆解与事件合批

1. **区域级信号隔离**：将 `BuildConsole` 按职责拆分为独立模块与独立信号域：
   - `AuthoringTree`（左侧文件/解法/测试点树）
   - `TestCaseMatrix`（用例与解法结果矩阵，长列表虚拟化 / 按行细粒度订阅）
   - `BuildDetailPane`（已独立，进一步隔离其内部订阅，避免非当前选中用例的状态变化触发重绘）
   - `AuthoringToolbar` & `StatusSlot`（顶栏动作与汇总状态）
2. **Rust → JS 进度事件帧对齐合批（Frame-Aligned Batching）**：
   - 并发跑测（数十上百个测试点）时，Rust 侧或桥接层以 `~16–33ms` 窗口合批推送状态增量（Delta Patch），而非每跑完 1 个用例就触发一次全量快照序列化与全树刷信号。

### 4.2 流式实时反馈与交互手感

1. **用例级流式点亮**：
   - 跑测 / 检查 / 编译开始后，正在运行的用例显示温和的呼吸脉冲态，单个用例出结果后立即原位平滑过渡到终态（AC / WA / TLE / RE 等），无需干等整批任务结束才突然刷新。
2. **极速排障操作流**：
   - 一键过滤异常行（保留/强化 `onlyAnomalyRows` 为即时零延迟切换，不丢当前选中焦点）；
   - 键盘上下键 / `J` / `K` 浏览用例矩阵时，右侧详情面板 1 帧内响应（大输出/Diff 异步懒截断，绝不卡主线程）。

---

## 5. 全局动效哲学与空间连续性

在保持「温润低过冲（`ζ = 0.86–0.90`，过冲 `≤ 1%`）」核心气质的前提下，重点补齐**空间连续性（Spatial Continuity）**：

1. **进出工作室的层次衔接**：
   - 在规避 GTK4 全屏 `scale(...)` FBO 重采样的约束下，改用分层 `opacity + 轻位移（translateX/Y）` 与顶栏/导航栏错峰收展，让「列表 → 工作室」有明确的纵深推进感，而非生硬切屏。
2. **面板、抽屉与浮层的速度继承**：
   - 所有可收折侧栏（大纲、属性抽屉、状态浮层）与弹窗统一走 NSH 原生 `GdkFrameClock` 弹簧，连续快速点击开关时无缝继承当前速度反向折返，零卡顿、零布局挤压。
3. **列表筛选与增删的柔和过渡**：
   - 题库/比赛筛选、归档收纳展开、样例与组卷项增删排序时，优先保持稳定 `key` 与局部小槽位柔和位移，杜绝整表闪烁或滚动条跳飞。
4. **全局刚性守则不变**：
   - 连续手势全程只动合成器 `transform` / `opacity`；
   - 任意动画随时可被用户输入打断并立即让权；
   - 自动遵守 `prefers-reduced-motion`。

---

## 6. 分期实施路线

### Phase 1：可观测性底座、编译提速与性能门禁
- **编译构建提速**：Linux Nix 开发环境启用 `clang + lld` 增量链接；`.cargo/config.toml` 收窄 `dev` 的 `opt-level = 2` 到热路径渲染/Typst 依赖、`debug = "line-tables-only"` 并新增 `[profile.bench-fast]`；修复 `apps/desktop/build.rs` 开发态监听 `.git/HEAD` 导致的频繁整包重编；
- 引入零开销离线 `xcpc-trace`（Rust 侧原子快速路径 + Chrome Trace 序列化）并埋点核心 Rust 链路；
- 实现 JS 侧轻量 `trace.ts` 与 Rust↔JS 时间轴合并导出（Chrome Trace `.json`）；
- 在「设置 → 关于 / 诊断」加入录制与导出开关，支持 `XCPC_TRACE=1`；
- 扩展自动化基准（`preview_bench --json` + `scripts/perf-gate.sh`），建立基线报告与门禁脚本。

### Phase 2：「编辑 ↔ 预览」渐进式管线
- 落地 `generation` 取消机制与自适应输入节拍；
- 实现视口页优先渲染 + 相邻页后台投机预热；
- 实现 `PreviewCanvas` 新旧帧极轻交叉溶变；
- 结合 Phase 1 火焰图，消除编译、渲染与像素传递中的 Top 热点。

### Phase 3：`BuildConsole` 拆解、事件合批与流式反馈
- 将 `BuildConsole.tsx` 拆分为细粒度信号隔离的子组件，接入行级订阅与虚拟化；
- 实现 Rust→JS 跑测进度 Delta 帧对齐合批；
- 落地用例级流式点亮脉冲、异常行瞬时过滤与键盘零延迟详情切换。

### Phase 4：空间连续性动效与终验
- 打磨进出工作室、抽屉/面板、列表筛选与增删的空间过渡；
- 更新 `docs/design-system.md` 同步已突破与升级的规范；
- 真机全链路录制 Perfetto Trace 与 `NSH_FPS=1` 验收，确认全部指标达标。
