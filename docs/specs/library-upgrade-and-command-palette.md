# Spec：题库界面全面升级与命令面板优化（Phase 3B+）

> 状态：已实现（2026-11-23 `/grill-me` 共识并完成端到端实现与全量测试）。  
> 排期：Phase 3B 之后、Phase 3C 之前。  
> 关联文档：[roadmap.md](../roadmap.md)、[product.md](../product.md)、[architecture.md](../architecture.md)、[design-system.md](../design-system.md)。

---

## 1. 背景与痛点

随着工作区引入批量 Polygon 题目包与多场赛事，当前题库界面与 `Ctrl+K` 命令面板在规模化使用下暴露出四类问题：

1. **缺少题目分组**：几十到数百道题全部平铺在单一列表里，既无法按备赛主题做持久化分类，也无法按难度、来源、比赛或数据状态临时折叠浏览。
2. **缺少多选与批量操作**：加入比赛、整理标签、标定难度、清理废弃题目、批量构建/验收或批量导出 OJ 包只能逐题重复操作。
3. **表格排版与视觉噪声**：
   - `LibraryView.tsx` 中 `id` 与 `source` 列的 `<Text>` 缺少 `flexShrink: 1, minWidth: 0`，超长文本被硬截断且无省略号；
   - 「数据未验 / 数据待重验 / 数据异常」直接拼接在标题文字末尾，导致整表参差不齐、满屏灰字；
   - 「题解」「代码」各占 `52pt` 却仅画一个 `6px` 圆点，列宽利用率低；右对齐的「难度」表头排序箭头位于文字右侧，触发排序时表头文字会向左偏移。
4. **`Ctrl+K` 命令面板动作难找**：`CommandPalette.tsx` 将 `preview.fuzzyJump(...).slice(0, 30)` 全部排在操作之前，空查询时最多 30 条题目/比赛铺满首屏，常用操作被迫沉底。

---

## 2. 设计原则与非目标

### 设计原则
- **单一 Rust Core**：分组持久化、批量元数据/删除/加赛、批量构建/验收、批量 OJ 导出与合并 PDF 全部在 `crates/` 中实现并强制执行安全边界，桌面 GUI、`xcpc` CLI 与 `xcpc mcp` 共用同一套 Core。
- **不移动物理目录**：`problems/<slug>/` 的目录名就是不可变 ID，持久化分组通过 `problem.toml` 逻辑字段表达，绝不破坏比赛活引用，且避免多人 Git 协同在单一文件上撞车。
- **做减法与零抖动**：严格遵守 [design-system.md](../design-system.md) §4、§7、§8（单层平铺分组不引入嵌套树；固定侧栏 `0ms` 切宽防逐帧重排；非阻塞实时进度，零弹窗打断；成功态静默；数字 `tabular-nums`）。

### 明确不做
- 不做多级嵌套目录树或多对多题集（一题至多归属一个持久化分组；跨维度分类交由 `tags` 与动态「分组方式」）；
- 不通过移动 `problems/<slug>/` 磁盘目录实现分组；
- 不在批量 OJ 导出时暗中触发 C++ 编译或跑数据（严格遵守 Phase 3B「动静分离」验收门禁）；
- 不在批量合并 PDF 时自动创建或污染 `contests/` 目录。

---

## 3. 持久化单层分组（Core + Storage）

### 3.1 领域模型与校验（`crates/xcpc-domain/src/lib.rs`）

1. **元数据与摘要扩展**：
   - `ProblemMeta`：新增 `#[serde(default, skip_serializing_if = "Option::is_none")] pub group: Option<String>`；
   - `ProblemMetaPatch`：新增 `#[serde(default)] pub group: Patch<String>`；
   - `ProblemSummary`：新增 `#[serde(default, skip_serializing_if = "Option::is_none")] pub group: Option<String>`。
2. **筛选与汇总类型**：
   - 为避免用户创建名为 `"ungrouped"` 的分组导致哨兵值冲突，引入显式枚举：
     ```rust
     #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
     #[serde(tag = "kind", content = "name", rename_all = "snake_case")]
     pub enum GroupFilter {
         Named(String),
         Ungrouped,
     }
     ```
   - `SearchQuery` 新增 `#[serde(default, skip_serializing_if = "Option::is_none")] pub group: Option<GroupFilter>`。
   - 新增 `GroupSummary`：
     ```rust
     #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
     pub struct GroupSummary {
         pub name: String,
         pub count: usize,
         pub declared: bool, // true: 声明于 workspace.toml；false: 仅在 problem.toml 出现
     }
     ```
