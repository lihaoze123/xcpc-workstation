# Phase 2 `desktop-ui` 交接与验收文档

本文是 Phase 2 Wave 3（`desktop-ui`）的单一完整规格交接与验收清单，汇总提交基线、文件边界、模块设计、原生桥接能力、NSH 0.6（Linux GTK4/GSK）硬约束、Phase 2.5 接缝及 12 条端到端验收标准。细节冲突时以 [design-system.md](./design-system.md) 与 [roadmap.md](./roadmap.md) 为准。

---

## 1. 提交基线与协作边界

- **前置提交链**（jj，与 Git colocated，**严禁执行任何会改动仓库状态的 `git` 命令**）：
  - `suwzvqsz`：`feat(phase2): Core, lint, CLI/MCP, preview fixes and desktop bridge`
  - `mwuqyyto`：`docs: 吸收 2026-11-20 精致感与品牌规范并新增 Phase 2.5`
  - `uoupomop`：`feat(desktop): 补齐导出新鲜度、revealInFolder、工作区失联标记与 Phase 2.5 接缝`
  - `txlpyqpy`：`docs: 修正 design-system 导航为 3 空间并记录 desktop-ui 交接文档`（本提交）
- **开工动作**：在 `txlpyqpy` 之上先执行 `jj new -m "feat(desktop): 实现 Phase 2 桌面端界面与出版工作室"` 开启独立 UI 提交。
- **允许修改的文件**：
  - `apps/desktop/src/ui/**`（新建/拆分各模块组件与状态工具）
  - `apps/desktop/App.tsx`（壳层组装、顶层状态与窗口联动）
  - `apps/desktop/App.test.tsx`（覆盖关键 UI 状态逻辑的无头单测）
- **禁止修改的文件**：
  - `crates/**`、根 `Cargo.toml`、`Cargo.lock`
  - `apps/desktop/src/*.rs`、`apps/desktop/Cargo.toml`、`apps/desktop/nsh.json`
  - `docs/**`、`AGENTS.md`
  - 若实现中发现桥接缺口或文档冲突，先向协调者回报，不越界改动。
- **开发与检查命令**（依赖 `XCPC_FONT_DIRS` 或 NSH 时统一走 `nix develop -c`）：
  - 类型与静态检查：`cd apps/desktop && nix develop -c nsh check`
  - 测试：`cd apps/desktop && nix develop -c nsh test`
  - 真机交互验证：`cd apps/desktop && XCPC_WORKSPACE=../../tests/fixtures/olymp-ref/contest-zh/workspace nix develop -c nsh dev`（性能排查可加 `NSH_FPS=1`）

---

## 2. 设计规格与模块划分

### 2.1 视觉 Token 与文案原则（见 [design-system.md](./design-system.md) §1、§3–5、§7）

- **色彩**：中性灰基底 Zinc（`--bg-canvas` Light `#FAFAFA` / Dark `#09090B`；`--bg-surface` `#FFFFFF` / `#111113`；`--bg-elevated` `#F4F4F5` / `#18181B`；`--bg-paper-stage` `#E4E4E7` / `#141417`；`--border-subtle` `#E4E4E7` / `#27272A`；`--border-strong` `#D4D4D8` / `#3F3F46`），浅色/深色自动跟随系统。
- **品牌主色**：深鸭青蓝 Deep Petrol（`H:195°`），Light `#0B6989`（Hover `#09546D`、Soft `rgba(11,105,137,0.10)`），Dark `#249AC2`（Hover `#34B0DA`、Soft `rgba(36,154,194,0.16)`）。
- **难度色阶**：沿用 CF / AtCoder 降饱和 7 段色阶（`<1200` 至 `≥2400`）加 `unrated` 中性灰；所有数字（rating、时空限制、题号计数、页码、统计）严格使用 `font-variant-numeric: tabular-nums`。
- **材质与圆角**：面板之间仅用 `1px --border-subtle` 分割，不加外阴影；圆角遵循梯度（徽章/键帽 `4–6px`、按钮/输入框/选中项 `6–8px`、卡片/弹窗 `10–12px`、预览悬浮胶囊 `9999px`）与**同心圆角**（`内圆角 = 外圆角 − 内边距`）；浮层用轻度毛玻璃（`backdrop-blur: 12px`）+ 暗色模式 `1px` 提亮内描边（`rgba(255,255,255,0.12)`）。
- **文案克制**：
  - 靠布局自我解释，主工作区**不放冗余区块标题**（不写「结构化编辑器」「比赛大纲」「题目描述 (Legend)」），不用 `·`/`|` 装饰分隔符；
  - 严禁暴露技术名词与遥测（如 `SQLite`、`12ms`、`prefs.json`、`MCP Ready`、`NSH Canvas`）；
  - 成功状态静默；耗时 `< 150ms` 的操作不显示 loading；**禁用 toast**。

