---
title: MCP 服务接入
---

# MCP 服务接入

`xcpc mcp` 通过标准输入输出（`stdio`，换行分隔的 JSON-RPC 2.0）提供原生 [Model Context Protocol (MCP)](https://modelcontextprotocol.io/) 服务，支持协议版本 `2025-11-25`、`2025-06-18`、`2025-03-26` 与 `2024-11-05`。

接入后，支持 MCP 的 AI 客户端（如 Claude Desktop、Cursor、Codex 等）可直接以结构化方式检索题库、编排比赛、编译诊断 Typst 题面、查看页面渲染图、执行题目构建与严格验收、维护算法模板与团队参考手册（TRD）以及查询训练看板。

---

## 客户端接入配置

接入前请先确认 `xcpc` 可执行文件已在 `PATH` 中（或在配置中填写其绝对路径），并将 `/path/to/your/workspace` 替换为实际的工作区路径。

=== "Claude Desktop"

    编辑 Claude Desktop 配置文件（macOS：`~/Library/Application Support/Claude/claude_desktop_config.json`；Windows：`%APPDATA%\Claude\claude_desktop_config.json`；Linux：`~/.config/Claude/claude_desktop_config.json`）：

    ```json
    {
      "mcpServers": {
        "xcpc": {
          "command": "xcpc",
          "args": ["-w", "/path/to/your/workspace", "mcp"]
        }
      }
    }
    ```

=== "Cursor"

    在项目根目录创建 `.cursor/mcp.json`（或在 Cursor 全局 MCP 设置中配置）：

    ```json
    {
      "mcpServers": {
        "xcpc": {
          "command": "xcpc",
          "args": ["-w", "/path/to/your/workspace", "mcp"]
        }
      }
    }
    ```

=== "Codex"

    在 `~/.codex/config.toml` 中添加：

    ```toml
    [mcp_servers.xcpc]
    command = "xcpc"
    args = ["-w", "/path/to/your/workspace", "mcp"]
    ```

!!! tip "只读接入模式（`--read-only`）"
    如果仅希望 AI 协助阅读题库、审查题面、查看渲染图、执行整场体检或分析训练数据，而不允许任何写入操作，请在 `args` 中追加 `"--read-only"`：

    ```json
    "args": ["-w", "/path/to/your/workspace", "--read-only", "mcp"]
    ```

    在 `--read-only` 模式下，所有写入型与构建执行型工具（如 `write_problem`、`export_pdf`、`build_package`、`write_snippet`、`set_training_status` 等）均会被 Core 层拦截并返回 `read_only` 错误码；`manage_groups` 仅放行 `action: "list"`，`run_external_solution` 仅放行 `auto_build: false`。

---

## 安全边界设计

所有安全边界均在 Rust Core 层强制执行，不依赖 AI 客户端提示词约束：

1. **禁止通过 MCP 解冻（Unfreeze）**：
   - `unfreeze_contest` 与 `unfreeze_trd` **从未暴露**为 MCP 工具（直接调用会返回 JSON-RPC `-32602` 未知工具错误）。
   - 比赛或 TRD 一旦冻结即进入只读状态，解冻必须由人工在桌面端 GUI 或终端 `xcpc contest unfreeze` / `xcpc trd unfreeze` 中二次确认执行。
2. **工作区路径沙箱与写入白名单**：
   - 所有写入操作均采用「临时文件 + 原子重命名（atomic rename）」，防止半写入损坏文件。
   - 严禁路径穿越（`..` 或越出工作区根目录的符号链接）；`write_problem` 与 `write_snippet` 仅允许写入白名单内的相对路径（如 `problem.toml`、`statement*.md`、`editorial.md`、`samples/*`、`solutions/*`、`generators/*`、`tests/*`、`checker.cpp`、`validator.cpp`、`interactor.cpp` 等）。
   - `bundle_code` 严格限制在工作区内部路径或算法模板 slug 内展开，不可读取宿主机任意文件。
   - `import_polygon` 强制检查 `.zip` 扩展名、Zip Slip 与解压体量上限，并拒绝覆盖已有题目 slug。
3. **并发写冲突检测与引用保护**：
   - `write_problem` 支持传入 `expected_hash`（SHA-256），当磁盘文件已被外部编辑器修改时拒绝覆盖并返回 `conflict`。
   - `batch_delete_problems` 自动跳过被未冻结比赛引用的题目并返回 `problem_referenced`。

---

## 完整 MCP 工具清单（共 41 个）

下表列出 `xcpc mcp` 当前暴露的全部 41 个工具，按功能模块分组；「只读」列对应工具的 `readOnlyHint` 声明与 `--read-only` 放行策略。

### 1. 排版、预览与模板（6 个）

| 工具名 | 只读 | 说明与核心参数 |
| :--- | :---: | :--- |
| `compile_and_diagnose` | 是 | 编译题目、比赛或 TRD（`target: "problem" \| "contest" \| "trd"`, `id`, 可选 `book: "statements" \| "editorial"`, `language`），返回编译错误、`xcpc-lint` 诊断、页数与溢出页码 |
| `render_page_png` | 是 | 将指定页面渲染为 Base64 PNG 供视觉检查（`target`, `id`, 可选 `book`, `language`, `page` 默认 `1`, `scale` 默认 `1.5`） |
| `export_pdf` | 否 | 编译并导出题目、比赛或 TRD 的 PDF 到工作区（`target`, `id`, 可选 `book`, `language`, `output`）；存在编译或 lint 错误时不写入 |
| `list_templates` | 是 | 列出工作区 Typst 模板（`templates/*.typ`）、声明式 `#let schema` 字段及可安装的内置预设种子 |
| `add_template` | 否 | 安装内置预设（`preset: "olymp-classic" \| "cnoi-classic" \| "minimal" \| "trd-compact" \| "trd-book"`, 可选 `name`）至 `templates/`，不覆盖已有文件 |
| `set_template_options` | 否 | 更新模板选择与命名空间选项（`scope: "workspace" \| "contest" \| "problem"`, 可选 `id`, `template`, `options`） |

### 2. 题库检索与管理（7 个）

| 工具名 | 只读 | 说明与核心参数 |
| :--- | :---: | :--- |
| `search_problems` | 是 | 按关键词、标签、Rating 区间、平台或分组检索题库元数据摘要（`query`/`text`, `tags`, `rating_min`, `rating_max`, `platform`, `group`, `ungrouped`） |
| `get_problem` | 是 | 读取单题元数据、`statement*.md` / `editorial.md` 全文与 SHA-256 哈希、样例及 `solutions/` 文件清单（`id`） |
| `write_problem` | 否 | 原子创建或更新题目白名单文件（`id`, `files`, 可选 `expected_hash` 并发校验） |
| `duplicate_problem` | 否 | 复制题目目录至新 slug（`source_id`, `new_id`, 可选 `title`），保留 `problem.toml` 注释与未知字段 |
| `manage_groups` | 视操作 | 管理持久化分组（`action: "list" \| "create" \| "rename" \| "delete" \| "reorder"`, 按需传 `name`/`new_name`/`order`）；仅 `list` 允许在 `--read-only` 下执行 |
| `batch_update_problems` | 否 | 批量更新多题的分组、标签与 Rating（`ids`, 可选 `group`, `tags_add`, `tags_remove`, `rating`） |
| `batch_delete_problems` | 否 | 批量删除题目（`ids`），自动跳过被未冻结比赛引用的题目 |

### 3. 组卷、覆写与定稿（6 个）

| 工具名 | 只读 | 说明与核心参数 |
| :--- | :---: | :--- |
| `get_contest` | 是 | 省略 `id` 时列出全部比赛；提供 `id` 时返回比赛元数据、冻结状态、各题覆写、编译页码范围及难度/标签统计 |
| `compose_contest` | 否 | 创建或更新比赛有序题单（`A..Z`，最多 26 题）及元数据（`id`, `problems`, 可选 `name`, `location`, `date`, `author`, `language`, `template`, `show-metadata`, `cover`, `blank-pages`） |
| `set_override` | 否 | 设置或清除比赛内单题的元数据覆写（`contest`, `label`, `fields: { title, limits, show-metadata, options }`） |
| `freeze_contest` | 否 | 校验、导出快照 PDF 并冻结比赛为只读归档（`id`）；存在编译或 lint 错误时拒绝冻结 |
| `archive_contest` | 否 | 将比赛标记为已归档（只读，与冻结状态正交，`id`） |
| `unarchive_contest` | 否 | 取消比赛归档标记，恢复原有状态（`id`） |

### 4. 题目构建、验收与数据包流转（9 个）

| 工具名 | 只读 | 说明与核心参数 |
| :--- | :---: | :--- |
| `inspect_package` | 是 | 只读查看题目构建包全景：文件清单、交互/多轮通信/OI 子任务配置、展开测试点、验收新鲜度、门禁状态与下一步建议（`id`） |
| `inspect_test_case` | 是 | 安全查看指定测试点的生成命令、文件大小、行数与有界首尾预览（`id`, `test`, 可选 `head_lines`, `tail_lines`），避免撑爆上下文 |
| `init_package` | 否 | 生成题目构建脚手架，不覆盖已有文件（`id`, 可选 `interactive`, `runs`, `subtasks`） |
| `build_package` | 否 | 编译组件并执行测试矩阵（`id`, `mode: "debug" \| "verify" \| "sanitize"`, 可选 `tests`, `solutions`, `jobs`, `contest`） |
| `stress_package` | 否 | 有界随机对拍（支持传统题、交互题与多轮通信），首个反例落盘 `tests/stress-<n>.in`（`id`, `gen`, 可选 `args`, `solutions`, `max_iter`, `timeout`） |
| `audit_contest` | 是 | 只读执行整场比赛验题体检，聚合验收新鲜度、时限余量、数据体量、解法覆盖与 `xcpc-lint` 风险标记（`id`） |
| `run_external_solution` | 是 | 在临时目录零污染试跑外部 `.cpp` 或验题人目录（`target: "problem" \| "contest"`, `id`, `path`/`paths`, 可选 `auto_build`, `jobs`）；`auto_build: true` 在 `--read-only` 下被拒绝 |
| `import_polygon` | 否 | 导入 Polygon 题目或比赛 `.zip` 归档（`zip_path`, 可选 `id`, `group`, `no_auto_group`） |
| `export_oj_package` | 否 | 导出 DOMjudge / Hydro 最小集合 `.zip`（`format: "domjudge" \| "hydro"`, `target: "problem" \| "contest" \| "problems"`, `id`/`ids`, 可选 `output`, `force`） |

### 5. 训练看板与个人记录（5 个）

| 工具名 | 只读 | 说明与核心参数 |
| :--- | :---: | :--- |
| `get_training_dashboard` | 是 | 读取融合 `oj_sync.db` 与当前工作区的训练统计（`window: "30d" \| "90d" \| "year" \| "all"`, 可选 `heatmap_year`, `tag_min_rating`） |
| `set_training_status` | 否 | 向 `training/logs.jsonl` 追加工作区题目的手动状态变更并返回派生结果（`problem_id`, `status`, 可选 `platform` + `source_id`） |
| `ingest_problem` | 否 | 按 URL 或 `(platform, problem_key)` 将外部 OJ 题目收入工作区题库，自动去重并补全元数据（可选 `id`, `title`, `tags`, `rating`, `initial_status`） |
| `log_training_record` | 否 | 追加手动训练记录（支持工作区题目或独立外站/离线条目，可选 `create_in_library: true` 同步入库） |
| `list_training_editorials` | 是 | 检索工作区包含非空 `editorial.md` 的题目列表及摘要（可选 `search`, `tag`, `min_rating`, `max_rating`） |

### 6. 算法模板与团队参考手册（TRD）（8 个）

| 工具名 | 只读 | 说明与核心参数 |
| :--- | :---: | :--- |
| `search_snippets` | 是 | 按关键词、标签、验证状态（`unverified`/`passed`/`failed`/`stale`）或纯公式卡（`formula_only`）检索算法模板 |
| `get_snippet` | 是 | 读取算法模板完整元数据、`code.hpp`、`notes.md`、测试文件、依赖 DAG、反向引用、累计短哈希与验证报告（`slug`） |
| `write_snippet` | 否 | 原子创建或更新算法模板元数据与白名单文件（`slug`, 元数据字段及 `files`） |
| `verify_snippet` | 是 | 编译运行算法模板的三源验证（`self`/`problem`/`library_checker`），支持单条目、`cascade: true` 级联验证或 `all: true` 全库验证（不修改源文件） |
| `bundle_code` | 是 | 将算法模板 `slug`、工作区相对 `path` 或内联 `source` 展开为单文件自包含 C++（可选 `include_dirs`, `compact`），严格沙箱化在工作区内 |
| `get_trd` | 是 | 省略 `id` 时列出全部 TRD；提供 `id` 时返回章节编排、冻结状态与 preflight 诊断 |
| `compose_trd` | 否 | 创建或更新 TRD 章节列表与元数据（`id`, `chapters`, 可选 `name`, `author`, `date`, `template`），拒绝修改已冻结 TRD |
| `freeze_trd` | 否 | 校验、导出 PDF、归档源码与模板快照并冻结 TRD（`id`, 可选 `strict: true` 要求全部代码模板已通过验证） |

---

## 下一步

- 搭配官方 Skill 让 AI 按标准工程流程调用上述工具：[官方 AI Skill 指南](./skills.md)
- 查看无头终端命令与退出码规范：[命令行手册（CLI）](./cli.md)
