# Spec：工作区定位与多人 Git 协同出题

> 状态：已确认（2026-11-22）
> 关联文档：[product.md](../product.md)、[architecture.md](../architecture.md)、[roadmap.md](../roadmap.md)

## 1. 背景与问题

当前规格将 XCPC Workstation 定位为「个人工作站」，并把比赛设计为对 `problems/<slug>/` 的活引用（`contests/<id>/contest.toml` 仅存题目 ID 与元数据覆写）。这一设计在「单人维护题库并组卷」时非常简洁，但在真实的**多人基于 Git 协同出题**场景下暴露出两处张力：

1. **概念冲突**：如果把工作区理解为「每人唯一的个人工作区」，出题人既无法把整份个人题库/训练日志放进团队仓库，也无法把只含引用的 `contests/<id>/` 目录单独拿出去协作。
2. **文件边界污染**：当前设计将部分个人数据与共享项目数据混放：
   - `workspace.toml` 规划中包含了「OJ 账号」；
   - `.xcpc/` 整目录被 `init_workspace` 写入 `.gitignore`，但影响所有协作者编译结果的 `preamble.typ` 却放在 `.xcpc/preamble.typ` 下，导致默认不会随 Git 同步；
   - `.xcpc/theme.toml` 属于个人界面外观偏好，放在工作区内易与协作仓库混杂。

本规格在不破坏「本地优先、开放目录、单一模型、less is more」原则的前提下，理清工作区的定位与文件边界，使多人 Git 协同出题自然成立。

---

## 2. 核心设计决策

### 2.1 工作区是「项目目录」，而非「个人单例」

- **单一模型，不区分工作区类型**：不引入 `personal` / `contest` 等工作区类型字段或模式切换。
- **典型用法**：
  - 用户可以维护一个长期的**个人工作区**，存放个人题库、题单与训练日志；
  - 多人协同出题时，为该赛事单独创建一个 **Git 仓库作为比赛工作区**，协作者克隆后直接用 XCPC Workstation 打开（通过桌面端切换工作区或 CLI `-w`）。
- **协作分工**：各出题人主要编辑各自负责的 `problems/<slug>/` 目录（互不冲突），由组卷人维护 `contests/<id>/contest.toml` 与共享模板。

### 2.2 个人数据与项目数据彻底解耦

| 归属 | 路径 / 位置 | 是否进 Git | 说明 |
| :--- | :--- | :---: | :--- |
| **应用级（个人全局）** | `<dataDir>/theme.toml`、系统钥匙串与全局设置 | —（不在工作区内） | 外观主题、编辑器偏好、OJ 账号与凭据均属于用户个人，不写入任何工作区 |
| **工作区级（项目内容）** | `workspace.toml`、`problems/`、`contests/`（含 `snapshot/`）、`templates/`（含 `templates/preamble.typ`）、`assets/`、`xcpc.lock`、`AGENTS.md` | **是** | 决定题面、题册、题目包构建结果的全部单一事实源 |
| **工作区级（本地派生）** | `build/`、`.xcpc/`（如 `.xcpc/oj_sync.db` 等） | **否** | `init_workspace` 生成的 `.gitignore` 固定忽略 `build/` 与 `.xcpc/`，保持 `.xcpc/` 100% 本地化 |
| **按需生成的训练记录** | `training/logs.jsonl` | 按需（个人工作区进 Git） | 仅在用户主动使用训练功能时按需创建；纯协作比赛仓库不会生成此目录 |

关键调整点：
1. **`preamble.typ` 迁至 `templates/preamble.typ`**：与模板文件 `templates/<name>.typ` 同处 `templates/` 目录，天然纳入 Git 版本管理，避免协作者克隆后编译结果不一致。
2. **移除工作区级 `.xcpc/theme.toml`**：`theme.toml` 仅保留用户全局 `<dataDir>/theme.toml`，工作区不再承载 GUI 主题覆写。
3. **OJ 账号移出 `workspace.toml`**：Phase 4 接入 OJ 同步时，账号列表与应用级设置存放于用户全局配置（凭据仍在系统钥匙串），不再写入 `workspace.toml`。