### 2.2 模块划分（`apps/desktop/src/ui/**`）

| 模块 | 职责与核心交互 |
| :--- | :--- |
| `TitleBar` | `44pt` 自绘顶栏（匹配 `nsh.json` `titleBar: { style: "hidden", height: 44 }`）。通过 `titleBar()` 获取 `{ height, left, right }` 留出窗口控制安全区；顶栏空白区及弹窗背景蒙层启用 `windowDrag` 确保打开浮层时仍可拖动窗口；通过 `currentWindow().set({ title })` 实时同步窗口标题（工作室显示当前对象如 `A. Two Sum — 春季训练赛` / 单题标题，空间视图显示空间名，无工作区显示 `XCPC Workstation`）。 |
| `NavRail` | 左侧 **3 空间**导航：`Cmd+1` 题库、`Cmd+2` 比赛、`Cmd+3` 设置；选中滑块 `150ms cubic-bezier(0.2, 0, 0, 1)`；进入出版工作室时自动收起；启动自动恢复 `prefs.lastSpace` 与上次打开对象。**严禁为「总览」「题单」「参赛记录与复盘」「赛程日历」留占位。** |
| `NoWorkspaceView` | **独立组件（Phase 2.5 接缝 #1）**。无打开工作区时呈现，提供「新建工作区」（`pickFolder()` → 非工作区时 `initWorkspace(path)`）、「打开已有文件夹」（`pickWorkspace()` / `pickFolder()`）及最近工作区列表；当 `WorkspaceInfo.lastWorkspaceMissing === true` 时在入口下方**预留固定高度错误槽**（无布局抖动）显示「上次的工作区已不存在」；首次全新启动（`false`）不显示额外文案。 |
| `ErrorDisplay` | **独立组件（Phase 2.5 接缝 #3）**。遵循 [design-system.md](./design-system.md) §7.3：**三段式结构**（人话说明现象 → 可点击定位 → 仅在有明确下一步时写下一步）；克制语气（不道歉、不拟人、不用感叹号）；固定高度错误槽防抖动；**规则拒绝不当错误**（如 `contest_frozen`、`problem_referenced`、`read_only` 用次级文本色陈述事实并附操作入口，如「解冻」）；主界面隐藏原始错误码与英文原文，并**预留「复制详情」入口**（调 `Clipboard.writeText` 复制技术细节）。 |
| `StudioView` | 沉浸式出版工作室（完整细则见 §2.3）。 |
| `LibraryView` | `Cmd+1` 题库中心：基于虚拟化 `<DataTable>`（行高 `36–40px`），列含 ID、标题、rating（难度色标、`tabular-nums`）、标签、来源、有无题解/代码（**不含训练状态、无「题单」分段**）；多维筛选（关键词、标签、rating 区间、平台，来自 `workspaceFacets` + `searchProblems`）；右侧详情面板只读预览题面与题解、`solutions/` 文件切换只读查看（`readSolution`）、查看引用比赛、「在出版工作室打开」、「加入比赛」、新建/删除题目（`problem_referenced` 按规则拒绝列出引用比赛）。搜索框保持焦点，支持上下键/`J`/`K` 浏览，指针高亮仅监听 `onMouseMove` 避免与键盘滚动冲突。 |
| `ContestsView` | `Cmd+2` 比赛空间：仅本地组卷与快照（无顶部分段）；展示比赛列表、题数、语言、冻结锁标；支持新建比赛（`createContest`）、打开工作室、查看 8 档难度分布横条与标签计数（`contestStats`）、冻结/解冻入口。 |
| `SettingsView` | `Cmd+3` 设置空间：仅包含当前工作区路径与切换/初始化、最近打开工作区列表（`recentWorkspaces`）、MCP 接入配置片段（`mcpConfigSnippet()` + 一键复制 `Clipboard.writeText`，按钮短暂切换为「已复制」）。不放 6 平台同步或主题预设占位（快捷键速查、关于留待 Phase 2.5）。 |
| `CommandPalette` | `Cmd+K` 全局命令面板：`140ms` 缩放 `0.98 → 1` + 淡入，无过冲。基于 `<Command filterable={false}>` 接入 `fuzzyJump(query, activeContest)`（Rust 模糊匹配 + 持久化 Frecency，跳转后调 `recordVisit(visitKey)`）与上下文动作（导出题面册、导出题解册、在文件夹中显示、冻结/解冻、新建题目/比赛、加入比赛、切 Tab、切换工作区、切换空间）。右侧统一展示快捷键 `kbd`。 |

