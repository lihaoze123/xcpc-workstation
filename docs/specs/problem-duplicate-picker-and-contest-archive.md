# Spec：复制题目、比赛批量挑题弹窗与比赛归档

> 状态：已确认（2026-11-22 `/grill-me` 共识），待实现。  
> 关联文档：[product.md](../product.md)、[architecture.md](../architecture.md)、[design-system.md](../design-system.md)、[roadmap.md](../roadmap.md)。

---

## 1. 背景与痛点

当前工作站在题库与比赛日常管理中存在三处明显断层：

1. **无法在应用内复制题目**：
   - [product.md](../product.md) §3 明确规定「题面正文、样例、配图不能在比赛里覆写；需要改动时在题库里复制出一道新题」，但 Core 与界面均未提供复制题目能力，用户只能手动在文件管理器拷贝目录再改 `problem.toml`。
2. **「从题库挑题」路径断裂且不支持批量添加**：
   - 比赛空态页点击「从题库挑题」目前只是 `goSpace("problems")` 跳转到普通题库，既没有「正在为某场比赛选题」的上下文，也不能一次选中多道题加入；
   - 比赛已有题目后，详情页和出版工作室大纲甚至缺少从题库挑题的直接入口，只能切回题库逐题点「加入比赛…」。
3. **缺少比赛收纳（归档）机制**：
   - 应用内既不支持删除比赛，也不支持归档。办完的历史比赛或废弃草稿一直堆在比赛列表里，无法收起。

---

## 2. 设计原则与非目标

### 设计原则
- **单一 Rust Core**：复制题目、归档/取消归档比赛的全部语义与安全边界在 `crates/xcpc-storage` 与 `crates/xcpc-domain` 中强制执行，桌面 GUI、`xcpc` CLI 与 `xcpc mcp` 共用同一套实现。
- **数据安全优先**：不做应用内删除比赛，通过可逆的「归档 / 取消归档」满足收纳需求。
- **做减法（Less is more）**：批量选题集中在比赛侧的「从题库挑题」弹窗完成，题库表格保持单选、不引入多选列；没有已归档比赛时，归档区块完全隐藏不占空间。
- **交互克制与零抖动**：严格遵守 [design-system.md](../design-system.md) §4、§7、§8（高不透明度实色弹窗、就地替换错误行、零重排、成功态静默）。

### 明确不做
- 不在应用内提供「删除比赛」功能（彻底清理仍由用户在文件管理器处理）；
- 不在题库主表格（`LibraryView`）增加多选框或批量操作工具栏；
- 不做「挑题弹窗取消勾选即从比赛移除」的集合同步模式（避免误删已有题目的比赛元数据覆写）；
- 不做专门的题目 fork / 血缘追踪关系（复制后的题目完全独立）。

---

## 3. 复制题目（Duplicate Problem）

### 3.1 Core 层契约（`xcpc-storage`）

新增方法：

```rust
impl Workspace {
    pub fn duplicate_problem(
        &self,
        source_id: &str,
        new_id: &str,
        new_title: Option<&str>,
    ) -> Result<Problem>;
}
```

#### 行为与安全约束
1. **权限与参数校验**：
   - 工作区只读时返回 `Error::ReadOnly`；
   - 对 `source_id` 与 `new_id` 执行 `check_id` 校验；
   - 检查 `problems/<source_id>/problem.toml` 存在，否则返回 `Error::NotFound("problem \"...\"")`；
   - 检查 `problems/<new_id>/` 尚不存在（目录或 `problem.toml` 已存在均拒绝），冲突时返回 `Error::Conflict`。
2. **完整递归复制（拒绝符号链接逃逸）**：
   - 递归复制 `problems/<source_id>/` 下的全部内容（含 `problem.toml`、`statement*.md`、`editorial.md`、`samples/`、`assets/`、`solutions/`、`generators/`、`tests/`、`checker.cpp` 等）；
   - 复制过程中对每个条目使用 `symlink_metadata` 检查，**拒绝跟随或复制符号链接**（遇符号链接返回 `Error::PathEscape`），同时校验目标路径不越出工作区；
   - 先复制到 `problems/` 下的临时目录，完成 `problem.toml` 标题更新后再原子 `rename` 到 `problems/<new_id>`，中途失败自动清理临时目录，避免留下半成品题目。
3. **更新新题目 `problem.toml`**：
   - 若传入非空 `new_title`，通过 `toml_edit` 原地更新 `title` 字段，保留原文件的注释、键顺序、未知字段、`[[source]]` 与 `[template-options]`；
   - 若 `new_title` 为 `None` 或全空白，保留源题目的 `title`。