### 2.3 跨工作区流转：保持仓库自包含

- **严格禁止跨工作区活引用**：`contest.toml` 只能引用当前工作区 `problems/` 下的题目，路径沙箱边界不变，确保任意比赛 Git 仓库克隆后开箱即可编译。
- **日常流转**：每道题的 `problems/<slug>/` 目录本身就是自包含单元，个人工作区与比赛仓库之间可直接通过文件系统复制流转。
- **后续轻量增强（按需）**：后续可在 Core / CLI / GUI 中提供一个极薄的「从目录导入题目/比赛」能力（将外部目录复制到当前工作区，并在题目 ID 冲突时提示重命名或拒绝覆盖），不引入专用的跨工作区同步协议或血缘追踪。

### 2.4 单比赛仓库的目录与 `contest.toml` 结构不变

- 即便一个 Git 仓库只服务于一场赛事，仍沿用统一的 `problems/<slug>/` + `contests/<id>/contest.toml` 结构，**不新增根目录扁平化变体**。这同时天然支持在同一赛事仓库内维护热身赛与正式赛（如 `contests/warmup/` 与 `contests/main/`）。
- 保留 `contest.toml` 中的显式 `label` 字段（由 Core 按顺序维护），以及冻结后将 `snapshot/`（含两本归档 PDF）纳入 Git 的既有规则。

---

## 3. 实施变更清单

### 3.1 Core 与桌面端（小规模清理）

1. **`preamble.typ` 路径迁移（`.xcpc/preamble.typ` → `templates/preamble.typ`）**：
   - `xcpc-storage`：
     - 将 preamble 读写路径改为 `templates/preamble.typ`（项目未正式发布，不做旧路径兼容回退）；
     - `list_templates()` 显式排除保留名 `preamble`（不将 `templates/preamble.typ` 视为可选比赛模板）；
     - `fork_template()` / `install_preset_template()` 拒绝目标名为 `preamble`；
     - `WorkspaceWatcher` 监听 `templates/preamble.typ` 的变更以触发预览热刷新；
     - `freeze_contest` 仍将生效的 preamble 固化为 `contests/<id>/snapshot/preamble.typ`。
   - `apps/desktop`：设置页与菜单中「打开 preamble」指向 `templates/preamble.typ`。
   - `WORKSPACE_AGENTS_MD`：更新目录树说明，明确 `templates/preamble.typ` 进 Git、`.xcpc/` 为纯本地缓存。
2. **移除工作区级 `.xcpc/theme.toml`**：
   - `apps/desktop`：仅加载 `<dataDir>/theme.toml`，移除工作区 `.xcpc/theme.toml` 的读取、监听与相关入口文案。

### 3.2 规格文档同步

- `docs/product.md`：
  - 第 1 节「定位」补充说明：应用本身不设账号体系与在线协同服务，但工作区是开放的项目目录，个人题库与多人 Git 协同比赛可分别作为独立工作区使用；
  - 确认「明确不做」仍保持「不做在线多人协作与团队训练看板」。
- `docs/architecture.md`：
  - 第 5 节「本地存储」目录树与说明：`workspace.toml` 移除「OJ 账号」；`preamble.typ` 移入 `templates/preamble.typ`；`.xcpc/` 明确为纯本地不进 Git 目录。
- `docs/roadmap.md`：
  - 同步 `templates/preamble.typ` 与全局 `theme.toml` 的表述；
  - 在「待定」中可记录轻量的「从外部目录导入题目/比赛」。

---

## 4. 明确不做

- 不引入工作区类型区分（`personal` / `contest`）或按类型裁剪界面。
- 不支持单比赛根目录扁平化格式或比赛内嵌私有题目目录（`contests/<id>/problems/`）。
- 不支持跨工作区活引用、跨仓库依赖或 Git submodule 专用集成。
- 不在应用内构建用户账号、权限、评论、在线协同或复杂三方冲突合并 UI；应用内仅提供极简的领域化纯 Git 主干闭环与离线盲验/目录流转，详见 [workspace-vcs-and-handoff.md](./workspace-vcs-and-handoff.md)。