### 2.3 `StudioView` 详细规格

1. **工作室顶栏与状态反馈（就地变形，不用 Toast）**：
   - **冻结锁标**：冻结比赛在比赛名旁显示锁形图标；冻结动作完成时锁扣约 `160ms` 指数缓动合上（仅播一次）；解冻时反向打开后淡出。
   - **导出分体按钮**：主体依据 `exportStatus(request)` 返回的当前册（`statements` 或 `editorial`，二者独立）新鲜度切换：
     - `fresh === true`：显示「在文件夹中显示」（点击调用 `revealInFolder(absPath)`）；
     - `fresh === false` 或 `!exists`：显示「导出 PDF」（点击调用 `export(request)`，成功后刷新 `exportStatus`）；
     - 重启应用后由桥接按 `mtime` 自动恢复新鲜度；
     - `▾` 下拉菜单固定 3 项：`导出题面册`、`导出题解册`、`在文件夹中显示`（PDF 过期时标注次级色「较旧」，从未导出时置灰）；`Cmd+K` 同步暴露这 3 个动作；
     - 导出耗时 `> 150ms` 时仅在按钮内部显示进度，不弹 toast 或全局遮罩；编译成功静默。
2. **左侧大纲（比赛模式）**：
   - 展示封面（`hasCover` 时）与各题 `A..Z` 及真实起始页码（`PreviewResult.starts`，`tabular-nums`）；底部紧凑呈现 `contestStats` 8 档难度分布条与标签计数。
   - 同场比赛内点击题号使用原生 `scrollTo({ y, animated: true })` 短时平滑跳页（可随时被手动滚轮打断）；跨比赛或单题切换直接就位。
   - 支持**零重排拖拽排序**：`4px` 阈值防误触、起拖 `measure()` 缓存槽位、仅浮起替身订阅高频指针、目标位置细蓝线、`180ms EaseOut` 槽位交换、松手保持 `pending` 乐观序直至 `composeContest` 落盘并重排题号 `A..Z`。
3. **主编辑与预览分栏**：
   - 使用 `<ResizablePanelGroup>`（**禁止使用 `<SplitView>`**），初始比例读取 `loadPrefs().splitRatio`，拖拽结束后写入 `savePrefs` 持久化。
