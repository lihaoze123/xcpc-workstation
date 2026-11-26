---
title: 训练数据同步与存储隔离
---

# 训练数据同步与存储隔离

XCPC Workstation 的训练模块面向个人长期刷题与备赛记录，采用**「外部 OJ 缓存跨工作区共享 + 工作区本地日志隔离派生」**的设计：既能自动聚合 Codeforces、AtCoder 等外部平台的提交与 Rating 走势，又绝不将个人刷题状态写入题库内容文件（如 `problem.toml`），确保题库目录可干净地用于版本控制与协作分享。

---

## 多平台增量同步

训练数据同步由内置及第三方 `oj_source` 能力插件驱动。应用内置以下两个官方 OJ 数据源插件：

- **`codeforces`**：同步账号概况、比赛 Rating 历史、提交记录与赛程日历。
- **`atcoder`**：同步账号概况、比赛 Rating 历史、提交记录与赛程日历。

如需接入更多平台，可在插件中心安装或自行开发 `oj_source` 插件（参见[插件生态与开发](../plugins/index.md)）。

### 账号 Handle 管理

每个平台维护一个主账号 Handle，保存在全局配置文件 `<dataDir>/sync_config.json` 中。既可以在桌面端「训练」空状态引导或「设置 → 插件」中配置，也可以通过 CLI 管理：

```bash
# 查看当前配置的 Handle
xcpc sync handle codeforces

# 设置 Codeforces 与 AtCoder Handle
xcpc sync handle codeforces tourist
xcpc sync handle atcoder tourist

# 清空某平台的 Handle
xcpc sync handle codeforces --clear
```

!!! info "切换或清空 Handle 时的自动清理机制"
    在桌面端切换或清空某平台的 Handle 时，同步引擎会立即清理 `<dataDir>/oj_sync.db` 中该平台的个人缓存数据（账号概况、Rating 历史、提交记录与汇总缓存），并重置增量游标 `sync_cursor`；同时保留公共赛程（`oj_contests`）与题目元数据（`oj_problem_meta`），避免不同账号的记录混杂。若通过 CLI（`xcpc sync handle`）或直接编辑 `sync_config.json` 切换为新 Handle，下次执行 `xcpc sync` 检测到账号变更时同样会自动清理旧账号数据与游标。

### 增量同步与全量重拉

配置 Handle 后，可执行增量同步：

```bash
# 增量同步全部已启用且已配置 Handle 的 OJ 平台（同时拉取赛程日历）
xcpc sync

# 仅同步指定平台
xcpc sync --platform codeforces

# 忽略增量游标，强制执行全量同步
xcpc sync --full
xcpc sync --platform atcoder --full
```

- **游标增量拉取**：每次同步完成后，插件会将增量游标（`sync_cursor`）保存在插件专属 KV 存储中；后续同步仅拉取新增提交，减少网络开销。
- **赛程日历免账号拉取**：即使尚未配置某平台的 Handle，只要对应 `oj_source` 插件处于启用状态，执行 `xcpc sync` 时仍会尽力拉取该平台的近期赛程。
- **启动自动同步与单平台隔离**：`sync_config.json` 中默认开启 `"auto_sync_on_startup": true`。多平台同步时各平台错误相互隔离，单个平台网络超时或限流不会中断其他平台的同步。

---

## 存储架构与状态派生

训练系统严格区分**跨工作区个人缓存**、**工作区本地日志**与**题库内容文件**三层存储：

| 存储位置 | 作用范围 | 文件格式 | 内容与职责 | 是否纳入题库 Git |
| :--- | :--- | :--- | :--- | :---: |
| `<dataDir>/sync_config.json` | 全局（跨工作区共享） | JSON | 各平台 Handle、`auto_sync_on_startup`、最近同步时间 `last_synced_at` | 否 |
| `<dataDir>/oj_sync.db` | 全局（跨工作区共享） | SQLite（WAL 模式） | 可重建的外部 OJ 缓存：账号、Rating 历史、提交记录、题目元数据、赛程日历 | 否 |
| `<workspace>/training/logs.jsonl` | 当前工作区 | JSON Lines（追加写入） | 手动打卡状态变更、无题库关联的离线/外站做题记录与备注 | 按需 |
| `<workspace>/problems/<id>/problem.toml` | 当前工作区 | TOML | 题目本体元数据与外部身份 `[[source]]`，**永不写入训练状态** | 是 |