3. **名称规范化与校验（`normalize_group_name`）**：
   - `pub const MAX_GROUP_NAME_CHARS: usize = 64;`（按 Unicode 标量值计数，兼容最长 64 字中文分组名）；
   - 执行 `s.trim()`；拒绝空字符串、拒绝 `chars().count() > 64`、拒绝包含任意 `c.is_control()` 字符；不合法时返回 `InvalidGroupName`；
   - 唯一性按规范化后的字符串精确比对；创建或重命名冲突时返回 `Error::GroupAlreadyExists(name)`（不静默合并）。

### 3.2 `toml_edit` 持久化与 Core 接口（`crates/xcpc-storage`）

- **`workspace.toml`**：新增可选顶层键 `groups = ["省赛候选", "校赛备选"]`，用于维护分组顺序与空分组。
  - `Workspace::workspace_groups(&self) -> Vec<String>`：读取 `groups`，逐项规范化、忽略空项并保序去重；
  - `write_workspace_groups`：经 `check_writable("workspace.toml")` + `toml_edit` 原地更新（空列表时 `root.remove("groups")`），保留键位、注释、`[template-options]` 与未知字段，写前反序列化自检并走 `write_atomic` + `SelfWriteTracker`。
- **`problem.toml`**：
  - `update_problem_meta` 处理 `patch.group`（`Set` 写入规范化值、`Clear` 移除 `group` 键），保留注释与未知字段；
  - `write_problem_files` 写盘前同样校验 `meta.group`。
- **`Workspace` 新增语义方法**：
  1. `list_groups(&self) -> Result<(Vec<GroupSummary>, usize)>`：按 `workspace.toml` 声明顺序返回全部已声明分组（含 0 题空分组，`declared: true`），其后按字典序附带仅在 `problem.toml` 中出现的未声明分组（`declared: false`），并同时返回 `ungrouped_count`。
  2. `ensure_group_registered(&self, name: &str) -> Result<String>`：校验并幂等追加到 `workspace.toml`（供单题/批量归组、Polygon 导入复用）。
  3. `create_group(&self, name: &str) -> Result<String>`：校验并追加到 `workspace.toml`；已存在时返回 `Error::GroupAlreadyExists`。
  4. `rename_group(&self, old_name: &str, new_name: &str) -> Result<RenameGroupReport>`：校验两名称；`old_name` 必须存在于声明列表或至少一道题（否则 `Error::NotFound("group ...")`）；`new_name` 不得已存在（否则 `Error::GroupAlreadyExists`）；原地替换 `workspace.toml` 保持顺序，并逐题 `update_problem_meta` 更新成员 `problem.toml`。
  5. `delete_group(&self, name: &str) -> Result<DeleteGroupReport>`：从 `workspace.toml` 移除，并将所有成员题目的 `group` 清空（退回「未分组」，绝不删除题目本身）。
  6. `reorder_groups(&self, ordered: &[String]) -> Result<Vec<String>>`：校验 `ordered` 为当前 `workspace_groups()` 的严格排列（与 `reorder_contest_problems` 一致）后写回。
  7. `set_problem_group(&self, id: &str, group: Option<&str>) -> Result<ProblemMeta>`：`Some(g)` 时 `ensure_group_registered` + `update_problem_meta`；`None` 时清空。

### 3.3 内存索引与增量刷新（`crates/xcpc-storage/src/index.rs`）

- `WorkspaceIndex` 新增 `declared_groups: Vec<String>`（`scan()` 时经 `ws.workspace_groups()` 初始化）。
- **修复 `refresh_path` 监听缺口**：补齐 `("workspace.toml", None)` 分支，外部修改 `workspace.toml` 时即时刷新 `declared_groups`，驱动侧栏无感更新。
- `load_indexed_problem` 填充 `summary.group` 并将分组名纳入搜索语料 `corpus`。
- 新增 `WorkspaceIndex::group_counts(&self) -> (Vec<GroupSummary>, usize)` 供桌面端 `WorkspaceFacets` 直接读取。
- `WorkspaceIndex::search_problems` 支持 `query.group`（`Named(g)` 精确匹配，`Ungrouped` 匹配 `group.is_none()`）。

### 3.4 Polygon 整场导入自动归组（`crates/xcpc-packages`）

- `PolygonImportOptions` 新增 `pub group: Option<String>` 与 `pub no_auto_group: bool`（默认 `false`）；`PolygonImportResult` 新增 `pub group: Option<String>`。
- 在 `import_polygon_contest` 解析出 `contest_name` 后计算生效分组：
  1. 若显式传入 `opts.group` → 校验并使用；
  2. 否则若 `!opts.no_auto_group` → 清洗 `contest_name`（去除控制字符、折叠空白、截断至 64 字符），非空即作为目标分组名；
  3. 否则 `None`。
