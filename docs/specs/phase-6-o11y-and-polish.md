# Phase 6 规格：增强 o11y 与全面整体打磨

2026-11-26 深度对齐确认，承接 [perf-ux-observability.md](./perf-ux-observability.md) 与 Phase 5（Living TRD），按 **6A → 6A.5 → 6B → 6C** 四步推进：先将宿主框架从 NSH `v0.6.0` 升级至 `v0.7.1` 并系统清理自造轮子，再优化构建体验与 Core 边界（解耦 `xcpc-storage`、下沉 `xcpc-app`、接入 `sccache` 与 `thin` LTO），随后引入 [`nsh-o11y`](https://jetsquirrel.github.io/nsh-o11y/) 搭起应用内实时可观测性诊断窗口，最后以可量化的前后基线对比为依据，对核心链路与交互手感做全面整体打磨。

---

## 1. 目标与原则

1. **底座先行，减法优先**：NSH 从 `v0.6.0` 到 `v0.7.1` 累计合入 600+ PR，带来 `nshm` 模块体系（`nsh-o11y`、`nsh-icon` 的前置依赖）、独立多窗口、`keymap.bind({ overFocused: true })`、`nsh/base` `TitleBar`、`promptForUpdate`、Taffy 0.16 与 Linux Canvas 光栅平移复用。能用上游官方组件与生态模块替代的自造轮子一律删掉，降低长期维护面。
2. **主工作区零污染，诊断能力拉满**：主窗口继续严格遵守 [design-system.md](../design-system.md)（不暴露技术细节、无常驻 FPS/延迟 HUD）；所有可观测性视图收纳进独立的「开发者诊断窗口」（NSH 0.7 多窗口），支持与主窗口并排实时观测。
3. **分层采集，平时零负担、出事可回溯**：未打开诊断窗口时不跑 `rAF`、不写盘，仅在轻量内存环形缓冲中保留结构化日志与慢操作/错误事件；打开诊断窗口或设 `XCPC_TRACE=1` 后再激活高频实时帧监控与全量 Span 录制。
4. **原生 `<Editor>` 优先验证，理性留后路**：先在 NSH 0.7.1 下实测原生 `<Editor>` 的字体度量、粗体偏移、中文 IME 跟随与去掉 `vim_bridge.rs` 后的 Vim 体验；仅在原生 `<Editor>` 仍存在无法绕过的根因问题时，再评估引入 `huacnlee/nsvim` 的 `editor-core`。
5. **用基线对比说话**：每项性能优化以 `TraceComparison` 基线对比与既有性能预算（[perf-ux-observability.md §2.2](./perf-ux-observability.md#22-极致导向的性能预算门禁)）为准绳，避免盲目微调。

### 明确不做（Phase 6 范围外）

- 不接入 `ServiceMap`、`FlameGraph`、`LatencyHeatmap`（聚焦 `TimeSeriesChart` + `TraceWaterfall` / `TraceComparison` + `LogView` 三大核心信号）；
- 不做持久化滚动磁盘日志文件（保留内存环形缓冲 + 手动导出 Perfetto JSON / 复制诊断信息）；
- 不单建 1,000 题规模的 CI 自动化压测基建（沿用本地实测与 `TraceComparison` 基线）；
- 不在未经 NSH 0.7.1 原生 `<Editor>` 复测的前提下直接用 `editor-core` 重写编辑器。

---

## 2. Phase 6A：升级 NSH v0.7.1、清理自造轮子与原生 `<Editor>` 复测

### 2.1 框架升级与 `nshm` 适配

- **`flake.nix` 与打包脚本**：将 NSH 从 `v0.6.0` 升级至 `v0.7.1`（同步更新各平台 release archive 与 `nsh-web` 校验和），同步检查 `scripts/package.sh` 与 CI 工作流。
- **适配 `nshm` 与 `nsh.lock`**：为 `apps/desktop` 启用 `nsh add` / `nsh.lock` 依赖管理，接入 `nsh-icon`、`nsh-icon-lucide` 与 `nsh-o11y`；评估宿主 Rust 扩展是否需按 0.7 规范微调清单字段与 struct 默认值（`..Default::default()`）。
- **精简 GStreamer 打包依赖**：NSH 0.7.1 在 Linux 上改为运行期动态加载 GStreamer，且仅在使用 `Sound` / `AudioPlayer` / `<Video>` 时链接；审查 `flake.nix` 与 Linux 打包脚本，移除多余的构建/打包强依赖。

### 2.2 六项「删轮子 / 换上游」清单

| # | 当前自造实现 | 上游替代方案 | 清理与验收要点 |
| :--- | :--- | :--- | :--- |
| 1 | `apps/desktop/src/ui/icons/*.svg`（60+ 手维护 Lucide SVG）+ `Icon.tsx` 静态表 | `nsh-icon` + `nsh-icon-lucide`（`nsh add`） | 删除 `ui/icons/` 目录；保留轻薄的语义封装或直接映射 `IconTone`，验证 release 产物体积仅打包实际用到的图标 |
| 2 | `apps/desktop/src/vim_bridge.rs`（Rust 零尺寸组件抢 `Esc` / `Ctrl+[`） | NSH 0.7 `keymap.bind(..., { overFocused: true })` | 删除 `vim_bridge.rs` 及导出绑定，验证 Normal / Insert / Visual 下单次 `Esc` 与 `Ctrl+[` 行为完全一致 |
| 3 | `apps/desktop/src/ui/TitleBar.tsx` + `apps/desktop/src/linux_chrome.rs`（GTK `dlsym` 剥离 `menu` 图标） | `nsh/base` 新增的 `TitleBar` | 用 `nsh/base` `TitleBar` 简化安全区与拖拽逻辑（保留居中命令栏与窗口标题同步）；实测 NSH 0.7.1 Linux 是否仍渲染多余应用菜单图标，若已修复则删除 `linux_chrome.rs` |
| 4 | `apps/desktop/src/ui/training/RatingChartCard.tsx` 与 `DifficultyTrendCard.tsx`（手写 `<Canvas>` 折线图） | `nsh-o11y` `TimeSeriesChart` | 替换两条时间序列折线图，获得平滑缩放/平移、联动 crosshair、段位阈值线与比赛注释；保留手写年度日历热力图 `HeatmapCard.tsx` |
| 5 | `SettingsView.tsx` 手写更新下载状态机 + `updateNotes.ts` | NSH 0.7 原生 `promptForUpdate({ quiet })` | 替换自定义更新进度与说明弹窗，自动获得断点续传与 zstd 增量补丁；开发态继续体面提示 |
| 6 | GStreamer 静态/构建期依赖声明与旧 `nsh.json` 布局 | NSH 0.7.1 运行期按需加载 + `nshm`/`nsh.lock` | 清理冗余打包依赖，确保 Linux 开发态与 release 归档均干净通过 |

### 2.3 原生 `<Editor>` 专项复测（门控检查）

升级至 NSH 0.7.1 并去掉 `vim_bridge.rs` 后，在 Linux GTK4 实机上逐项验证：

1. **字体度量与粗体偏移**：取消 [MarkdownEditor.tsx](../../apps/desktop/src/ui/studio/MarkdownEditor.tsx) 中对 `heading` / `strong` 禁用 `font-bold` 的规避注释，实测中英混排粗体标题的光标落点、选区高亮与拖拽命中是否仍有漂移；
2. **中文 IME 合成体验**：验证拼音输入法候选框跟随（含 NSH 0.7 `#3088` caret sink 跟随修复）与连续输入稳定性；
3. **长文档与 Vim 键位**：验证长题面/多题切换时的滚动流畅度、`overFocused: true` 接管后的 Vim 模式切换，以及斜杠命令、公式悬浮预览与诊断浮层坐标。

- **决策门**：若上述体验全部达标（或仅需轻量参数/样式修补），则正式留在原生 `<Editor>`；若粗体度量或核心编辑能力仍存在无法绕过的根因缺陷，记录最小复现路径并单独开启 `editor-core`（裁剪版 + 原生 IME 合成槽）替代评估。

---

## 2.5 Phase 6A.5：构建体验与 Core 边界优化

### 2.5.1 Core 依赖解耦与 `apps/desktop` 瘦身

- **斩断 `xcpc-storage → xcpc-engine` 依赖倒挂**：
  - 将 [crates/xcpc-storage/src/lib.rs](../../crates/xcpc-storage/src/lib.rs) 与 [crates/xcpc-storage/src/example.rs](../../crates/xcpc-storage/src/example.rs) 仅有的 3 处 `xcpc_engine::preset_source` 静态模板文本移出 `xcpc-engine`（下沉至 `xcpc-domain`）；
  - 消除后，`xcpc-storage`、`xcpc-judge`、`xcpc-lint`、`xcpc-plugin`、`xcpc-sync` 均不再依赖 `typst-*`，可与 `xcpc-engine` 并行编译。
- **业务下沉为 `crates/xcpc-app`**：
  - 将 `apps/desktop/src/` 中与 NSH 无关的纯业务编排、状态机、插件宿主、缓存/快照、配置/主题、纯 Rust DTO 与单元测试（`state.rs`、`plugins.rs`、`snippets.rs`、`cache.rs`、`snapshot.rs`、`prefs.rs`、`theme_config.rs`、`types.rs` 纯 Rust 结构、`tests.rs`）下沉至 `crates/xcpc-app`；
  - `apps/desktop` 回归薄壳定位（仅保留 `#[native_module]` / `#[class] Preview` 胶水、JS DTO 映射、`file_dialog`、`linux_chrome.rs` 与 `page_image.rs`），日常逻辑迭代直接 `cargo test -p xcpc-app` 秒跑，不再拖带 `nsh-core` / GTK / QuickJS。

### 2.5.2 `nshm` 边界约定

- 业务代码一律留在普通 Cargo workspace crate，**本次不自建本地 `--path` `nshm`**；
- 仅在未来出现「可跨项目复用或强隔离的纯原生组件」（如独立开源 `PageImage` 或引入 `editor-core`）时再抽成本地 `nshm`。

### 2.5.3 本地开发与 Release 构建加速

- **本地 `sccache`**：在 [flake.nix](../../flake.nix) 的 `devShell` 中加入 `sccache`（`RUSTC_WRAPPER=sccache`），跨根 workspace、`apps/desktop/.nsh/rust` 与 swarm/jj 分支共享 `typst`/`rusqlite`/`rquickjs`/`nsh` 等重依赖缓存；保持 `lld` 链接器，不对 NSH 生成的 `.nsh/rust/Cargo.toml` 做侵入式 hack。
- **Release `thin` LTO 与 CI 缓存**：
  - [.cargo/config.toml](../../.cargo/config.toml) 中 `[profile.release]` 从 `lto = "fat"` 切换为 `lto = "thin"`（保留 `opt-level = "z"`、`strip = true`、`panic = "abort"`），在产物体积基本持平的前提下大幅缩短 release 链接耗时；
  - 在 [.github/workflows/ci.yml](../../.github/workflows/ci.yml) 与 [.github/workflows/release.yml](../../.github/workflows/release.yml) 中补充缓存 `~/.nsh/nshms`；
  - 打包时实测对比优化前后体积与耗时，确认 `nsh-icon-lucide` 与 `nsh-o11y` 未带来非预期的图标/代码体积膨胀。

---

## 3. Phase 6B：基于 `nsh-o11y` 的独立诊断窗口与分层采集底座

### 3.1 独立开发者诊断窗口

- **多窗口形态**：利用 NSH 0.7 独立桌面窗口能力创建「XCPC 诊断」副窗口，与主窗口共享 Rust Core 状态，互不遮挡、互不阻塞；主窗口保持零技术指标。
- **唤起入口**：
  - `Cmd+K` 命令面板（「打开性能与诊断窗口」）；
  - 全局开发者快捷键（例如 `Ctrl+Shift+D` / 避免与现有编辑快捷键冲突）；
  - 启动环境变量 `XCPC_O11Y=1` 自动打开。
- **窗口生命周期**：关闭主窗口时按 `QuitMode` 一并退出（诊断窗口不阻止应用正常退出）；关闭诊断窗口自动退回「常态分层采集」，零残留开销。
- **离线兼容**：保留「导出 Perfetto Trace（`.json`）」按钮，继续兼容 [Perfetto UI](https://ui.perfetto.dev) 深度离线排查。

### 3.2 分层采集架构（常态零负担 + 激活全链路）

在既有 [crates/xcpc-trace](../../crates/xcpc-trace/src/lib.rs) 与 [apps/desktop/src/ui/trace.ts](../../apps/desktop/src/ui/trace.ts) 之上，拆分为两层：

| 层级 | 触发条件 | 采集内容 | 性能约束 |
| :--- | :--- | :--- | :--- |
| **常态轻量层（Always-On Ring）** | 始终开启 | 结构化日志（`INFO`/`WARN`/`ERROR`，含 Core / Preview / Judge / Plugin / IPC 模块标签）+ 慢事件/错误事件（帧耗时 `> 16ms`、IPC / 编译 / 光栅化 `> 50ms`、未捕获异常） | 固定容量内存环形缓冲（如日志 5,000 条、慢事件 1,000 条），**不启 `rAF` 轮询、不写磁盘**，热路径开销可忽略 |
| **激活全量层（Live Diagnostics）** | 诊断窗口打开，或 `XCPC_TRACE=1` / `XCPC_O11Y=1` | 实时时间序列采样（帧耗时、掉帧计数、编译 ms、光栅化 ms、IPC 延迟、像素缓存字节数与命中率）+ 全量 Rust/JS 合并 Span（`ux.inp.*`、`preview_request`、`build_document`、`rasterize_cached`、`judge.*`、`storage.*`） | 定时批量推送至诊断窗口（如 100–250ms 节拍），避免诊断窗口自身反过来拖慢主窗口 |

### 3.3 诊断窗口三大核心视图（全部基于 `nsh-o11y`）

诊断窗口顶部提供统一控制条：**暂停/继续采样**、**清空缓冲**、**时间窗（1m / 5m / 15m）**、**捕获当前 Trace 为基线**、**导出 Perfetto JSON**；主体切分三个 Tab：

1. **实时指标（Metrics）**：
   - 使用 `TimeSeriesChart` + `createChartSync()` 联动展示 4 组关键面板：
     - **帧率与卡顿**：帧间隔（ms）+ `16.67ms`（60Hz）/ `8.33ms`（120Hz）阈值线；
     - **排版与预览管线**：`build_document`（转换+编译）与 `rasterize_cached`（光栅化）耗时；
     - **IPC 与交互响应**：IPC 往返耗时与 `ux.inp.*` 交互延迟；
     - **缓存与内存**：像素缓存占用（MB，对照 LRU 预算上限）与缓存命中率（%）。
   - 支持拖拽缩放到卡顿瞬间、滚轮缩放、双击复位，并在时间轴上标注关键事件注释（如切题、导出 PDF、批量检查、插件重载）。
2. **链路追踪与基线对比（Traces）**：
   - 左侧/上方列出最近捕获的交互与任务 Trace（按耗时、慢请求或类别筛选，标注 INP 与总耗时）；
   - 主视图用 `TraceWaterfall` 展开单次 Trace 的完整 Rust + JS 跨线程 Span 树（关键路径高亮、折叠子树、minimap 导航、点击 Span 查看属性）；
   - 支持将任意一次 Trace 保存为命名基线（如「优化前：15 题切题」），并与后续同场景 Trace 用 **`TraceComparison`** 做逐调用 Diff（红/蓝标出变慢、变快、新增、消失的 Span，支持「仅看变化」过滤），直接服务 Phase 6C 的优化验证。
3. **结构化日志（Logs）**：
   - 使用 `LogView`（顶部日志量时间分布柱状图 + 虚拟化日志列表），按级别（`ERROR` / `WARN` / `INFO` / `DEBUG`）、子系统（`engine` / `convert` / `judge` / `plugin` / `sync` / `ipc` / `ui`）和关键字实时过滤；
   - 点击异常日志可展开上下文参数并一键复制，替换掉以往只能翻终端 stderr 的排障方式。

---

## 4. Phase 6C：数据驱动的全面整体打磨

在 Phase 6B 诊断窗口落地后，先对下列核心场景录制一批 **Phase 6C 前基线**，再逐项定位瓶颈、实施优化并用 `TraceComparison` 验证收益：

### 4.1 核心性能与流畅度主战场

1. **冷启动到可交互（TTI）**：测量从进程启动到主窗口首帧快照绘出、工作区索引就绪、编辑器可打字的完整瀑布图，削减同步阻塞 I/O 与冗余首帧计算；
2. **编辑 ↔ 预览全链路**：压测单题与 15–50 页比赛下的连续打字、公式编辑、滚轮平移与 Ctrl+滚轮/捏合缩放，利用 NSH 0.7.1 Linux Canvas 光栅平移复用与 `xcpc-engine` 增量编译，核验高刷屏掉帧率与视口优先光栅化延迟；
3. **大规模题库与视图切换**：在 500–1,000 题测试工作区下测量题库筛选、排序、快速滚动、`Cmd+K` 检索以及跨空间（题库 / 比赛 / 模板 / 训练 / 工具）切换的 INP，消除列表重排与冗余信号订阅；
4. **出题检查与插件并发**：测量 `BuildConsole` 流式用例推送、整场批量检查、以及 `<WebView>` 插件切换/热重载期间主窗口帧时间，确保后台重任务零阻塞 UI 线程。

### 4.2 吃掉 NSH 0.7 体验红利

- **未保存改动退出保护**：接入 NSH 0.7 可取消 `quitRequest` 事件；存在未保存题面/题解/样例/板子改动时先尝试原子刷盘或弹出克制确认，防止误关丢稿；
- **桌面滚轮手感调优**：评估并配置 `smoothWheel` / `wheelLineHeight`，统一题库、大纲、预览与设置页的鼠标滚轮阻尼；
- **表格列宽自适应**：利用 NSH 0.7 `DataTable` 的 `grow` / `autoFit` 优化题库表与训练提交表在窄窗/宽屏下的列宽分配，消除挤压与多余空白；
- **多窗口协同细节**：统一主窗口与诊断窗口的主题切换（浅色/深色/`theme.toml` 实时同步）、快捷键隔离与退出协调。

---

## 5. 验收标准

1. **6A 验收**：
   - `nix develop`、`cargo check`、`cargo test`、`nsh check`、`nsh test` 与 `scripts/package.sh` 在 NSH `v0.7.1` 下全部通过；
   - 6 项自造轮子全部清理/替换完毕（无残留 `apps/desktop/src/ui/icons/` 与 `vim_bridge.rs`）；
   - 原生 `<Editor>` 复测记录清晰结论（通过或明确记录触发备选方案的条件）。
2. **6A.5 验收**：
   - `cargo tree -p xcpc-storage` 无 `xcpc-engine` / `typst-*` 依赖；
   - `cargo test -p xcpc-app` 脱离 `nsh-core` / GTK / QuickJS 独立全绿，且 `cargo test --workspace`、`nsh check`、`nsh test` 全部通过；
   - `flake.nix` 集成 `sccache`，`.cargo/config.toml` 切换 `lto = "thin"`，CI 缓存 `~/.nsh/nshms`，`scripts/package.sh --target linux --no-upx` 实测验证耗时与体积。
3. **6B 验收**：
   - 独立诊断窗口可正常打开/关闭/与主窗口联动；主窗口无任何新增技术文案或常驻 HUD；
   - 常态下无后台 `rAF` 轮询与额外磁盘写入，且慢事件/错误日志可打开窗口回溯；
   - `TimeSeriesChart`（4 面板联动）、`TraceWaterfall` + `TraceComparison`、`LogView` 以及 Perfetto JSON 导出均在实机工作流中跑通。
3. **6C 验收**：
   - 至少覆盖冷启动、编辑→预览、缩放/平移、500+ 题库筛选滚动与视图切换 5 组核心场景，每组具备 `TraceComparison` 优化前后量化对比，关键指标满足 [perf-ux-observability.md §2.2](./perf-ux-observability.md#22-极致导向的性能预算门禁) 的门禁红线；
   - `quitRequest`、滚动手感、表格自适应与全套本地 lint 检查（`cargo fmt`、`shellcheck`、`py_compile`、`check-plugin-docs.py`）全部通过。