4. **返回值与索引**：
   - 返回新创建的 `Problem`；桌面端桥接层调用后同步刷新 `WorkspaceIndex` 并记录 `SelfWriteTracker` 抑制多余的文件监听回调。

### 3.2 桌面端交互（`apps/desktop`）

1. **入口**：
   - 题库表格行右键菜单：在「加入比赛…」上方增加「复制题目…」（只读工作区禁用）；
   - 右侧 `ProblemInspector` 顶部 `⋯` 菜单：增加「复制题目…」（只读工作区禁用）。
2. **「复制题目」弹窗（`Dialogs.tsx`）**：
   - 复用 `Form` / `Field` 表单样式与 `DialogHeader`（图标 `copy`，标题「复制题目」，说明「复制整道题的题面、样例、配图和题解为新题目。」）；
   - **字段与默认值**：
     - **标题**：默认预填 `<原标题>（副本）`，自动聚焦；
     - **ID**：默认预填 `<原ID>-copy`（若工作区中已存在同名 ID，则自动递增为 `<原ID>-copy-2`、`<原ID>-copy-3`…；若超长则截断原 ID 保留后缀，确保满足 64 字符上限）；
     - 若用户未手动改过 ID 且修改标题，**不**覆盖已生成的 `-copy` 建议（与新建题目区分，避免打中文标题把默认 ID 清空）；
     - 校验错误（无效 ID / 重名冲突）就地替换 ID 字段的说明行，零抖动。
   - **提交后行为**：
     - 调用 `preview.duplicateProblem(sourceId, newId, title)`；
     - 刷新工作区、关闭弹窗、**留在题库并自动选中新题**（同步滚动到可见行）。

---

## 4. 比赛「从题库挑题」弹窗与批量添加

### 4.1 交互定位（追加模式）

- 挑题弹窗（`PickProblemsModal`）是面向**单场特定比赛**的批量选题模态框，采用**纯追加模式**：
  - 不会移除比赛里已有的题目，也不会改动已有题目的顺序与元数据覆写；
  - 确认后将本次选中的题目按勾选顺序追加到比赛末尾，底层复用既有 `preview.composeContest(contestId, [...existingIds, ...pickedIds])`（Core 层自动重排 `A..Z` 并保留已有覆写）。

### 4.2 入口位置

仅当工作区非只读、且目标比赛**未定稿且未归档**时启用（只读 / 已定稿 / 已归档状态下禁用或隐藏）：

1. **比赛空态页（`ContestsView`）**：点击「从题库挑题」直接弹出 `PickProblemsModal`（取代原有的 `goSpace("problems")`）；
2. **比赛详情页「题目」区块右上角（`ContestsView`）**：在题数统计旁增加紧凑的「挑题」按钮；
3. **比赛列表右键菜单 & 详情页顶部 `⋯` 菜单**：增加「从题库挑题…」（置于「新建题目加入…」之上）；
4. **出版工作室左侧大纲底部（`Outline.tsx`）**：增加「从题库挑题」入口（位于统计区上方或紧邻列表底部，仅未定稿、未归档、非只读时显示）。

### 4.3 弹窗结构与交互细节

- **尺寸与材质**：宽度 `640px`（`maxHeight` 自适应视口，列表区独立滚动），遵循 [design-system.md](../design-system.md) §4 模态弹窗实色表面规范。
- **顶部区域**：
  - 标题行：「从题库挑题」 + 副说明显示目标比赛名（如「加入「ICPC 2026 Regional」末尾」）；
  - 筛选栏：复用题库检索能力（调用 `preview.searchProblems` 与 `preview.workspaceFacets`）：
    - 关键词搜索框（自动聚焦，120ms 防抖，支持标题 / ID / 标签 / 来源）；
    - 标签多选菜单、难度段位菜单、来源平台菜单、以及有筛选条件时的「清除」按钮。
- **中间题目列表**：
  - 每行显示：勾选框、等宽 ID、标题、难度徽章（`RatingBadge`）、标签摘要（紧凑展示）；
  - **已在比赛中的题目**：行尾显示弱化文字「已加入」，整行降低透明度并禁止勾选；
  - **名额上限控制**：剩余名额 `remaining = 26 - existingIds.length`。当已选数量达到 `remaining` 时，其余未勾选行置灰禁止继续勾选（已勾选行仍可取消勾选）；
  - **跨筛选保留勾选**：切换搜索词或标签筛选时，已勾选的题目 ID 集合保持不变（按勾选顺序存储为 `string[]`）；
  - **键盘支持**：`↑` / `↓` 移动高亮行，`Space` 切换勾选，`Cmd+Enter` 直接提交，`Escape` 优先清空搜索词、搜索为空时关闭弹窗。