4. **编辑区 3 Tab：`题面 | 样例 | 题解`**（`150ms` 指示条；不做结构化分区编辑）：
   - **题面 / 题解**：`<Editor>` 配合 `EditorRef`；`500ms` 延迟原子保存（传 `expectedHash` 做 SHA-256 冲突检测）；诊断（含 `xcpc-lint`）标在编辑器内。
   - **`notify` 外部热刷新与冲突处理**：后台运行 `nextChange()` 循环（切换工作区收到 `"closed"` 时重启新循环）。外部变更命中当前文件且编辑器**无未保存改动**时，经 `EditorRef` 原地更新文本并保留光标与滚动位置（**禁止重建 `<Editor>`**）；若当前有未保存改动（或 `save` 返回 `conflict: true`），在编辑器顶部展示克制冲突条，仅提供二选一：**「载入外部版本」** / **「保留我的版本」**（不做三方合并）。
   - **样例 Tab**：左右双栏编辑 `.in` / `.ans`，支持新增、删除与零重排拖拽排序；调用 `saveSamples(id, samples)` 原子重命名 `01..NN` 并自动清理 UTF-8 BOM 与 `\r`（粘贴时前端也可即时清理）。
5. **右侧出版开关抽屉**：
   - 使用 `transform: translateX()` 浮层滑入（`150–180ms cubic-bezier(0.16, 1, 0.3, 1)`，**严禁动画改宽挤压编辑器**），展开状态持久化至 `prefs.propsOpen`。
   - **上下文写入**：单题上下文写 `problem.toml`（`updateProblemMeta`）；比赛上下文写比赛元数据（`updateContestMeta`）与当前题覆写（`setOverride`）。
   - **覆写标识与还原**：比赛上下文中，根据 `ContestItemDetailJs.overridden*` 标记在对应字段左侧显示细竖条并提供单字段还原按钮；将值改回题库/比赛默认值时桥接自动删除覆写。
   - **冻结只读与解冻确认**：冻结态抽屉淡为只读语气（不按报错显示），仅留「解冻」入口；点击先调用 `unfreezeDiff(id)` 展示差异列表二次确认弹窗，确认后再执行 `unfreezeContest(id)`。
6. **纸面实时预览画布与悬浮胶囊 HUD**：
   - 继承 `App.tsx` 现有两阶段合成器缩放 + `PageImage` 零闪白 SWR 机制：切换题目/比赛/书册时保留旧页直至新首帧到达，按 `(target, id, book)` 记忆 `{ zoom, scrollY, panX }`；核验 `commitZoom()` 在文档末尾附近放大时 `zoom.set` 与 `scrollTo` 同帧原子提交。
   - 画布底色 `--bg-paper-stage`，A4 纸张恒定 `#FFFFFF` + 双层纸张投影 + `1px` 极淡纸边，仅单页竖向滚动（不做纸张反色、双页对开、横向惯性 `<Scroll>`）。
   - **底部居中悬浮毛玻璃胶囊 HUD**（距底 `16px`，圆角 `9999px`，**常驻不淡出**）：
     `[ - | 100% | + | 适合宽度 ]`、`[ ‹ 3 / 12 › ]`、`[ Preflight 状态徽章 ]`（`diagnostics` 无 error/warning 时不显示；点击展开诊断列表，支持按 `file/line` 或 `page` 跳转）。
   - **`Ctrl+点击` 预览定位源码**：将页面点击坐标换算为 0-based `page` 与 typographic points `(xPt, yPt)` 调用 `locateClick(page, xPt, yPt)`，命中后切换至对应文件/Tab 并经 `EditorRef` 定位光标到 1-based `(line, column)`（仅做预览 → 源码单向）。
7. **被动层隐形引导（不做任何主动提示）**：
   - Tooltip（悬停 `600ms` 出现）右侧用 `kbd` 标注快捷键；
   - `Cmd+K` 每项动作右侧标注快捷键；
   - 搜索/输入占位符带淡色 `Ctrl K`；
   - 空工作区/空比赛/空筛选结果仅留克制留白 + 1–2 个直达入口（标出快捷键）。

---

## 3. 原生桥接能力与 NSH 0.6 关键约束

### 3.1 `xcpc-workstation/native` 关键接口（完整签名见 `apps/desktop/.nsh/types/xcpc-workstation/native.d.ts`）

