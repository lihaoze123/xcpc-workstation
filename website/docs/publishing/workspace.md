# 工作区与开放目录

XCPC Workstation 采用**开放目录**作为唯一的事实源（Single Source of Truth）。工作区本身就是一个普通的本地文件夹，所有题目、比赛、模板与训练日志都以人类可读的纯文本 Markdown 与 TOML 存储，不包含私有二进制数据库。

## 目录结构

一个典型工作区的顶层布局如下（以根目录存在 `workspace.toml` 为标志）：

```text
my-xcpc/
├── workspace.toml            # 工作区配置（默认模板、模板参数、分组清单、模块开关等）
├── .gitignore                # 初始化时自动写入 .xcpc/ 与 build/
├── AGENTS.md                 # 工作区级 AI Agent 协作指南（初始化时写入，之后归用户维护）
├── templates/                # 工作区 Typst 模板（<name>.typ，全部可直接编辑并进 Git）
│   ├── olymp-classic.typ
│   └── preamble.typ          # 可选：跨模板共享的自定义宏与样式注入
├── assets/                   # 工作区级共享静态资源（如通用校徽、封面徽标）
├── problems/                 # 题库目录：每道题独立子目录，目录名 slug 即题目 ID
│   └── lantern-street/
│       ├── problem.toml      # 题目元数据、时限内存、[[source]]、题目包声明
│       ├── statement.md      # 题面正文（双语时为 statement.zh.md / statement.en.md）
│       ├── editorial.md      # 题解正文（排入 A4 题解册）
│       ├── samples/          # 样例目录（01.in、01.ans、02.in、02.ans ...）
│       ├── assets/           # 题目专属配图
│       ├── validator.cpp     # 可选：题目构建文件（校验器/检查器/交互器）
│       ├── checker.cpp
│       ├── generators/       # 可选：数据生成器
│       ├── solutions/        # 可选：标程与参考解
│       └── tests/            # 可选：手写测试点与自测用例
├── contests/                 # 比赛目录
│   └── welcome-round/
│       ├── contest.toml      # 比赛元信息、题目活引用列表、元数据覆写、模板参数
│       ├── template.typ      # 可选：仅对该场比赛生效的专属模板
│       ├── assets/           # 可选：比赛专属配图或封面徽标
│       └── snapshot/         # 仅定稿（冻结）后存在：源文件副本 + 模板 + 两本 PDF（进 Git）
├── snippets/                 # 可选：算法模板库（参见「算法模板与参考手册」）
├── trds/                     # 可选：团队参考手册（TRD）目录
├── training/
│   └── logs.jsonl            # 追加写入的本地训练状态日志（进 Git）
├── build/                    # 导出的 PDF、PNG、OJ ZIP 与生成的测试数据（不进 Git）
└── .xcpc/                    # 纯本地派生目录（固定写入 .gitignore，绝不进 Git）
```

## `workspace.toml` 配置

`workspace.toml` 可以为空文件，也可以按需声明工作区级默认设置：

```toml
# 工作区默认出版模板（对应 templates/<name>.typ）
template = "olymp-classic"

# 声明的持久化题目分组（有序）
groups = ["省赛候选", "日常训练", "基础练习"]

# 可选：多人协同命题仓可设为 false 以隐藏左侧「训练」导航
training = true

# 可选：设为 false 可隐藏左侧「算法模板 / TRD」导航
snippets = true

# 工作区级模板默认参数（按模板名分命名空间隔离）
[template-options.olymp-classic]
cover = "auto"
show-metadata = true
blank-pages = false
```

所有由桌面端或 CLI 触发的 TOML 写入均使用局部编辑引擎（`toml_edit`），会完整保留你在文件中手写的注释、键顺序以及未知字段。

## 单一事实源与缓存隔离

为了保证工作区在多人 Git 协作与多设备同步中保持干净，XCPC Workstation 严格区分**项目内容数据**与**个人/派生数据**：

| 数据类别 | 存放位置 | 是否进 Git | 说明 |
| :--- | :--- | :---: | :--- |
| 题目、比赛、模板、算法模板、TRD、训练日志 | 工作区目录内 | **是** | 单一事实源，支持外部编辑器直接修改，桌面端通过文件监听自动热刷新。 |
| 比赛定稿快照（`contests/<id>/snapshot/`） | 工作区目录内 | **是** | 固化定稿时的全部源文件、生效模板与 PDF，保障历史归档可永久复现。 |
| 构建产物（`build/`） | 工作区目录内 | **否** | 导出的临时 PDF、PNG、数据包 ZIP 及生成的测试数据。 |
| 本地工作区派生目录（`.xcpc/`） | 工作区目录内 | **否** | `xcpc init` 时固定写入 `.gitignore`，绝不提交到仓库。 |
| 全局外观、OJ 同步库、插件与编译缓存 | 用户全局数据/缓存目录（`<dataDir>` / `~/.cache/xcpc/`） | **否** | 包含 `<dataDir>/theme.toml`、`<dataDir>/oj_sync.db`、`<dataDir>/plugins/`、系统钥匙串凭据以及跨工作区共享的三级构建缓存。 |

!!! note "训练状态不污染题库文件"
    每道题的过题状态（`ac` / `upsolved` / `attempted` / `todo`）**绝不写入 `problem.toml`**，而是在运行时由 `training/logs.jsonl` 与全局 `<dataDir>/oj_sync.db` 实时合并派生。这样同一份题库在多人之间共享时，不会因为个人刷题记录产生无意义的 Git 冲突。

## 并发写入保护与外部编辑器联动

- **原子写入**：Core 层对工作区内所有文件的修改均采用「写临时文件 + 原子 `rename`」方式，避免异常中断导致文件损坏。
- **冲突检测**：桌面端与 MCP 在写入前会比对文件上次读取时的内容哈希；若磁盘文件已被外部工具修改，Core 会返回冲突提示而非静默覆盖。
- **文件监听热刷新**：桌面端打开工作区后会递归监听工作区文件变动（自动忽略 `build/`、`.xcpc/`、`.git/`、`.jj/` 与原子写入临时文件），覆盖 `problems/`、`contests/`、`snippets/`、`trds/`、`workspace.toml` 与 `templates/*.typ`。无论你是用 VS Code / Vim 编辑题面，还是通过 `xcpc mcp` 让 AI Agent 修改文件，桌面端都会在无闪白状态下自动刷新预览与模板参数。

## 多人 Git 协作实践

XCPC Workstation 不设云端账号或在线协同服务器；多人命题推荐直接为单场赛事或命题组建立独立的 Git 仓库：

1. **初始化赛事仓**：由命题组长执行 `xcpc init my-contest-2026`，在 `workspace.toml` 中设置 `training = false`（隐藏个人训练页），并按需通过 `xcpc template add` 安装好本场赛事使用的模板。
2. **分工独立目录**：每位出题人负责各自的 `problems/<slug>/` 目录，题目 ID（目录名 slug）创建后不再改名，天然避免目录间冲突。
3. **统一字体与 CI 门禁**：团队约定统一的 `XCPC_FONT_DIRS`（或在 CI 中运行 `xcpc check contest <id>` 与 `xcpc package build <slug> --strict`），保证合并到主分支前题面排版与题目数据全部通过检查。
4. **定稿归档**：赛前最终审校完成后，在主分支执行 `xcpc contest freeze <id>`（或在桌面端点击定稿），将 `contests/<id>/snapshot/` 连同最终 PDF 一并提交并打 Git Tag 归档。
