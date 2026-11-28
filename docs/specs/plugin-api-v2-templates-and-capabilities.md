# 插件 API 增强规格：排版模板扩展、工作区读写与宿主底层能力

> 状态：已确认并排入 **Phase 7（7A / 7B / 7C，目标 `v0.7.0` 与 `plugin-sdk v0.3.0`）**（2026-11-28 `/grill-me` 共识）。
> 关联文档：[plugin-development.md](../plugin-development.md)、[phase-4d-plugin-sdk-and-ecosystem.md](./phase-4d-plugin-sdk-and-ecosystem.md)、[template-deprivilege-and-render-contract.md](./template-deprivilege-and-render-contract.md)、[architecture.md](../architecture.md)、[roadmap.md](../roadmap.md)。

本文档定义 XCPC Workstation 插件体系在 Phase 4D 基础上的下一阶段能力增强，覆盖三个经对齐的核心支柱：

1. **排版模板扩展（优先）**：纯声明式 Typst 模板资源包，支持题面与 TRD 模板、配套静态资源与按需加载字体、活引用命名空间、一键复制到工作区以及比赛冻结快照；
2. **工作区与内容扩展**：统一补全 QuickJS 与 WebView 两侧的 `workspace_read` 只读投影，并新增受控领域级写权限 `workspace_write`；
3. **宿主底层能力补齐**：复用内置 `xcpc-engine` + MiTeX 的 Typst/数学片段渲染、内存 ZIP 归档读写与 SHA-256，以及 WebView 二进制文件导出与多文件选择。

---

## 0. 总体设计原则与范围边界

- **做减法（Less is More）**：每个新增扩展点与宿主 API 均取最小高价值集合，不引入冗余配置或占位控件；
- **安全边界在 Core 层强制执行**：所有权限校验、路径沙箱、冻结比赛保护、`--read-only` 限制、体积与超时上限均在 `crates/` 中强校验，桌面端与 MCP 只是外壳；
- **现有插件零破坏**：所有新增字段与能力均为向后兼容扩展，现有 `oj_source` / `tool` / `commands` / `importer` / `exporter` 插件无需修改即可继续运行；
- **明确不做**：
  - 不为模板插件引入 QuickJS 编译期钩子或自定义 Markdown/Typst AST 改写器；
  - 不向插件暴露原始文件系统路径，不开放删除题目/比赛或比赛解冻（`unfreeze`）等破坏性写操作；
  - 不向插件开放剪贴板读取（`clipboard.readText` / `clipboard.readImage`）或 `openExternal`；
  - 不在宿主维护自定义 UI 组件库，也不开放新的主界面侵入式 UI 插槽。

---

## 1. 支柱一：排版模板扩展（Template Plugins）

### 1.1 运行形态与 `manifest.toml` 声明

模板插件采用**纯声明式 Typst 资源包**形态，复复用现有 `#let schema = (...)` 声明式选项面板机制：

- 新增能力枚举：`PluginCapability::Template`（`capabilities = ["template"]`）；
- 当插件仅包含 `"template"` 能力时，可不提供 `index.js`（`entry` 校验在无脚本能力且未显式提供文件时豁免）；
- 纯模板插件若未声明 `domains` / `credentials` / `workspace_read` / `workspace_write`，**不触发 `pending_trust` 信任弹窗**（Typst 编译在虚拟 `World` 沙箱中执行，无网络、无宿主机任意文件读写）。

`manifest.toml` 新增字段：

```toml
id = "icpc-asia-pack"
name = "ICPC 亚洲区域赛模板包"
version = "0.1.0"
description = "包含区域赛题面与团队参考手册模板"
capabilities = ["template"]

# 可选：配套字体目录（相对插件根目录，仅加载 .ttf / .otf / .ttc）
font_dirs = ["fonts"]

[[templates]]
id = "ec-final"                        # 模板标识（^[a-z0-9][a-z0-9_-]{0,63}$），插件内唯一
entry = "templates/ec-final.typ"       # 相对插件根目录的 .typ 入口文件
kind = "statement"                     # 可选："statement" | "trd"，默认按 #let schema.kind 推导，缺省 statement
name = "ICPC EC-Final 题面"            # 可选展示名（缺省回退至 #let schema.name 或 id）
description = "双栏页眉与比赛徽标布局"   # 可选描述（缺省回退至 #let schema.description）

[[templates]]
id = "icpc-trd"
entry = "templates/icpc-trd.typ"
kind = "trd"
```

