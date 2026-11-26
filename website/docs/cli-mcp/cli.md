---
title: 命令行手册（CLI）
---

# 命令行手册（CLI）

`xcpc` 是 XCPC Workstation 的无头命令行入口，与桌面端和 `xcpc mcp` 共用同一套 Rust Core。所有题库管理、Typst 出版、题目构建与导出、算法模板验证、团队参考手册（TRD）编排、插件管理与训练统计均可在终端或 CI 流水线中完整执行。

---

## 全局选项与退出码

### 全局选项

以下选项可放在任意子命令之前或之后：

| 选项 | 简写 | 说明 |
| :--- | :--- | :--- |
| `--workspace <DIR>` | `-w <DIR>` | 指定工作区根目录。未提供时优先读取环境变量 `XCPC_WORKSPACE`，否则从当前目录向上自动探测工作区 |
| `--json` | — | 在标准输出（`stdout`）打印单个机器可读的 JSON 对象，便于脚本与自动化解析 |
| `--read-only` | — | 以只读模式打开工作区，拒绝一切写入操作（同时适用于 `xcpc mcp`） |
| `--system-fonts` | — | 在 `XCPC_FONT_DIRS` 与内嵌字体之外，额外加载操作系统已安装的字体 |

### 退出码约定

| 退出码 | 含义 | 典型场景 |
| :---: | :--- | :--- |
| `0` | 执行成功 | 命令正常完成，所有编译、校验与门禁通过 |
| `1` | 文档、校验或评测失败 | Typst 编译报错、`xcpc-lint` 存在 Error、`package build` / `snippet verify` 验收未通过、冻结阻断等 |
| `2` | 参数、工作区、I/O 或权限错误 | 参数不合法、找不到工作区、目标文件不存在、工作区处于 `--read-only`、比赛或 TRD 已冻结、冲突或未确认危险操作 |

在 `--json` 模式下，发生退出码为 `2` 的致命错误时，`stdout` 会返回统一格式的错误对象：

```json
{
  "ok": false,
  "error": {
    "code": "read_only",
    "message": "the workspace is open read-only"
  }
}
```

---

## 子命令速查（按功能域分组）

### 1. 工作区与环境

| 命令 | 用途与主要参数 |
| :--- | :--- |
| `xcpc init [dir]` | 初始化新工作区（生成 `workspace.toml`、`problems/`、`contests/`、`.gitignore`、`AGENTS.md`，不覆盖已有 `AGENTS.md`）。支持 `--example`（填充示例题库与比赛，需为空目录）、`--lang <chinese\|zh\|english\|en>`（默认 `chinese`）、`--with-skills`（安装内置出题 Skill 至 `.agents/skills/xcpc-problem-creator/`） |
| `xcpc list` | 列出当前工作区的全部题目与比赛（含已归档标记） |
| `xcpc show <problem\|contest> <id>` | 查看单题元数据、内容哈希与样例概况，或查看比赛页码分布与难度/标签统计 |
| `xcpc fonts` | 列出排版引擎当前可见的全部字体族与字重数量 |

### 2. 题库检索、分组与题目管理