- **底部操作栏**：
  - 左侧状态（`tabular-nums`）：显示「已选 N 题 · 剩余名额 M」（若已选 0 题则显示「还可加入 M 题」，无需技术术语；不使用 `·` 分隔符，遵循 §1.4 改用间距分隔）；若已有选中项，提供轻量的「清空已选」幽灵按钮；
  - 右侧按钮：「取消」（幽灵按钮）与「加入 N 题」（主色按钮；`N === 0` 时文案为「加入」且禁用）；
  - 若 `composeContest` 返回错误，在底部上方用 `ErrorDisplay` 展示。
- **空题库兜底**：当工作区题库为空时，弹窗内展示空态提示并提供「新建题目加入」按钮直接切到新建题目弹窗。

---

## 5. 比赛归档（Archive Contest）

### 5.1 数据模型与 Core 规则（`xcpc-domain` / `xcpc-storage`）

1. **持久化字段**：
   - `ContestMeta`（`contests/<id>/contest.toml`）新增可选布尔字段：
     ```toml
     archived = true   # 默认 false；取消归档时从 contest.toml 移除该键保持整洁
     ```
   - 与 `frozen`（定稿）**完全正交**：
     - 未定稿的比赛可以归档（保留活引用）；
     - 已定稿的比赛也可以归档（保留 `snapshot/`）；
     - 取消归档后恢复其原有的 `frozen` 状态，不触发定稿也不触发重新编辑。
2. **新增错误变体**：
   - `xcpc_storage::Error::Archived(String)`，稳定错误码 `code = "contest_archived"`；
   - 集中式中文文案（`human_message`）：`比赛「{id}」已归档，当前只读。如需修改，请先「取消归档」。`
3. **只读保护边界**：
   - 在 `Workspace` 中，凡是对比赛进行写入的操作（`compose_contest`、`update_contest_meta`、`set_override`、`freeze_contest`、`unfreeze_contest`、`write` 比赛目录下文件等），当目标比赛 `archived == true` 时一律拒绝并返回 `Error::Archived(id)`；
   - 唯一允许修改已归档比赛的操作是 `set_contest_archived(id, false)`。
4. **Core 层 API**：
   ```rust
   impl Workspace {
       pub fn set_contest_archived(&self, id: &str, archived: bool) -> Result<ContestMeta>;
   }
   ```
   - 校验 `check_id(id)` 与工作区非只读；
   - 校验比赛存在（不存在返回 `Error::NotFound`）；
   - 使用 `toml_edit` 原地写入 `archived = true`（当 `archived == true`）或移除 `archived` 键（当 `archived == false`），保留 `contest.toml` 的注释、顺序与未知字段；
   - 幂等：对已归档比赛再次归档、或对未归档比赛取消归档均正常返回 `Ok(meta)`。
5. **删除题目的引用保护**：
   - 保持现有逻辑不变：`delete_problem` 检查所有 `!contest.meta.frozen` 的比赛（无论该比赛是否已归档）。若一道题被未定稿的已归档比赛引用，依然拒绝删除并列出该比赛，确保取消归档后不会出现题目缺失。

### 5.2 桌面端交互（`apps/desktop`）

1. **数据结构桥接**：
   - `WorkspaceContest` 与 `ContestDetailJs` 增加 `archived: boolean`。
2. **左侧比赛列表（`ContestsView`）**：
   - 主列表仅显示 `archived === false` 的进行中比赛；顶部工具栏计数显示进行中比赛数（如 `N 场比赛`）；
   - 当存在至少 1 场已归档比赛时，在列表底部（或紧跟主列表下方）展示可折叠区块：**`已归档  N`**（带展开/折叠小箭头，默认折叠）；无归档比赛时**完全不渲染**；
   - 展开后列出所有 `archived === true` 的比赛（视觉略微柔和，可同时显示定稿锁图标），点击可在右侧正常查看详情与预览；
   - 归档某场比赛后：若当前选中的正是该场，自动选中下一场进行中比赛（若已无进行中比赛则清空或保留选中）；取消归档后自动选中该场比赛并确保主列表可见。
3. **比赛详情页与工作室（`ContestsView` / `StudioView`）**：
   - `viewOnly = detail().frozen || detail().archived || readOnly()`；
   - 详情页标题旁徽章：
     - `archived` 为 `true` 时显示「已归档」徽章（若同时 `frozen`，并排显示「已定稿」与「已归档」）；
   - 顶部操作区：
     - 主按钮在 `archived` 时显示「查看」（进入只读工作室，不可编辑题面、不可拖拽排序、不可改属性）；
     - 归档状态下隐藏「定稿 / 重新编辑」写操作按钮，替换为「取消归档」按钮（非只读工作区可用）；
   - 归档 / 取消归档菜单入口：
     - 比赛列表右键菜单与详情页 `⋯` 菜单中，未归档比赛显示「归档比赛」，已归档比赛显示「取消归档」（只读工作区禁用）。