约束与校验：
- `[[templates]]` 中每个 `id` 必须符合标识符规则且在同插件内唯一；
- `entry` 必须为相对路径、以 `.typ` 结尾、不得逃逸插件目录（含 `SYMLINK_ESCAPE` 检查）；
- `font_dirs` 中每个目录必须位于插件根内；
- **包体积与字体上限**（防止巨型字体包拖垮内存与分发）：
  - 单个字体文件上限：`25 MiB`；
  - 单个模板插件内所有资源（含字体）解压后总大小上限：`64 MiB`；
  - 由 `xcpc plugin check`、`xcpc plugin pack` 与 `PluginManager::install` 共同强制校验。

### 1.2 虚拟 `World` 挂载与按需字体加载（`xcpc-engine`）

在 `crates/xcpc-engine` 中扩展虚拟路径解析，使插件模板拥有独立的只读根挂载点，避免与内置 `/templates/*.typ` 或工作区文件冲突：

- **虚拟路径映射**：启用中的模板插件 `<plugin-id>` 目录只读挂载到 `/plugins/<plugin-id>/...`；
  - 模板入口 `/plugins/<plugin-id>/<entry>` 可使用相对路径 `#import "./common.typ": *`、`#image("../assets/logo.svg")` 或 `#raw(theme: "../assets/theme.tmTheme")`；
  - 插件模板同样允许 `#import "/templates/defaults.typ": *`、`#import "/templates/markdown.typ": *` 等内置公共辅助模块；
  - `Engine::read_disk` 对 `/plugins/<plugin-id>/...` 实施严格根目录隔离（`canonicalize` 前缀校验），严禁通过 `..` 或符号链接越出该插件根目录，且禁止跨插件读取其他插件目录。
- **按需字体加载**：
  - 常规编译不加载未使用的插件字体；
  - 仅当当前编译目标选用某个 `plugin:<plugin-id>/<tpl-id>` 且该插件声明了 `font_dirs` 时，`Engine` 才按需扫描并附加该插件的字体（按插件 `(plugin_id, version/mtime)` 做缓存，插件热重载或卸载时失效）。

### 1.3 模板引用命名空间、发现与优先级

- **引用格式**：`plugin:<plugin-id>/<tpl-id>`（如 `template = "plugin:icpc-asia-pack/ec-final"`），适用于：
  - `contest.toml` 的 `template` 字段；
  - `trd.toml` 的 `template` 字段；
  - `xcpc build/render --template` 覆写与单题预览模板选择。
- **与现有来源并存**（保持与 [template-deprivilege-and-render-contract.md](./template-deprivilege-and-render-contract.md) 一致的「去预设特权」模型）：
  - 工作区模板：`templates/<name>.typ`（含初始化时从 `olymp-classic`、`cnoi-classic`、`minimal`、`trd-book`、`trd-compact` 等预设种子落盘的文件，ID 为 `<name>`）；
  - 本场/本册专属：`contests/<id>/template.typ`、`trds/<id>/template.typ`（ID 为 `local`）；
  - 插件模板：`plugin:<plugin-id>/<tpl-id>`。
- **`TemplateInfo` 扩展**（`crates/xcpc-domain`）：
  - 增加 `source: TemplateSource`（`Workspace | Local | Plugin { plugin_id, plugin_name, template_id }`），前端模板选择器据此分组展示（工作区模板 / 本场专属 / 插件提供）并在下拉项显示来源标签。
- **缓存失效**：`xcpc-app` 的模板列表与 schema 缓存监听插件变更事件（安装、卸载、启用/禁用、热重载），自动失效重算。

### 1.4 缺失处理、一键复制到工作区与冻结快照