| 命令 | 用途与主要参数 |
| :--- | :--- |
| `xcpc search [QUERY...]` | 检索题库。支持 `--tag <TAG>`（可重复，要求同时满足）、`--rating-min <N>`、`--rating-max <N>`、`--platform <NAME>`、`--group <NAME>`、`--ungrouped` |
| `xcpc group list` | 列出 `workspace.toml` 声明的持久化分组及未分组题目数量 |
| `xcpc group create <name>` | 创建新分组（1–64 字符） |
| `xcpc group rename <old> <new>` | 重命名分组并同步更新所有成员题目的 `problem.toml` |
| `xcpc group delete <name>` | 删除分组（成员题目转为未分组，不删除题目文件） |
| `xcpc group reorder <GROUP...>` | 按给定完整排列重排分组顺序 |
| `xcpc problem duplicate <src> <new>` | 复制题目目录至新 slug，可选 `--title <TITLE>` |
| `xcpc problem group <GROUP> <ID...>` | 批量设置题目分组；传入 `--clear <ID...>` 批量清除分组 |
| `xcpc problem tag <ID...>` | 批量增删标签：`--add <T1,T2>`、`--remove <T1,T2>` |
| `xcpc problem rating <RATING> <ID...>` | 批量设置难度 Rating；传入 `--clear <ID...>` 批量清除 |
| `xcpc problem delete <ID...>` | 批量删除题目（自动跳过被未冻结比赛引用的题目）；`--yes` 跳过确认 |
| `xcpc problem from-url <url>` | 从外部 OJ URL 创建题目（按 `[[source]]` 自动去重），可选 `--id <SLUG>`、`--title <TITLE>` |

### 3. 出版、模板与组卷

| 命令 | 用途与主要参数 |
| :--- | :--- |
| `xcpc build <problem\|contest\|problems> <ID...>` | 编译并导出 PDF（默认写入 `build/`）。支持 `--editorial`（题解册）、`--language <zh\|en>`、`-o/--output <PATH>`、`--emit-typst <FILE>`；`build problems` 额外支持 `--title <TITLE>` 与 `--cover <true\|false>` |
| `xcpc check <problem\|contest> <id>` | 执行 Typst 排版与 `xcpc-lint` 预检并输出诊断、页数与溢出页码，不写入任何文件。支持 `--editorial`、`--language <zh\|en>` |
| `xcpc render <problem\|contest> <id>` | 渲染页面为 PNG（默认输出至 `build/<id>/page-<n>.png`）。支持 `--editorial`、`--language <zh\|en>`、`-o/--out <DIR>`、`--scale <F32>`（默认 `2.0`）、`--page <N>`（1-based） |
| `xcpc template list` | 列出工作区模板（`templates/*.typ`）与内置预设种子 |
| `xcpc template schema <name>` | 查看指定工作区模板解析出的声明式 `#let schema` |
| `xcpc template add <preset> [name]` | 将内置预设（`olymp-classic`、`cnoi-classic`、`minimal`）安装至 `templates/<name>.typ` |
| `xcpc template fork <source> <new_name>` | 复制现有工作区模板为新模板 |
| `xcpc contest compose <id> [PROBLEM_ID...]` | 创建或更新比赛题目列表（按顺序分配 `A..Z`，最多 26 题）及元数据：`--name`、`--location`、`--date`、`--author`、`--language`、`--template`、`--cover`、`--blank-pages`、`--show-metadata` |
| `xcpc contest override <contest> <label>` | 设置或清除单题元数据覆写：`--title` / `--clear-title`、`--time` / `--clear-time`、`--memory` / `--clear-memory`、`--input` / `--clear-input`、`--output` / `--clear-output`、`--show-metadata` / `--clear-show-metadata` |
| `xcpc contest freeze <id>` | 预检、导出试题册/题解册 PDF 并冻结比赛至只读快照 `contests/<id>/snapshot/` |
| `xcpc contest unfreeze <id>` | 展示快照与当前题库的差异并解冻比赛（需人工确认或传入 `--yes`） |
| `xcpc contest archive <id>` | 归档比赛（标记为只读并移入归档区，与冻结状态正交） |
| `xcpc contest unarchive <id>` | 取消归档，恢复比赛原有可编辑/冻结状态 |
| `xcpc contest audit <id>` | 只读执行整场比赛验题体检（验收新鲜度、时限安全边际、数据体量、解法覆盖与 `xcpc-lint`），支持 `--markdown <PATH>` 导出报告 |
| `xcpc contest verify-external <id> <path>` | 零污染试跑外部验题人提交目录（单层或 `<dir>/<tester>/<prob>.cpp`），支持 `--auto-build`、`-j/--jobs <N>`、`--markdown <PATH>` |