4. **题库侧联动**：
   - 题库「加入比赛…」弹窗（`AddToContest`）：过滤掉 `c.archived === true` 的比赛，避免误操作；
   - 题库 `ProblemInspector` 的引用比赛标签：点击引用了该题的已归档比赛跳转过去时，自动展开「已归档」区块并选中该比赛。
5. **`Cmd+K` 模糊跳转**：
   - 已归档比赛仍可被搜索到（避免用户找不到旧赛），但在结果项中附带「已归档」次级标记，且排序权重低于未归档比赛；跳转时自动展开归档区。

---

## 6. CLI 与 MCP 接口规范（`crates/xcpc-mcp`）

### 6.1 `xcpc` CLI

1. **复制题目**：
   - 新增顶层子命令组 `xcpc problem`（与现有 `xcpc contest` / `xcpc template` 对齐）：
     ```bash
     xcpc problem duplicate <SOURCE_ID> <NEW_ID> [--title <TITLE>] [--json]
     ```
   - `--json` 输出：`{ "ok": true, "source_id": "...", "id": "...", "title": "...", "path": "problems/<new_id>" }`。
2. **归档 / 取消归档比赛**：
   - 在现有 `xcpc contest` 下新增：
     ```bash
     xcpc contest archive <ID> [--json]
     xcpc contest unarchive <ID> [--json]
     ```
   - `--json` 输出：`{ "ok": true, "id": "...", "archived": true|false, "frozen": true|false }`。
3. **列表与详情输出**：
   - `xcpc list`（含 `--json`）和 `xcpc show contest <id>` 的 JSON/文本输出补齐 `archived` 字段。

### 6.2 `xcpc mcp` 工具

均受 `--read-only` 约束（只读模式下拒绝写入并返回 `read_only` 错误码）：

1. **`duplicate_problem`**
   - 参数：
     - `source_id` (`string`, required)：源题目 ID
     - `new_id` (`string`, required)：新题目 ID（合法 slug）
     - `title` (`string`, optional)：新标题；省略则沿用源标题
   - 返回：新题目的 ID、标题、路径与文件摘要。
2. **`archive_contest`**
   - 参数：`id` (`string`, required)
   - 返回：`{ "id": "...", "archived": true, "frozen": ... }`。
3. **`unarchive_contest`**
   - 参数：`id` (`string`, required)
   - 返回：`{ "id": "...", "archived": false, "frozen": ... }`。
   - 注：与 `unfreeze_contest` 不同，取消归档不会丢失任何快照或引起文件内容变化，属于安全可逆的元数据操作，因此允许对 MCP 开放。
4. **`get_contest`**
   - 返回结构中补齐 `archived: boolean`。

---

## 7. 验证与文档更新清单

### 7.1 自动化测试
- **`xcpc-storage` 单元测试**：
  - `duplicate_problem`：完整目录复制（含嵌套 `samples/`、`assets/`、`solutions/`）、`title` 替换且保留 `problem.toml` 注释与未知字段、重名冲突拒绝、非法 ID 拒绝、只读工作区拒绝、源目录含符号链接时拒绝且不留残留目录；
  - `set_contest_archived`：归档 / 取消归档幂等性、与 `frozen` 正交组合、归档后拦截 `compose_contest` / `update_contest_meta` / `set_override` / `freeze_contest` / `unfreeze_contest` 并返回 `contest_archived`、未定稿归档比赛仍拦截 `delete_problem`。
- **`xcpc-mcp` 集成测试**：
  - CLI `problem duplicate`、`contest archive`、`contest unarchive` 的正常与 `--json` / `--read-only` 用例；
  - MCP `duplicate_problem`、`archive_contest`、`unarchive_contest` 工具调用与错误码校验。
- **`apps/desktop` 前端测试**：
  - 复制题目弹窗默认 ID 递增逻辑、提交后选中新题；
  - 挑题弹窗筛选 + 跨筛选多选 + 已加入置灰 + 26 题上限拦截 + 批量追加；
  - 归档区块折叠/隐藏、只读态按钮切换、题库「加入比赛」过滤已归档比赛。

### 7.2 同步更新文档
实现落地时同步更新：
- [docs/product.md](../product.md)：§2 补充题库复制说明，§3 `contest.toml` 示例与规则补充 `archived` 字段与只读语义；
- [docs/design-system.md](../design-system.md)：§2.1 补充题库复制、比赛挑题弹窗与归档区块说明。