- **工作区与偏好**：`new Preview(process.dataDir)`、`workspace()`（含 `lastWorkspaceMissing: boolean`）、`openWorkspace(path, readOnly)`、`initWorkspace(path)`、`pickFolder()`、`pickWorkspace()`、`recentWorkspaces()`、`mcpConfigSnippet()`、`loadPrefs()`、`savePrefs(prefs)`、`dispose()`。
- **文件读写与热监听**：`read(path)`、`save(path, text, expectedHash?)`（返回 `{ ok, hash, conflict, error }`）、`nextChange()`（异步长轮询 `"changed" | "rescan" | "closed"`，自写事件已由 `SelfWriteTracker` 过滤）。
- **预览、定位与导出**：`compile(request, scale, view)`（仅 `scale`/`view` 变化零重编译复用缓存）、`locateClick(page, xPt, yPt)`、`export(request)`、`exportStatus(request)`（返回 `{ statements, editorial }` 独立 `{ relPath, absPath, exists, fresh }`）、`revealInFolder(path)`（沙箱校验 + 符号链接防护）。
- **题库与样例**：`searchProblems(query)`、`workspaceFacets()`、`problemDetail(id)`、`readSolution(id, name)`、`createProblem(id, title)`、`updateProblemMeta(id, patch)`、`deleteProblem(id)`（遇未冻结引用返回 `code: "problem_referenced"` 与 `referencingContests`）、`saveSamples(id, samples)`。
- **比赛与冻结**：`contestDetail(id)`、`contestStats(id)`、`createContest(id, name)`、`composeContest(id, problemIds)`、`updateContestMeta(id, patch)`、`setOverride(contestId, label, patch)`、`freezeContest(id)`、`unfreezeDiff(id)`、`unfreezeContest(id)`。
- **命令面板**：`fuzzyJump(query, activeContest?)`、`recordVisit(visitKey)`。
- **视图组件**：`<PageImage layers={...} />`（新图层全部就绪前保留旧图层）。

### 3.2 NSH 0.6 平台 API 与 Linux（GTK4 / GSK）铁律

1. **窗口与顶栏（`nsh/desktop`）**：
   - `import { currentWindow, titleBar } from "nsh/desktop";`
   - `currentWindow()` **必须在组件同步渲染期间调用**并缓存 `WindowRef`（在事件回调、定时器或 `await` 后调用会抛 `TypeError`）。
   - 使用 `win.set({ title })` 实时更新窗口标题；使用 `titleBar()` 响应式读取自绘顶栏高度与左右控制区安全边距。
2. **冷启动防 GTK 白闪**：
   - NSH 0.6 在 Linux 上已于 `window.present()` 前同步挂上 `.nsh-light` / `.nsh-dark`；`apps/desktop/nsh.json` 不支持 `backgroundColor`（`MainWindowOptions::from_json` 拒绝未知键），但已配置 `defaultSize: { width: 1280, height: 820 }`、`minSize: { width: 960, height: 640 }`、`titleBar: { style: "hidden", height: 44 }`、`restoreBounds: true`。
   - **TSX 侧硬约束**：根 `<View>` 必须**首帧同步渲染**并设置 `style={{ flex: 1 }}` 与 `class="bg-background"`（映射 `--bg-canvas`），严禁在任何异步初始化完成前返回空树。
3. **窗口几何与分栏恢复分工**：
   - 窗口位置、大小、最大化模式及断开外接屏回退至主屏，由 `nsh.json` 的 `"restoreBounds": true` + `nsh-core` `WindowPlacement::fit_on_displays` 原生自动处理（UI 不得用 `setBounds` 覆盖启动位置）。
   - 编辑区与预览区的 `<ResizablePanelGroup>` 分栏比例由 UI 通过 `loadPrefs().splitRatio` 恢复并在调整后调用 `savePrefs` 落盘。