### 4. 题目构建、导入与导出

| 命令 | 用途与主要参数 |
| :--- | :--- |
| `xcpc package inspect <id>` | 只读查看题目构建包的文件清单、子任务配置、测试点展开与最近验收报告新鲜度 |
| `xcpc package init <id>` | 生成题目构建脚手架（不覆盖已有文件）。支持 `--interactive`（交互题 `interactor.cpp`）、`--runs <N>`（多轮通信）、`--subtasks`（OI 子任务） |
| `xcpc package build <ID...>` | 编译组件、生成校验数据并运行解法矩阵。支持 `--strict`（全量严格验收）、`--sanitize`（隔离 ASan/UBSan 体检）、`--tests <LIST>`、`--solutions <LIST>`、`-j/--jobs <N>`、`--contest <CONTEST>`（按比赛覆写时限/内存检查） |
| `xcpc package stress <id> --gen <name>` | 有界随机对拍并将首个反例写入 `tests/stress-<n>.in`。支持 `--args <TEMPLATE>`（`{i}` / `{seed}`）、`--solutions <LIST>`、`--max-iter <N>`（默认 `100`）、`--timeout <S>`（默认 `30`） |
| `xcpc package verify-external <id> <FILE...>` | 在临时目录试跑外部 `.cpp` 解法而不改动工作区。支持 `--auto-build`、`-j/--jobs <N>`、`--markdown <PATH>` |
| `xcpc package adopt <id> <file>` | 原子化收编外部 `.cpp` 到 `solutions/` 并登记至 `problem.toml`。支持 `--name`、`--expect <V1,V2>`、`--tag <T1,T2>`、`--main` |
| `xcpc import <target> <FILE...>` | 导入外部归档：`polygon` 或 `<plugin>:<importer>`。Polygon 导入支持 `--id <SLUG>`、`--group <NAME>`、`--no-auto-group` |
| `xcpc export <format> <problem\|contest\|problems> <ID...>` | 导出数据包：`domjudge`（别名 `dj`）、`hydro`（别名 `hydrooj`）或 `<plugin>:<exporter>`。支持 `-o/--output <PATH>`、`--force`（跳过严格验收要求，仍需已构建数据）、`--with-pdf`（插件导出器附带 Base64 PDF） |

### 5. 算法模板与团队参考手册（TRD）

| 命令 | 用途与主要参数 |
| :--- | :--- |
| `xcpc snippet list` | 列出 `snippets/` 中的算法模板及验证状态。支持 `--tag <TAG>`、`--status <unverified\|passed\|failed\|stale>`、`--formula-only` |
| `xcpc snippet show <slug>` | 查看算法模板详情、依赖关系、反向引用、累计短哈希与验证报告 |
| `xcpc snippet new <slug>` | 创建新算法模板。支持 `--title`、`--tag`、`--time`、`--space`、`--description`、`--formula-only` |
| `xcpc snippet delete <slug>` | 删除算法模板目录（`--yes` 跳过确认） |
| `xcpc snippet verify [slug]` | 验证单个算法模板、下游依赖（`--cascade`）或全库（`--all`）。支持 `-j/--jobs <N>`、`--force`（忽略缓存重跑） |
| `xcpc bundle [file]` | 将本地 C++ 文件或 `--snippet <slug>` 展开为单文件自包含 C++。支持 `-I/--include-dir <DIR>`、`--compact`（去注释与空行）、`-o/--output <PATH>`、`--copy`（写入剪贴板） |
| `xcpc trd list` | 列出 `trds/` 下所有团队参考手册 |
| `xcpc trd show <id>` | 查看 TRD 章节编排、冻结状态与 preflight 诊断 |
| `xcpc trd compose <id> [SLUG...]` | 编排 TRD 章节（`--chapter "章节名:slug1,slug2"`，可重复）与元数据（`--name`、`--author`、`--date`、`--template <trd-compact\|trd-book\|...>`） |
| `xcpc trd check <id>` | 执行 TRD 依赖 preflight 与 Typst 编译检查 |
| `xcpc trd build <id>` | 编译并导出 TRD PDF，支持 `-o/--output <PATH>`、`--emit-typst <FILE>` |
| `xcpc trd render <id>` | 渲染 TRD 页面为 PNG，支持 `-o/--out <DIR>`、`--scale <F32>`、`--page <N>` |
| `xcpc trd freeze <id>` | 校验、导出 PDF 并冻结 TRD 快照；`--strict` 要求所有代码模板均已通过验证 |
| `xcpc trd unfreeze <id>` | 展示快照差异并解冻 TRD（需确认或 `--yes`） |