- `stage_polygon_problem` 将生效分组写入每道子题的 `ProblemMeta.group`；`commit_staged_files` 原子落盘成功后调用 `ws.ensure_group_registered(&group)?`，保留 `workspace.toml` 既有分组顺序与注释。单题导入若显式指定 `opts.group` 同样生效。

---

## 4. 四类批量操作（Core 层契约）

### 4.1 统一逐题回报结构（`crates/xcpc-domain/src/lib.rs`）

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BatchItemStatus { Ok, Skipped, Failed }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchItemOutcome {
    pub id: String,
    pub status: BatchItemStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub referencing_contests: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchReport {
    pub ok: bool, // failed == 0
    pub total: usize,
    pub succeeded: usize,
    pub skipped: usize,
    pub failed: usize,
    pub items: Vec<BatchItemOutcome>,
}
```

所有批量操作均先按调用顺序对 `ids` 去重，并在进入逐题循环前优先校验工作区级守卫（`Error::ReadOnly`、非法分组名、目标比赛只读/冻结/归档）。

### 4.2 组织与元数据批量操作（`crates/xcpc-storage/src/edit.rs`）

1. **`batch_set_group(&self, ids: &[String], group: Option<&str>) -> Result<BatchReport>`**：
   - 预检只读与 `normalize_group_name`；单题 `meta.group` 未变化时标记 `Skipped` 且不写盘；至少一题存在/成功且 `group.is_some()` 时调用一次 `ensure_group_registered`。
2. **`batch_edit_tags(&self, ids: &[String], add: &[String], remove: &[String]) -> Result<BatchReport>`**：
   - 对 `add` / `remove` 去首尾空白、去空、去重；逐题大小写不敏感移除 `remove`、追加尚不存在的 `add`，无变化时 `Skipped` 不写盘。
3. **`batch_set_rating(&self, ids: &[String], rating: Option<u32>) -> Result<BatchReport>`**：
   - 逐题设置或清空 `rating`，未变化时 `Skipped`。
4. **`batch_update_problems(&self, ids: &[String], group: &Patch<String>, tags_add: &[String], tags_remove: &[String], rating: &Patch<u32>) -> Result<BatchReport>`**：
   - 组合变更单次 `toml_edit` 写盘（供 MCP 一次调用完成多字段批量更新）。
5. **`batch_add_to_contest(&self, contest_id: &str, problem_ids: &[String]) -> Result<BatchAddToContestResult>`**：
   - 预检 `check_id(contest_id)` 与 `check_writable("contests/<id>/contest.toml")`（拦截 `read_only`、`contest_frozen`、`contest_archived`）；
   - 已在比赛中的题标记 `Skipped("already_in_contest")`，不存在/非法 ID 标记 `Failed`；
   - 写盘前原子校验 `existing.len() + to_add.len() <= 26`，超限时直接返回 `Err(Error::TooManyProblems(total))`；
   - 通过既有 `compose_contest` 一次提交，保留已有覆写并自动重排 `A..Z`。

### 4.3 批量删除题目（`crates/xcpc-storage/src/edit.rs`）

- **`batch_delete_problems(&self, ids: &[String]) -> Result<BatchReport>`**：
  - 预检只读；**单次**扫描 `list_contests()` 建立 `problem_id -> Vec<unfrozen_contest_id>` 映射（未冻结的比赛无论是否 `archived` 均阻止删除；已冻结的比赛因依赖 `snapshot/` 不阻止删除）。
  - 逐题处理：
    - 非法 ID / 不存在 / 符号链接逃逸（`path_escape`）/ I/O 错 → `Failed`；
    - 被未冻结比赛引用 → `Skipped`，附 `code: Some("problem_referenced")`、`referencing_contests` 与人类可读说明；
    - 其余安全删除 `problems/<id>/` 并向 `SelfWriteTracker` 写入删除哨兵，绝不静默吞错。

### 4.4 批量构建 / 严格验收题目包（`crates/xcpc-judge/src/build.rs`）

```rust
pub fn build_packages_batch(
    ws: &Workspace,
    slugs: &[String],
    opts: &BuildOptions,
    cancel: Option<Arc<AtomicBool>>,
    progress: Option<&(dyn Fn(BatchBuildEvent) + Send + Sync)>,
) -> Result<BatchBuildReport>
```

- **执行模型**：题目间**严格串行**（保证 Phase 3A 计时不失真；单题内部仍按阶段并发）；进入循环前预检 `read_only` 与 `resolve_active_compiler` 一次。
- **纯出版题处理**：`!meta.has_package_in_dir(&prob_dir)` 直接标记 `Skipped("not_a_package")`。
- **进度事件（`BatchBuildEvent`，serde tagged `kind`）**：
  - `ProblemStarted { problem_id, index, total }`
  - `ProblemProgress { problem_id, index, total, event: ProgressEvent }`
  - `ProblemFinished { item: BatchBuildItemResult, index, total }`（含 `status: Passed | Failed | Skipped | Error | Cancelled`、`verified`、`elapsed_ms`、`code?`、`message?`）
  - `Cancelled { completed, total }`
  - `AllFinished { passed, failed, skipped, cancelled, total, elapsed_ms }`
- **可中断取消**：共享 `Arc<AtomicBool>` 同时传入每题的 `build_package` 并在题间检查；取消后剩余题目记为 `Cancelled`，返回带 `cancelled: true` 的部分报告。

### 4.5 批量导出（OJ 包与合并 PDF）

1. **批量 DOMjudge / Hydro ZIP（`crates/xcpc-packages/src/export.rs`）**：
   - `export_oj_packages_batch(ws, slugs, opts, cancel, progress) -> Result<BatchExportOjResult>`。
   - **严格遵守「动静分离」**：逐题调用 `check_package_freshness`，仅 `VerifiedFresh` 允许打包（`--force` 仅跳过验收门禁，仍要求 `build/problems/<slug>/tests/` 已存在），绝不暗中编译或跑数据；未验收 / 已过期 / 纯出版题记为 `Skipped`（`not_verified` / `missing_testdata` / `not_a_package`），不中断其余题目。
   - 复用同一个 `Engine` 实例编译各题 `problem.pdf`，经 `AtomicZipWriter` 输出 `<slug>.zip` 到目标目录（默认 `build/batch-<format>-<YYYYMMDD-HHMMSS>/`），并逐题发出 `BatchExportEvent` 供前端流式展示进度。
2. **临时合并导出 PDF（`crates/xcpc-convert/src/publish.rs`）**：
   - 新增 `document_problems(ws, problem_ids, opts: &MergedBookOptions) -> Result<Document, Error>` 与 `export_merged_pdf(ws, engine, problem_ids, opts, output_rel) -> Result<MergedPdfOutcome, Error>`。
   - 直接复用既有 `Book` + `Vec<Entry>` 管线（`statements` 或 `editorials`），**不创建** `contests/<id>/` 目录。
   - 题号：`<= 26` 题用 `A..Z`，`> 26` 题用 `1..N`；模板解析沿用 `opts.template -> ws.default_template() -> "olymp-classic"` 并注入工作区 `template-options` 与 `preamble.typ`。
   - 默认文件名：`build/<标题>-试题册.pdf`（缺省 `build/题集-<N>题-试题册.pdf` / `-题解.pdf`）；GUI 指定外部保存路径时走 `write_atomic` 落盘。

---

## 5. 桌面端题库界面重构（`apps/desktop`）

### 5.1 左侧可折叠「分组侧栏」（`180pt`）

- **外层结构**：在 `LibraryView` 既有 `<ResizablePanelGroup>` 外部包裹横向 `<View style={{ flex: 1, flexDirection: "row" }}>`：
  - 左侧：`<If when={effectiveSidebarOpen()}><GroupSidebar ... /></If>`，固定 `width: 180`、`flexShrink: 0`、`borderRightWidth: 1`、`bg-background border-border`；
  - 右侧：原有两栏 `<ResizablePanelGroup>`（表格 `minSize={320}` + 检查器 `size={380} minSize={300} maxSize={560} fixed`）。
- **零重排动效**：遵循 [design-system.md](../design-system.md) §8.3.2，侧栏本身 `0ms` 切宽避免 `DataTable` 逐帧重算列宽；内部列表淡入微移（`opacity 0→1` + `translateX(-4px→0)`，`120ms`，`reduceMotion()` 下跳过）。
- **内容区（无冗余顶标题）**：
  - 顶置固定项（行高 `30pt`，圆角 `6px`）：「全部题目」（`libraryBig`，总题数）与「未分组」（`folder`，未分组数），数字全部 `tabular-nums`；
  - `1px` 分隔线 + `<Scroll style={{ flex: 1 }}>` 渲染自定义分组（按 `workspace.toml` 顺序，单行省略 + 右对齐题数）；右键弹出菜单「重命名分组… / 删除分组…」（只读禁用）；
  - 底部固定「+ 新建分组」幽灵按钮（只读禁用）。
- **折叠与持久化**：
  - 工具栏最左侧新增 `panelLeft` 图标按钮手动切换；外层宽度 `< 1040pt` 时自动收起（不覆盖持久偏好，宽屏恢复；窄屏手动展开走会话覆盖）；
  - `AppPrefsJs`（`prefs.rs`）新增 `librarySidebarOpen: bool`（默认 `true`）与 `libraryGroupBy: Option<String>`。

### 5.2 表格视觉与列结构重构

#### 现存缺陷修复
1. **ID / 来源列硬截断**：`renderCell` 外层 `<View>` 补齐 `minWidth: 0`，所有单行 `<Text>` 补齐 `flexShrink: 1, minWidth: 0`；标签列限制显示 2–3 个 `<Chip maxWidth={96} />` + 紧凑 `+N` 溢出徽章，杜绝半截药丸。
2. **移除标题尾部噪声**：彻底删除标题末尾拼接的灰色「数据未验 / 数据待重验 / 数据异常」文字，统一迁入独立「状态」列。
3. **表头排序箭头对齐**：右对齐列（`ratingText`）的排序箭头改为渲染在文字**左侧**，使表头右边缘与下方数字严格对齐。

#### 新列宽预算（行高维持 `40pt`）
在 `68pt` 导航栏 + `180pt` 分组侧栏 + `380pt` 检查器下，表格可用宽度：`1280px` 窗宽约 `650pt`，`1440px` 窗宽约 `810pt`：

| 列 key | 宽度 | 对齐 / 排序 | 内容与响应式规则 |
| :--- | :--- | :--- | :--- |
| `select` | `36pt` | 居中 / 不可排 | `16×16` 勾选框（`selectable: false`），表头为三态全选框 |
| `title` | 剩余弹宽（`min 180pt`） | 左 / 可排 | 主标题（`13.5px`，单行省略）+ 同行弱化等宽 `id`（`11.5px`，`maxWidth: 112`，`titleW < 220` 时自动隐藏） |
| `ratingText` | `64pt` | 右 / 可排 | `<RatingBadge />` |
| `tagsText` | `168pt`（`w>=540`）/ `120pt`（`460..539`）/ 隐藏 | 左 / 不可排 | 最多 2–3 个定宽上限 `<Chip>` + `+N` |
| `status` | `76pt` | 居中 / 不可排 | 3 个固定 `18pt` 图标槽（`gap: 4`，空槽占位保持纵列齐整），均带 `600ms` tooltip：<br>① 数据包（`shieldCheck` 主色=已验收 / `circleAlert` 琥珀=待重验 / `triangleAlert` 红=验收失败 / `database` 灰=未验收）<br>② 题解（`filePenLine`）<br>③ 代码（`code`） |
| `source` | `132pt`（仅 `w>=760`） | 左 / 不可排 | 单行等宽/次色省略文本 |

### 5.3 动态「分组方式」折叠视图（双模式渲染）

经核查 NSH 0.6 `DataTable`（`base.d.ts:3330-3429`），其行高固定且不支持跨列分组头。为保证零抖动与大规模题库流畅度，采用**双模式渲染**：
- **平铺模式（`groupBy === "none"`，默认）**：继续使用 NSH 原生 `<DataTable>`（原生虚拟化 + `selectOnModifierPress={false}` + `rowClasses` + `onRowPress`）。
- **分组折叠模式（`groupBy !== "none"`）**：
  - 工具栏新增「分组方式」菜单（`无` / `分组` / `难度` / `来源` / `比赛` / `数据状态`）；
  - 上方渲染与 `DataTable` 像素级一致的吸顶表头（复用同一套 `columns()` 与排序事件）；
  - 下方在 `<Scroll style={{ flex: 1 }}>` 中对扁平化可见序列 `Array<GroupHeader(32pt) | ProblemRow(40pt)>` 做窗口化切片渲染（视口 + 上下各 5 行 overscan，O(1) 高度计算）；
  - 全宽 `32pt` 分组头包含：折叠箭头（`120ms` 旋转 `90deg`）、该组三态勾选框、分组标题与 `tabular-nums` 题数；
  - `ProblemRow` 复用与 `DataTable` 完全相同的单元格渲染器，并通过 `View.onPress`（读取 `modifiers` 与 `clickCount`）与 `onContextMenu` 保持完全一致的多选与右键体验。
- **动态分组归属规则（`logic.ts:groupProblems`）**：
  - `group`：按 `workspace.groups` 顺序，末尾为「未分组」；
  - `rating`：按 `TIER_RANGES` 顺序，末尾为「未评定」；
  - `platform`：按首个外部来源平台字典序，末尾为「无外部来源」；
  - `contest`：仅属 1 场未归档比赛 → 比赛名；属 ≥2 场 →「多个比赛」；未加入 →「未加入比赛」（确保多选 ID 唯一不重复）；
  - `package_status`：已验收 / 待重验 / 验收失败 / 未验收 / 无题目包。

### 5.4 多选状态机、底部浮动条与右侧批量面板

#### 多选状态机（纯函数归入 `logic.ts`）
```ts
export interface SelectionState {
  selectedIds: ReadonlySet<string>; // 多选集合
  anchorId: string | undefined;     // Shift+点击 区间锚点
  focusedId: string | undefined;    // 单题聚焦（同步 store.selectedProblem）
}
```
- **普通点击行主体**：清空 `selectedIds`，聚焦并预览该题（`focusedId = anchorId = row.id`）；
- **点击勾选框列（`column === 0`）**：切换 `row.id` 勾选态并更新 `anchorId / focusedId`；
- **`Ctrl/Cmd + 点击`**：若 `selectedIds` 为空且当前有另一道聚焦题 `focusedId`，先把 `[focusedId, row.id]` 一并纳入多选；否则切换 `row.id`；
- **`Shift + 点击`**：将当前可见序列中 `anchorId ?? focusedId` 到 `row.id` 的闭区间纳入 `selectedIds`（保持 `anchorId` 不变）；
- **表头三态勾选框**：在当前筛选可见集上于 `none / partial / all` 间切换全选/全不选；
- **键盘**：`Ctrl/Cmd+A` 全选当前可见行；`Esc` 优先清空多选（无多选时再清空搜索词）；
- **防误删裁剪**：切换搜索词、筛选器或左侧分组时，自动将 `selectedIds` 裁剪为当前可见集的子集，避免误操作隐藏题目；
- **右键菜单**：右键已选行且 `selectedIds.size >= 2` 时弹出批量右键菜单；否则聚焦该行并弹出单题菜单（新增「移入分组 ▸」）。

#### 底部浮动批量操作条
- 当 `selectedIds().size >= 1` 时，在表格区底部居中浮出（`position: "absolute", bottom: 16`，高 `44pt`，圆角 `12px`，`bg-card border border-border`，`modalShadow()`，经 `SPRING_OVERLAY` 温润弹入）。
- 内容：`已选 N 题`（`tabular-nums`）+ 高频动作（表格宽 `>= 680pt` 显示图标+文字，窄宽自动收为图标+tooltip）：**加入比赛**、**移入分组**（内联菜单）、**标签**、**难度**、**构建**、**导出**、**删除**（危险色）、**取消**（`Esc`）。

#### 右侧面板：`ProblemInspector` ⇄ `BatchInspector`
- 右侧 `ResizablePanel` 保持 `380pt`（零宽度跳动）。当 `selectedIds().size >= 2` 或存在进行中/待查看的批量任务时切换为 `<BatchInspector />`；单选且无批量任务时仍显示 `<ProblemInspector />`。
- **`BatchInspector` 结构**：
  1. `DetailHeader`（`52pt`）：标题「已选 N 题」+「取消选择」幽灵按钮；
  2. 选中概览条：难度区间/均值（`tabular-nums`）与数据包状态计数；
  3. 空闲态：完整批量动作列表（加入比赛、移入分组、编辑标签、设置难度、批量严格验收、批量快速构建、批量导出、批量删除）；
  4. **实时非阻塞任务面板**（执行批量构建/验收或批量导出时就地展开，不弹阻断弹窗）：
     - 顶部状态行（「正在严格验收 3 / 12」+ 耗时 `tabular-nums` +「取消」按钮）+ 原生 `<Progress>` 条；
     - `<Scroll style={{ flex: 1 }}>` 逐题结果列表（行高 `34pt`，状态图标 + 标题/ID + 耗时或跳过/失败原因；点击失败题直接跳转出版工作室「构建」Tab）；
     - 完成后底部汇总 `通过 X  失败 Y  跳过 Z` +「在文件夹中显示」（导出类）+「收起结果」。

#### 批量交互弹窗（`apps/desktop/src/ui/Dialogs.tsx`）
在 `dialog` 联合类型中新增：
- `newGroup` / `renameGroup` / `deleteGroup`（删除分组明确提示「仅移除分组，组内题目将移至未分组」）；
- `batchTags`：展示所选题目现有标签及覆盖比例（如 `dp (3/5)`），支持一键标为「添加 / 移除 / 不变」及输入新标签；
- `batchRating`：数值输入（`0..4000`）+ 段位快捷按钮 +「清除难度」；
- `batchDelete`：打开前即对照 `info().contests` 预分为「可删除 `M` 题」与「被未冻结比赛引用跳过 `K` 题（列出比赛名）」；若 `M === 0` 则按规则拒绝态仅显示「知道了」，若 `M > 0` 则明确提示「删除 M 题，跳过 K 题」；
- `batchAddToContest`：泛化既有 `AddToContest` 支持 `problemIds: string[]`，实时展示净增题数并禁用已冻结、已归档或超出 26 题上限的比赛；
- `batchExport`：选择格式（`DOMjudge ZIP` / `Hydro ZIP` / `合并 PDF`）。OJ 格式直接展示门禁预检摘要（`X 题已验收可导出，Y 题将跳过`）；合并 PDF 可填册名、语言（中文/English）与册型（题面册/题解册）。提交后关闭弹窗并在 `BatchInspector` 流式汇报进度。

---

## 6. `Ctrl+K` 命令面板优化（`CommandPalette.tsx`）

将排序与配额逻辑抽离为纯函数 `buildPaletteItems(rawQuery, jumps, actions, activeContest)`（`logic.ts`）：

1. **`>` 前缀仅搜操作**：
   - `raw.trimStart().startsWith(">")` 时进入动作模式，剥离 `>` 后跳过 `preview.fuzzyJump()`，仅检索上下文 `actions()`；
   - 占位符更新为「跳转到题目或比赛，或输入 > 搜索操作」（英文：`Jump to problem or contest, or type > for actions`）；动作模式下搜索图标切为主色 `primary`（无布局抖动）。
2. **空查询（非 `>` 模式）**：
   - **题库 / 比赛列表 / 设置空间**：仅展示「最近访问」最多 **5 条**（按持久化 Frecency 排序），紧接着展示全部当前上下文「操作」，首屏立即可见；
   - **比赛出版工作室内**：展示「本场比赛」题号（按 `A..Z` 序，上限 **8 条**），紧接着展示「操作」。
3. **有输入查询（非 `>` 模式）**：
   - 按分组设上限：`本场比赛` 最多 **6 条**、`题目` 最多 **8 条**、`比赛` 最多 **4 条**、`操作` **不截断**；
   - 操作相关度打分（精确 `100` > 前缀 `70` > 标题子串 `40` > 关键词子串 `20`），若最高操作分 `>= 70` 且对象无精确 ID/标题命中，则将「操作」组提升至对象组之前。
4. **新增快捷动作**：`新建分组`（`a:new-group`）、`显示/隐藏分组侧栏`（`a:toggle-group-sidebar`）。

---

## 7. 桌面端 Rust 桥接、CLI 与 MCP 契约

### 7.1 桌面端桥接（`apps/desktop/src/{types.rs, state.rs, lib.rs}`）

- **DTO 扩展**：
  - `ProblemSearchQuery`：新增 `group: Option<String>`、`ungrouped_only: bool`（在 `state.rs` 映射为 `GroupFilter`）；
  - `ProblemSummaryJs`：新增 `group: Option<String>` 与 `contest_ids: Vec<String>`（内存索引直出，避免按比赛分组时 N+1 IPC）；
  - `ProblemDetailJs`：新增 `group: Option<String>`；
  - `WorkspaceFacets`：新增 `groups: Vec<GroupSummaryJs>`、`ungrouped_count: u32`、`total_count: u32`。
- **同步桥接方法**：`create_group`、`rename_group`、`delete_group`、`reorder_groups`、`batch_set_group`、`batch_edit_tags`、`batch_set_rating`、`batch_add_to_contest`、`batch_delete_problems`。
- **无轮询异步任务流**（复用 `next_change` 的 `mpsc::Receiver` + `nsh_core::blocking` 范式）：
  - `State` 持有 `batch_task: Arc<Mutex<BatchTaskSlot>>`（含 `task_id`、`cancel_flag: Arc<AtomicBool>`、`event_rx`、`snapshot`）；
  - `start_batch_build(ids, mode)` / `start_batch_export(req)` / `cancel_batch_task()` / `batch_task_status()` / `async fn next_batch_event(task_id) -> BatchTaskEventJs`；
  - 窗口关闭（`Preview::drop`）或切换工作区时自动置位 `cancel_flag` 并丢弃 sender，使 `next_batch_event` 立即返回 `"closed"`。

### 7.2 CLI 最小闭环（`crates/xcpc-mcp/src/main.rs`）

- `xcpc search`：新增 `--group <NAME>`（与 `--ungrouped` 互斥）和 `--ungrouped`；
- `xcpc group list|create|rename|delete|reorder [--json]`；
- `xcpc problem group <GROUP|--clear> <ID>... [--json]`；
- `xcpc problem tag [--add <T>...] [--remove <T>...] <ID>... [--json]`；
- `xcpc problem rating <N|--clear> <ID>... [--json]`；
- `xcpc problem delete <ID>... [--yes] [--json]`；
- `xcpc import polygon <zip> [--id <slug>] [--group <name>] [--no-auto-group] [--json]`；
- `xcpc package build <ID>... [--strict] [-j <N>] [--json]`（单 ID 保持原输出结构，多 ID 走 `build_packages_batch`）；
- `xcpc export <domjudge|hydro> problems <ID>... [-o <DIR>] [--force] [--json]`；
- `xcpc build problems <ID>... [--title <T>] [--editorial] [--lang <zh|en>] [--cover <bool>] [-o <PATH>] [--json]`。

### 7.3 MCP 最小闭环（`crates/xcpc-mcp/src/mcp.rs`）

> 注：现有 MCP 写题工具名为 `write_problem`（无需改名），其写回 `problem.toml` 自动兼容 `group` 字段。

- `search_problems`：新增可选参数 `group?: string`、`ungrouped?: boolean`（同传报错）；
- `import_polygon`：新增可选参数 `group?: string`、`no_auto_group?: boolean`；
- `manage_groups`：`action: "list" | "create" | "rename" | "delete" | "reorder"`（仅 `list` 允许在 `--read-only` 下调用）；
- `batch_update_problems`：`ids: string[]` + 可选 `group?: string | null`、`tags_add?: string[]`、`tags_remove?: string[]`、`rating?: number | null`（`--read-only` 拒绝）；
- `batch_delete_problems`：`ids: string[]`（`--read-only` 拒绝，强制执行 `problem_referenced` 守卫）；
- `export_oj_package`：扩展 `target` 支持 `"problems"` + `ids: string[]`（输出严格限制在工作区 `build/` 内，`--read-only` 拒绝）。

---

## 8. 错误码、安全边界与验证计划

### 8.1 新增错误码
- `invalid_group_name`：`分组名称「{name}」无效。名称不能为空、不能包含控制字符，且长度不超过 64 个字符。`
- `group_already_exists`：`分组「{name}」已存在。请使用其他名称。`
- `not_found`：扩展对 `"group "` 前缀的中文提示。
- 复用既有错误码：`read_only`、`not_found`、`invalid_id`、`path_escape`、`contest_frozen`、`contest_archived`、`too_many_problems`、`problem_referenced`、`not_verified`、`missing_testdata`、`cancelled`、`no_compiler`、`pdf_compile_failed`。

### 8.2 安全边界（Core 强制）
1. 所有写入走 `write_atomic` / `AtomicZipWriter` 与 `SelfWriteTracker`；
2. `--read-only` 拦截一切分组写入、批量元数据/删除、批量构建与批量导出；
3. 冻结与归档比赛拒绝批量加题；未冻结比赛（含已归档）阻止批量删题；符号链接一律报 `path_escape`；
4. 批量 OJ 导出严格校验 `verified == true` 且哈希未过期；MCP 输出严格锁在工作区路径沙箱内。

### 8.3 测试与验收清单
- **Rust 单元测试 & 集成测试**：
  - `xcpc-domain`：`normalize_group_name` 边界（空白、64 字中文、超长、控制字符）与 serde 往返；
  - `xcpc-storage`：`toml_edit` 保留注释/顺序/未知字段、分组 CRUD、`refresh_path("workspace.toml")`、`GroupFilter` 搜索、全部批量操作（含 26 题上限原子拒绝、冻结 vs 未冻结引用删除守卫、`--read-only`）；
  - `xcpc-packages`：Polygon 整场导入自动归组（默认 / `--no-auto-group` / `--group`）与 `export_oj_packages_batch` 混合门禁 + 取消；
  - `xcpc-convert`：`document_problems` / `export_merged_pdf`（`<=26` 与 `>26` 题号，不创建比赛目录）；
  - `xcpc-judge`：`build_packages_batch` 串行执行、纯出版题跳过、`AtomicBool` 取消；
  - `xcpc-mcp`：CLI 与 MCP 全部新增/扩展接口及 `--read-only` 守卫。
- **前端逻辑与桥接测试**：
  - `apps/desktop/src/ui/library.test.tsx`（`nsh test`）：`applyRowSelection`、`headerCheckboxState`、`groupProblems`、`computeLibraryColumns`、`buildPaletteItems`、`partitionBatchDelete`；
  - `apps/desktop/src/tests.rs`：`prefs.rs` 往返、分组与批量桥接、`next_batch_event` 取消与完成；
  - `i18n.test.tsx`：`zh-CN.ts` / `en.ts` 键集完全对齐。
- **视觉与交互人工验收**：
  - `1280px` 与 `1440px` 宽窗下检查 ID/来源省略号、无「数据未验」标题噪声、状态三槽对齐、分组侧栏 0ms 切换零抖动、动态分组折叠、多选浮动条 + 右侧批量任务实时进度，以及 `Ctrl+K` 空查询 / `>` 前缀体验。