1. **插件缺失或禁用时的明确错误**：
   - 当工作区/比赛引用 `plugin:<plugin-id>/<tpl-id>`，但插件未安装、已禁用或该模板 ID 不存在时，Core 返回结构化错误码 `PLUGIN_TEMPLATE_NOT_FOUND`（携带 `plugin_id` 与 `template_id`）；
   - 桌面端展示清晰提示（指明缺失的插件 ID），并提供两个快捷操作：「打开插件仓库查找/安装」与「临时切换为其他模板」。
2. **一键「复制到工作区」（Eject）**：
   - Core 提供 `eject_plugin_template(plugin_id, template_id, target_name?)` 操作（暴露给 GUI 与 CLI）：
     - 若模板为单文件，复制到 `<ws>/templates/<target_name>.typ`；
     - 若模板依赖插件内静态资源或子模块，将相关文件复制到 `<ws>/templates/<target_name>/` 并生成薄入口 `<ws>/templates/<target_name>.typ`（或重写相对引用），使工作区完全自包含、脱离插件依赖；
     - 可选自动将当前比赛/TRD 的 `template` 字段更新为 `<target_name>`。
3. **比赛冻结快照（`snapshot/`）**：
   - 冻结使用插件模板的比赛时，除现有源文件与 PDF 外，将所引用的插件模板目录（`.typ`、静态资源及声明的字体）一并拷贝进 `contests/<id>/snapshot/template/`，并在冻结清单中记录来源插件 ID 与版本；
   - 保证冻结后的比赛即使在无插件环境下仍可完整审计与复现。

---

## 2. 支柱二：工作区与内容扩展（Workspace Read & Write）

### 2.1 统一补全只读投影（`workspace_read = true`）

将 QuickJS `host.workspace` 与 WebView `window.xcpc.workspace` 的只读能力拉齐，共用 `crates/xcpc-plugin/src/workspace.rs` 的领域模型，**绝不暴露宿主机绝对路径**：

| 方法（QuickJS 同步 / WebView Promise） | 说明 |
| :--- | :--- |
| `listProblems()` | 返回 `WorkspaceProblemSummary[]`（补齐 `rating`、`time_limit`、`memory_limit`、`sample_count`、`has_editorial`） |
| `getProblem(id)` | 返回 `WorkspaceProblemDetail`（补齐 `samples: { input, output }[]`、`time_limit`、`memory_limit`、`rating`、`source`、`statement_md`、`editorial_md`） |
| `listContests()` | 返回 `WorkspaceContestSummary[]`（含 `id`、`title`、`subtitle`、`problem_count`、`frozen`、`template`） |
| `getContest(id)` | 返回 `WorkspaceContestDetail`（含比赛元数据、有序题目列表及题号标签 `A/B/C...`） |
| `listSnippets()` | 返回 `WorkspaceSnippetSummary[]`（含 `id`、`title`、`category`、`tags`、`language`） |
| `getSnippet(id)` | 返回 `WorkspaceSnippetDetail`（含源码 `code`、复杂度说明、关联文档/用法说明） |
| `listTrds()` | 返回 `WorkspaceTrdSummary[]`（含 `id`、`title`、`template`、`section_count`） |
| `getTrd(id)` | 返回 `WorkspaceTrdDetail`（含章节结构与引用的 snippet ID 列表） |

权限校验：未声明 `permissions.workspace_read = true`、插件被禁用或处于 `pending_trust = true` 时，统一返回 `PERMISSION_DENIED` / `PLUGIN_UNTRUSTED`。

### 2.2 新增受控领域级写权限（`workspace_write = true`）

在 `manifest.toml` 的 `[permissions]` 中新增：

```toml
[permissions]
workspace_read = true
workspace_write = true
```