### 6. 训练同步、插件、工具与 MCP

| 命令 | 用途与主要参数 |
| :--- | :--- |
| `xcpc sync` | 增量同步已配置 Handle 的 OJ 平台及赛程日历。支持 `--platform <ID>`、`--full` |
| `xcpc sync handle <platform> [handle]` | 查看、设置或清除（`--clear`）指定 OJ 平台的账号 Handle |
| `xcpc training <status\|dashboard\|contests\|submissions\|log\|editorials>` | 训练看板统计与手动记录，详细参数参见[数据视图与手动记录](../training/dashboard.md) |
| `xcpc plugin <list\|install\|uninstall\|enable\|disable\|trust\|export>` | 插件全生命周期管理，参见[插件生态与开发](../plugins/index.md) |
| `xcpc plugin <init\|link\|unlink\|check\|pack\|test>` | 插件开发、热重载联调、静态/沙箱检查与打包，参见[插件开发指南](../plugins/development.md) |
| `xcpc plugin repo <list\|catalog\|add\|remove>` | 管理去中心化插件仓库（`index.json`）订阅与目录检索 |
| `xcpc tool list` / `xcpc tool run <plugin>:<tool>` | 列出或无头运行已启用插件提供的离线工具，支持 `--input key=value` 与 `--input-json <JSON>` |
| `xcpc mcp` | 在 `stdin`/`stdout` 上启动 MCP 服务器（通常配合 `-w <DIR>` 与可选 `--read-only` 使用），参见[MCP 服务接入](./mcp.md) |

---

## 典型组合示例

### 1. CI 赛前验题与导出流水线

```bash
# 1. 对整场比赛的题目执行严格验收与内存体检
xcpc -w ./contest-ws package build prob-a prob-b prob-c --strict
xcpc -w ./contest-ws package build prob-a prob-b prob-c --sanitize

# 2. 校验题面与题解排版无错误
xcpc -w ./contest-ws check contest icpc-2026 --json
xcpc -w ./contest-ws check contest icpc-2026 --editorial --json

# 3. 导出整场体检报告与 DOMjudge 数据包
xcpc -w ./contest-ws contest audit icpc-2026 --markdown ./build/audit.md
xcpc -w ./contest-ws export domjudge contest icpc-2026 -o ./build/domjudge
```

### 2. 算法模板重构、级联验证与赛场手册定稿

```bash
# 1. 修改底层模板后，级联回归所有下游依赖
xcpc snippet verify modint --cascade

# 2. 展开单文件代码并复制到剪贴板，准备提交 OJ
xcpc bundle --snippet ntt --compact --copy

# 3. 赛前严格冻结团队参考手册（确保所有代码模板均为 passed）
xcpc trd check regional-2026
xcpc trd freeze regional-2026 --strict
```

### 3. 日常训练同步与补题筛选

```bash
# 同步最新提交并筛选出 1800–2200 分尚未 AC 的 Codeforces 题目
xcpc sync --platform codeforces
xcpc training submissions --by-problem --unsolved --platform codeforces --min-rating 1800 --max-rating 2200
```
