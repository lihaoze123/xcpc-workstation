# 组卷与定稿归档

在 XCPC Workstation 中，比赛（`contests/<id>/contest.toml`）是题库题目的有序组合。未定稿时采用**活引用**，确保题面勘误自动同步；赛前印刷定稿时通过**冻结快照**将源文件、模板与 PDF 永久固化，实现历史赛事的完全只读复现。

## 活引用组卷与元数据覆写

### `contest.toml` 结构示例

```toml
name = "ICPC 2026 Regional"
location = "上海"
date = "2026 年 11 月 20 日"
author = "命题组"
language = "chinese"              # chinese | english
template = "olymp-classic"
frozen = false
archived = false

[template-options.olymp-classic]
cover = "auto"
blank-pages = true

[[problem]]
id = "lantern-street"
label = "A"

[[problem]]
id = "matrix-walk"
label = "B"
title = "矩阵漫步"                 # 仅覆写本场比赛中的标题

[problem.limits]
time = "2s"                       # 仅覆写本场比赛题面中的时限

[problem.options.olymp-classic]
sample-layout = "wide"            # 单题级模板参数覆写
```

### 活引用规则与覆写边界

- **自动同步**：题库中修改某道题的错别字或样例后，所有引用该题且**未冻结**的比赛都会立即同步更新。
- **题号自动重排**：在桌面端拖拽或通过 CLI 调整题目顺序后，Core 自动按序重排为 `A`、`B`、`C`……最多支持 26 道题（`A`–`Z`）。
- **允许覆写的范围**：
  - 元数据：`title`、`limits`（`time` / `memory` / `input` / `output`）
  - 单题级模板参数：`[problem.options.<template>]`（如 `show-metadata`、`sample-layout`、`prob-type`）
- **不允许覆写的范围**：**题面正文、样例与配图不可在比赛层覆写**。如果某场比赛需要修改题面或样例形成独立变体，请先在题库中复制题目（桌面端「复制题目」或 `xcpc problem duplicate <src> <new>`），再在比赛中引用新题。
- **时限覆写的影响边界**：比赛中的 `limits` 覆写仅影响题面出版；题目构建验收默认以 `problem.toml` 自身的限制为准（也可在构建时显式传 `--contest <id>` 按比赛覆写限值测试）。

### CLI 组卷与覆写命令

```bash
# 创建或更新比赛题目列表与元信息
xcpc contest compose spring-2026 lantern-street matrix-walk stamp-collecting \
  --name "2026 春季校赛" \
  --date "2026 年 4 月 18 日" \
  --language chinese \
  --template olymp-classic \
  --blank-pages true

# 设置或清除某题在本场比赛中的元数据覆写
xcpc contest override spring-2026 B --title "矩阵漫步" --time "2s" --show-metadata true
xcpc contest override spring-2026 B --clear-title --clear-time

# 查看比赛题目列表、每题起止页码与统计信息
xcpc show contest spring-2026
```

## PDF 出版与奇偶留白页

XCPC Workstation 支持三种出版形态，桌面端（工作室顶部导出按钮）、CLI（`xcpc build`）与 MCP 产出完全一致的 PDF（默认写入 `build/` 并按标题自动命名，自动注入 Title/Author 等 PDF 元数据）：

1. **整本试题册**：包含封面与试题列表页（多于 1 题时默认开启）、每题独立起页与统一页眉页脚。
2. **A4 题解册（`--editorial`）**：沿用题面册的纸张、版心与字体体系，首页为扉页与带跳转链接的目录，每题独立起页并自动生成 PDF 书签，展示元信息与单色高亮代码块；未撰写 `editorial.md` 的题目自动跳过并输出警告。
3. **单题 PDF / 临时合集**：`xcpc build problem <id>` 导出无封面无题号前缀的单题 PDF；`xcpc build problems <id1> <id2> ...` 可临时将多道题合并为一本册子。

### 双面打印奇偶留白页（`blank-pages`）

开启 `blank-pages = true` 后：

- 每道题强制从**奇数页（右页）**开始；
- 若上一题在奇数页结束，引擎会自动在偶数页插入空白页，并在版心正中排印「此页故意留白」（英文 `This page is intentionally left blank`）；
- 最后一题结束后同样补齐为偶数页，方便直接双面装订发卷。

## 定稿快照（冻结）与只读保护

赛前完成全部审校后，应将比赛**定稿（冻结）**以锁定历史状态：

```bash
xcpc contest freeze spring-2026
```

冻结流程在 Core 层按以下步骤原子执行：

1. **完整性校验**：完整编译试题册与题解册并执行 `xcpc-lint`；如果存在 `error` 级错误（warning 不拦截），拒绝冻结。如果题目带有题目包，还会校验最近一次严格验收报告的内容哈希是否有效（不暗中编译跑数）。
2. **生成只读快照**：在 `contests/<id>/snapshot/` 中固化以下内容并写入 `frozen = true`：
   - 每道题合并覆写后的源文件副本（`problem.toml`、`statement*.md`、`editorial.md`、`samples/`、`assets/`，不含题目包源码与测试数据）
   - 当前生效的 `template.typ`、`templates/preamble.typ`（若存在）及解析后的模板参数
   - 当时导出的试题册 PDF 与题解册 PDF（无题解时不生成）
3. **只读保护**：冻结后，Core 层对整场比赛（含 `snapshot/`）强制只读，任何修改比赛元数据、题目列表或覆写的请求均被拦截；后续渲染与预览始终直接读取 `snapshot/`，不再受题库后续修改或工作区模板变动影响。

!!! tip "将 `snapshot/` 纳入 Git"
    `contests/<id>/snapshot/` 是正式赛事归档物，应随 `contest.toml` 一并提交到 Git 仓库，确保多年后仍可无损查看或重印完全一致的试题册。

## 解冻确认

如果定稿后确需勘误，必须由人工在桌面端或 CLI 执行**解冻**（**MCP 协议出于安全边界不开放解冻工具**）：

```bash
xcpc contest unfreeze spring-2026
```

执行解冻时，Core 会先逐文件比对 `snapshot/` 与当前题库之间的字节差异，明确列出哪些题目的题面、样例已变更或已被删除，随后要求输入比赛 ID 进行二次确认（脚本自动化可显式加 `--yes` 跳过交互输入）。确认后删除 `snapshot/` 目录，比赛恢复活引用与可编辑状态。

## 比赛归档（Archive）

应用内不提供「永久删除比赛」按钮，而是通过**归档**（在 `contest.toml` 写入 `archived = true`）将已结束的比赛收纳到「已归档」分组中：

```bash
# 归档比赛（收纳并强制只读）
xcpc contest archive spring-2026

# 取消归档（恢复先前的编辑/冻结状态）
xcpc contest unarchive spring-2026
```

- `archived` 与 `frozen` 完全正交：未定稿或已定稿的比赛均可归档。
- 归档后 Core 层会拦截一切写操作（返回 `contest_archived`），仅允许通过「取消归档」恢复。
- 如果一道题仍被**未冻结的已归档比赛**引用，删除该题时仍会被安全拦截，防止破坏历史组卷记录。