4. **剪贴板（`nsh`）**：
   - 直接使用 `import { Clipboard } from "nsh"; await Clipboard.writeText(text);`（GTK4 原生支持，无需 Rust 桥接包装），用于 MCP 片段复制及 `ErrorDisplay`「复制详情」。
5. **§8 流畅感工程铁律与 §8.3 Linux 避坑清单**：
   - **合成器优先**：缩放、平移、拖拽排序手势期只改 GPU `transform`，禁止每帧改布局尺寸或反复 `scrollTo`，结束时单次原子提交；
   - **1 帧乐观响应（≤16ms）**：点击、切 Tab、切题、`J/K`、`Cmd+1..3`、`Cmd+K` 当帧反馈，不等待磁盘/编译；
   - **零闪白、零整树重建**：SWR 保留旧画面；列表稳定 `key`；外部热刷新用 `EditorRef` 原地更新，严禁重建 `<Editor>`；
   - **禁止重面板动画改宽**：不用 `LayoutAnimation` / `animateLayout` 改编辑器/预览宽度；抽屉用 `transform: translateX()`，固定侧栏 0ms 切宽 + 内部 120ms 淡入微移；
   - **可打断性 & 降级**：所有程序化动效随时可被用户输入打断，全局遵守 `prefers-reduced-motion`；
   - **零抖动微交互**：聚焦环用 `ring(1).ring_inset()`（`0px` 占位）；分级按压微缩（卡片 `0.99`、侧栏行 `0.98`、常规按钮 `0.96`、图标按钮 `0.94`）；行内操作绝对定位或固定槽位；表单/弹窗底部预留固定错误槽；`Select` 触发重操作用 `setTimeout(..., 0)` 让菜单当帧收起；`<Scroll>` 内图片/页面设 `overflow: hidden` 防止原图尺寸撑大滚动区域；搜索高亮用 `onMouseMove` 不用 `mouseEnter`。

---

## 4. 为 Phase 2.5 预留的 3 处接缝

1. **`NoWorkspaceView` 独立组件**：无工作区界面单独拆为组件，消费 `WorkspaceInfo.lastWorkspaceMissing` 并在固定错误槽呈现失联提示；Phase 2.5 只需替换该组件为品牌页（144px icon + 字标 + 3 入口 + 1.9s 开场动效），零壳层重构。
2. **`prefs.rs` `windowState` 与 `snapshots/`**：`apps/desktop/src/prefs.rs` 已在 `StoredPrefs` 中预留 `window_state: Option<JsonValue>`（无值不落盘、有值跨 `loadPrefs`/`savePrefs` 原样保留）并提供 `snapshots_dir(data_dir)`（`<dataDir>/snapshots`）；UI 侧通过 `loadPrefs`/`savePrefs` 读写偏好时不会擦除预留字段，Phase 2.5 可直接落冷启动预览快照与扩展窗口状态。
3. **`ErrorDisplay` 预留「复制详情」**：所有工作区错误统一经 `ErrorDisplay` 渲染三段式中文提示与规则拒绝语义，并预留「复制详情」操作（通过 `Clipboard.writeText` 复制结构化错误码与底层细节），Phase 2.5 将错误文案集中至 Core 后无需改 UI 结构。

---

## 5. 完整 12 条验收清单

自动化门禁：`cd apps/desktop && nix develop -c nsh check` 零错误、`cd apps/desktop && nix develop -c nsh test` 全部通过（含 Rust 与 `App.test.tsx` 关键状态逻辑测试）。人工验收在 release 构建（或 `nsh dev`）下完整走完以下 12 项：