安全与信任规则：
- `requires_trust(manifest)` 将 `workspace_write` 纳入敏感权限；
- 权限升级（如从仅 `workspace_read` 升级到含 `workspace_write`）自动重置 `pending_trust = true`；
- 在 `--read-only` 模式或未打开工作区时，所有写接口统一返回 `WORKSPACE_READ_ONLY`；
- 写入一律经由 `xcpc-storage` / `xcpc-app` Core 原子写（临时文件 + rename），**严格保留 `problem.toml` 未知字段**；
- 若目标题目隶属已冻结比赛，仍允许编辑题库中的源题目（活引用模型，不影响 `snapshot/`）；任何试图修改冻结比赛快照的行为均被 Core 拒绝（`CONTEST_FROZEN`）。

**精简实用写接口集合**（QuickJS `host.workspace.*` 与 WebView `window.xcpc.workspace.*` 同名对齐）：

1. `createProblem(draft: CreateProblemInput): WorkspaceProblemDetail`
   - 校验 slug 合法性与唯一性（重复返回 `ALREADY_EXISTS`）；
   - 创建 `problems/<id>/problem.toml`、`statement.md`（可选 `editorial.md`）及 `samples/`；
   - 触发工作区索引增量刷新。
2. `updateProblem(id: string, patch: UpdateProblemPatch): WorkspaceProblemDetail`
   - 局部更新字段：`title?`、`tags?`、`difficulty?`、`rating?`、`time_limit?`、`memory_limit?`、`statement_md?`、`editorial_md?`；
   - 未传入字段保持原值，保留 `problem.toml` 中所有未知字段。
3. `setSamples(problemId: string, samples: DraftSample[]): { count: number }`
   - 原子替换目标题目的 `samples/`（校验数量与单文件体积上限，如最多 64 组、单文件 ≤ 2 MiB）。
4. `upsertSnippet(input: UpsertSnippetInput): WorkspaceSnippetDetail`
   - 创建或更新算法模板（`id`、`title`、`category`、`tags`、`language`、`code`、`notes?`），原子写入并刷新算法库索引。

明确不开放：`deleteProblem`、`deleteSnippet`、比赛创建/删除/冻结/解冻、任意文件读写。

---

## 3. 支柱三：宿主底层能力补齐

### 3.1 Typst / LaTeX 数学片段渲染

复用内置 `xcpc-engine`（含内嵌字体与 MiTeX），无需插件打包任何数学排版引擎：

- **QuickJS（`host.render`）与 WebView（`window.xcpc.render`）**：
  - `renderTypst(source: string, opts?: { format?: "svg" | "png"; scale?: number; widthPt?: number; theme?: "light" | "dark" }): { svg?: string; png_base64?: string; width: number; height: number }`
  - `renderMath(latex: string, opts?: { display?: boolean; format?: "svg" | "png"; scale?: number; theme?: "light" | "dark" }): { svg?: string; png_base64?: string; width: number; height: number }`
- **安全与资源护栏**：
  - 运行在独立无磁盘访问的纯内存 `Engine` 实例中（禁止读取工作区或宿主机文件，仅允许访问内置 `/templates/mitex/*` 与 `/templates/defaults.typ`）；
  - 单次输入源码长度上限（如 `64 KiB`）、编译超时限制（如 `2000ms`）、渲染尺寸上限（与现有 `svgToPng` 像素上限对齐），防止恶意或死循环 Typst 源码阻塞主线程。

### 3.2 内存 ZIP 归档读写与 SHA-256

服务于 `importer` / `exporter` 与数据处理工具，免去插件内联重型 JS 压缩库：

- **QuickJS（`host.archive` 与 `host.crypto`）**：
  - `host.archive.listZip(zipBase64: string): ZipEntryMeta[]`（返回 `{ path, size, is_dir }`）
  - `host.archive.readZip(zipBase64: string, opts?: { paths?: string[]; asText?: boolean }): ZipEntryData[]`（按需解包指定条目或全部条目，返回 `{ path, text?, content_base64? }`）
  - `host.archive.createZip(entries: { path: string; text?: string; content_base64?: string }[]): string`（返回 ZIP 的 Base64）
  - `host.crypto.sha256(input: string, encoding?: "utf8" | "base64"): string`（返回小写十六进制摘要）