### 全局数据目录（`<dataDir>`）定位规则

桌面端与 `xcpc` CLI 共用同一个 `<dataDir>`（应用标识 `dev.xcpc.workstation`），确保两端同步结果即时互通。目录解析优先级如下：

1. 若设置了非空环境变量 `XCPC_DATA_DIR`，优先使用该路径。
2. 否则按操作系统标准应用数据目录定位：
   - **Linux / 其他 Unix**：`$XDG_DATA_HOME/dev.xcpc.workstation`（未设置时回退到 `$HOME/.local/share/dev.xcpc.workstation`）
   - **Windows**：`%APPDATA%\dev.xcpc.workstation`
   - **macOS**：`$HOME/Library/Application Support/dev.xcpc.workstation`

!!! note "为什么 `oj_sync.db` 放在全局目录而非工作区内？"
    一名选手通常会维护多个工作区（例如不同赛季的个人题库、校队出题工作区、公开共享题单）。将 `oj_sync.db` 置于用户级 `<dataDir>` 后，在任意工作区打开题库时，只要题目的 `[[source]]` 与外部提交匹配，就能立刻看到自己的做题状态，且不会在工作区中生成体积庞大的二进制缓存文件。

### 运行时状态派生规则

题目的有效训练状态（`none` / `todo` / `attempted` / `ac` / `upsolved`）**不持久化在 `problem.toml` 中**，而是在运行时由以下两路数据动态合并派生：

1. **外部 OJ 同步状态**：通过题目的 `[[source]]`（`platform` 与 `id`，不区分大小写）匹配 `oj_sync.db` 中的提交记录，判定是否存在外部 `AC` 或 `attempted`。
2. **工作区本地日志**：按时间顺序扫描 `training/logs.jsonl` 中匹配当前 `problem_id` 或 `[[source]]` 的最新一条手动记录（`clear` 用于清除之前的手动覆盖）。

合并优先级为 **`ac` / `upsolved` > `attempted` > `todo` > `none`**，具体规则如下：

| 外部 OJ 同步状态 | 本地最新手动记录 | 最终派生状态 |
| :--- | :--- | :--- |
| 已有外部 `AC` | `upsolved` | `upsolved` |
| 已有外部 `AC` | 其他值或无记录 | `ac` |
| 仅有外部 `attempted`（未 AC） | `ac` 或 `upsolved` | 采用手动值（`ac` / `upsolved`） |
| 仅有外部 `attempted`（未 AC） | `none` / `todo` / `attempted` | `attempted` |
| 无外部记录 | 任意值 | 采用手动值（无记录时为 `none`） |

---

## 按工作区关闭训练模块

在纯出题工作区、团队协作工作区或无需训练追踪的场景下，可以在工作区根目录的 `workspace.toml` 中显式关闭训练模块：

```toml
# workspace.toml
training = false
```

- 未配置 `training` 字段或设为 `true` 时，训练模块默认启用。
- 设为 `training = false` 后，桌面端侧边栏将隐藏「训练」空间入口，题库列表也不再展示个人训练状态标记；该设置仅作用于当前工作区，不影响全局 `<dataDir>/oj_sync.db` 中的个人同步缓存。

---

## 下一步

- 查看四大核心数据视图、交互筛选与手动记录方法：[数据视图与手动记录](./dashboard.md)
- 查看 `xcpc sync` 与 `xcpc training` 的完整命令速查：[命令行手册（CLI）](../cli-mcp/cli.md)