1. **新建工作区与出题全流程**：`xcpc init` 新建工作区；GUI 内用 `Cmd+K` 新建 4 道题，编写题面、在样例双栏面板增删样例并拖拽排序（粘贴清理 BOM/`\r`）、编写题解。
2. **题库筛选与加入比赛**：在题库中心（`Cmd+1`）按标签、rating 区间和关键词筛选，查看只读详情与 `solutions/`，将题目加入新建的比赛。
3. **工作室组卷、覆写与跳页定位**：在出版工作室大纲内拖拽调题序（题号自动重排 `A..Z`）；在右侧抽屉给 B 题覆写时限（出现细竖条）再一键还原；用大纲和 HUD 跳页；`Ctrl+点击` 预览页面精准定位编辑器源码行。
4. **MCP 实时联动与冲突保护**：Agent 通过 MCP 执行搜索、`write_problem` 修改题面、`compose_contest` 加题；GUI 经 `notify` 无闪白、经 `EditorRef` 不丢光标与滚动位置地热刷新；编辑器存在未保存修改时触发克制冲突条，二选一（载入外部版本 / 保留我的版本）生效。
5. **Preflight 阻断冻结与快照生成**：删除一张配图后，HUD 出现 Preflight 徽章且点击可定位，尝试冻结被拒绝；修复配图后执行冻结，面板转为只读，`snapshot/` 与两本 PDF 正确生成。
6. **解冻差异核对**：在题库修改某道已冻结比赛引用的题目后点击解冻，二次确认弹窗准确列出差异文件/类别，确认后成功解冻并恢复活引用。
7. **§8 流畅感铁律验收**（必要时开 `NSH_FPS=1`）：滚轮/捏合缩放与拖拽排序零抖动、无锚点漂移、无丢帧；切题/切 Tab/开关抽屉零闪白、无 `1px` 布局跳动；程序化动画随时可被打断；高频打字时编辑器与实时预览不卡顿。
8. **导出分体按钮与独立新鲜度**：题面册与题解册各自独立判断新鲜度；未导出或存在编辑/未保存改动时显示「导出 PDF」，导出一致后就地变为「在文件夹中显示」并可在文件管理器中定位；重启应用后按 `mtime` 正确恢复状态；`▾` 菜单固定 3 项（过期标次级色「较旧」、未导出置灰）；耗时 `> 150ms` 仅按钮内显示进度，无 toast 或遮罩。
9. **冷启动、窗口标题、几何状态与分栏比例恢复**：
   - 冷启动无 GTK 白闪（浅/深色系统主题下首帧即呈现 `--bg-canvas`）；
   - 窗口标题随当前对象实时更新（如 `A. Two Sum — 春季训练赛`，无工作区时显示 `XCPC Workstation`）；
   - 调整窗口大小与位置后重启准确恢复几何；
   - **最大化后重启仍保持最大化**；
   - **断开外接屏后重启窗口自动回到主屏可见区域**；
   - **`ResizablePanelGroup` 分栏比例重启后准确恢复**。
10. **冻结/解冻锁标动效与规则拒绝语气**：冻结完成时比赛名旁出现锁形图标，锁扣约 `160ms` 合上仅播一次，右侧面板淡为只读仅留解冻入口；解冻时锁扣反向打开淡出；冻结只读、删除被引用题目等规则拒绝均以次级文本色陈述事实并附操作入口，不按红色报错处理。
11. **三段式错误文案与 Phase 2.5 三处接缝**：报错界面采用三段式（现象 → 可点击定位 → 必要时下一步）且不暴露底层错误码与英文原文，提供「复制详情」写入剪贴板；移除/改名上次打开的工作区目录后重启，独立 `NoWorkspaceView` 在固定错误槽无抖动提示「上次的工作区已不存在」；`prefs.json` 读写往返保留 `windowState` 且 `snapshots/` 接缝完好。
12. **3 空间克制边界与被动层隐形引导**：左侧导航仅 3 空间（`Cmd+1` 题库、`Cmd+2` 比赛、`Cmd+3` 设置），无「总览」「题单」「参赛记录与复盘」「赛程日历」及 6 平台同步占位；tooltip 悬停 `600ms` 与 `Cmd+K` 右侧显示快捷键、搜索框占位符带淡色 `Ctrl K`、空态 1–2 个直达入口标快捷键；全应用无 toast、无主动弹窗提示、无结构化分区编辑、无 HUD 自动淡出。