- **安全护栏**：
  - 严格校验条目路径（拒绝绝对路径、`..` 与空路径，防 Zip Slip）；
  - 限制输入压缩包大小（如 `≤ 20 MiB`）、解压总大小（防 ZIP Bomb，如 `≤ 50 MiB`）与最大条目数（如 `≤ 1000`）。

### 3.3 WebView 文件导出与多选增强（`window.xcpc.files`）

补齐现有 `files.saveText` / `files.savePng` / `files.pickFile` 的空白：

- `files.saveBinary(defaultName: string, base64: string, options?: { title?: string; extensions?: string[] }): Promise<string | null>`
  - 弹出原生保存对话框，将解码后的任意二进制（PDF、ZIP、SVG 等）原子写入用户选定路径（大小上限 `50 MiB`）。
- `files.pickFiles(options?: XcpcPickFileOptions & { multiple?: boolean; maxBytes?: number }): Promise<XcpcPickedFile[]>`
  - 支持用户一次选择多个文件（默认单文件上限 `10 MiB`，总大小上限 `30 MiB`），同时返回 `name`、`base64`（UTF-8 文本文件额外提供 `content`）。

---

## 4. SDK、CLI 与文档同步清单

按仓库「插件文档与代码同步维护」规范，落地时需同步更新以下位置：

1. **`packages/plugin-sdk`**（升级至 `v0.3.0`）：
   - `manifest.schema.json`：新增 `template` 能力、`templates`、`font_dirs`、`permissions.workspace_write`；
   - `host.d.ts`：新增扩展后的 `workspace` 读写契约、`render.renderTypst` / `renderMath`、`archive.*`、`crypto.sha256`；
   - `ui.d.ts`：同步 `window.xcpc.workspace` 读写方法、`window.xcpc.render`、`window.xcpc.files.saveBinary` / `pickFiles`；
   - `README.md` 与 `xcpc-plugins/README.md`、`xcpc-plugin-sdk.tgz`（发布时同步）。
2. **`xcpc plugin` CLI（`crates/xcpc-mcp/src/main.rs`）**：
   - `xcpc plugin init <dir> --kind template`：生成最小可运行的模板插件脚手架（`manifest.toml` + `templates/sample.typ` 含 `#let schema` + 示例资源）；
   - `xcpc plugin check <dir>`：校验 `[[templates]]` 入口存在性、路径沙箱、Typst 编译与 `#let schema` 提取、`font_dirs` 与包体积上限；
   - `xcpc plugin pack <dir>`：打包模板与配套资源/字体，跳过无关临时文件并校验体积上限。
3. **文档同步**：
   - 内部权威文档：`docs/plugin-development.md`（并通过 `python3 scripts/check-plugin-docs.py`）；
   - 对外双语文档站：`website/docs/plugins/` 与 `website/docs-en/plugins/`（遵守正式克制的技术手册风术语规范）。

---

## 5. Phase 7 分阶段实施路线

- **Phase 7A：排版模板扩展（最高优先级）**
  - `xcpc-plugin` 清单扩展与体积/路径校验 → `xcpc-engine` `/plugins/<id>/...` 虚拟挂载与按需字体加载 → `xcpc-convert` / `xcpc-app` 模板发现、`plugin:` 解析、Eject 复制与冻结快照 → 桌面端模板选择器分组与缺失引导 → `xcpc plugin init --kind template` / `check` / `pack` 与全套测试/文档。
- **Phase 7B：工作区读写扩展**
  - 统一 `WorkspaceReader` / `WorkspaceWriter` trait（`xcpc-plugin`）与 Core 实现（`xcpc-app`）→ QuickJS 与 WebView 桥接接入 → `workspace_write` 信任门与冻结/`--read-only` 单测 → SDK 与文档更新。
- **Phase 7C：宿主底层能力补齐**
  - `xcpc-engine` 纯内存片段渲染接口 → `host.render.{renderTypst,renderMath}`、`host.archive.*`、`host.crypto.sha256` → WebView `render` 与 `files.{saveBinary,pickFiles}` → 边界/防炸弹单测与 SDK/文档更新；收口统一发布 `plugin-sdk v0.3.0` 与 `v0.7.0`。
