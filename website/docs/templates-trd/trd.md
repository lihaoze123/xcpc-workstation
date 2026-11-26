---
title: 团队参考手册（TRD）
---

# 团队参考手册（TRD）

团队参考手册（TRD）存放在工作区的 `trds/<id>/` 目录下，采用**活引用**机制组织 `snippets/` 中的算法模板与纯公式卡片：日常维护模板时手册内容自动同步更新，赛前印刷前再通过预检与定稿冻结锁定确定版本。

## `trd.toml` 章节编排

每份手册由 `trds/<id>/trd.toml` 定义元信息、模板选项与章节列表：

```toml
name = "2026 ICPC 区域赛模板册"
author = "Team Alpha"
date = "2026 年 11 月"
template = "trd-compact"      # 可选：trd-compact（默认）、trd-book 或自定义 templates/<name>.typ
frozen = false
archived = false

[template-options.trd-compact]
columns = "3"                 # 栏数（trd-compact 支持 "2"/"3"/"4"；trd-book 支持 "1"/"2"）
font-size = "7.5pt"           # 正文字号
hash-interval = 10            # 累计校验哈希间隔（行数）；0 表示仅在末尾印总哈希
strip-comments = false        # 印刷时是否剥离 C/C++ 代码注释

[[chapter]]
title = "数学与多项式"
items = ["modint", "ntt", "number-theory-facts"]

[[chapter]]
title = "数据结构"
items = ["dsu"]
```

也可直接通过 `xcpc trd compose` 在命令行创建或更新手册编排：

```bash
# 按章节编排（--chapter 可重复传入，格式为 "章节标题:slug1,slug2"）
xcpc trd compose icpc-regional \
  --name "2026 ICPC 区域赛模板册" \
  --author "Team Alpha" \
  --date "2026 年 11 月" \
  --template trd-compact \
  --chapter "数学与多项式:modint,ntt,number-theory-facts" \
  --chapter "数据结构:dsu"

# 查看工作区内所有手册或某份手册的详细章节与预检状态
xcpc trd list
xcpc trd show icpc-regional
```

## 内置排版模板

XCPC Workstation 内置两套面向不同使用场景的灰度印刷级 Typst 模板，均支持通过 `[template-options.<template>]` 声明式调整参数，或通过 `xcpc template fork` 派生为工作区自定义模板：

| 模板 ID | 纸张与版式 | 默认栏数 / 字号 | 适用场景 |
| :--- | :--- | :--- | :--- |
| `trd-compact`（默认） | 横版 A4，紧凑多栏，灰度印刷 | 3 栏 / `7.5pt`（可选 2/3/4 栏） | 赛场携带的 25 页上限纸质参考手册 |
| `trd-book` | 竖版 A4，衬线正文，灰度印刷 | 1 栏 / `10pt`（可选 1/2 栏） | 赛前复习讲义、数学推导与公式代码混排读本 |

两套模板均原生支持：

- **自动目录与页眉页脚**：自动生成章节目录与页码，方便赛场快速定位。
- **KACTL++ 累计哈希旁注**：按 `hash-interval` 在代码边栏印出行号与 6 位累计 MD5 哈希，并在手册开头目录后附带赛场终端核对命令（`hash-interval > 0` 时）；开启 `strip-comments = true` 时，系统会在计算哈希**之前**剥离注释并折叠空行，确保纸面代码与校验哈希完全一致。
- **依赖索引标注**：自动剥离代码体内的 `#include "<slug>/code.hpp"`，并在条目头部以清晰的依赖标签标明所需前置模板。

## 印刷前预检（`xcpc trd check`）

在编译输出或送印前，运行预检命令同时执行**结构/验证预检**与 **Typst 排版编译检查**（不写入任何文件）：

```bash
xcpc trd check icpc-regional
```

预检覆盖五类结构与质量问题：

| 严重级别 | 检查项 | 说明 |
| :--- | :--- | :--- |
| Error | `missing_snippet` | 章节中引用了工作区不存在的模板 slug |
| Error | `missing_dependency` | 某模板依赖了上游模板（如 `ntt` 依赖 `modint`），但该上游模板未被编入当前手册 |
| Error | `dependency_cycle` | 引用的模板存在循环 `#include` 依赖 |
| Warning | `stale_snippet` | 编入的代码模板或其上游依赖已修改，验证缓存已过期（纯公式卡片不触发） |
| Warning | `failed_snippet` | 编入的代码模板最近一次验证失败（纯公式卡片不触发） |

!!! note "关于未验证模板"
    尚未执行验证（`unverified`）的代码模板默认不会在 `xcpc trd check` 中产生预检警告；若需强制所有代码模板均已通过验证后才能定稿，请使用 `xcpc trd freeze <id> --strict`。

## 编译导出与页面渲染

```bash
# 编译并导出 PDF（默认输出至工作区 build/<name>.pdf）
xcpc trd build icpc-regional
xcpc trd build icpc-regional -o ./dist/icpc-2026-trd.pdf

# 同时导出生成的 Typst 源码以便排查排版细节
xcpc trd build icpc-regional --emit-typst /tmp/trd.typ

# 将页面渲染为 PNG 预览图（默认输出至 build/<id>/page-<n>.png，scale 2.0 ≈ 144 dpi）
xcpc trd render icpc-regional
xcpc trd render icpc-regional --page 1 --scale 2.5 -o ./preview/
```

## 可逆定稿与只读快照（`freeze` / `unfreeze`）

当某场比赛的团队参考手册排版确认、准备打印时，可以执行**定稿冻结**，将其所引用的全部模板文件、排版配置与成品 PDF 复制归档到 `trds/<id>/snapshot/`：

```bash
# 标准定稿：允许带警告冻结（错误项如缺失模板、缺少前置依赖、依赖环仍会阻断）
xcpc trd freeze icpc-regional

# 严格定稿：若手册内存在任何未验证、过期或验证失败的代码模板，拒绝冻结
xcpc trd freeze icpc-regional --strict
```

定稿后的保护机制与比赛定稿保持一致（参见 [组卷与定稿归档](../publishing/contests.md)）：

- **彻底只读**：`trd.toml` 中 `frozen = true` 的手册在 Core 层拒绝一切修改操作；后续再次预览、检查或导出时，直接从 `trds/<id>/snapshot/` 读取快照，**完全不受**工作区 `snippets/` 后续迭代修改的影响。
- **可逆解冻与差异对比**：如需基于最新模板库重新修订手册，可在桌面端或 CLI 执行解冻。解冻前系统会自动比对 `snapshot/` 与当前 `snippets/` 之间的差异（代码变更、说明变更、元数据变更或模板已删除），并要求二次确认（CLI 可用 `--yes` 跳过交互确认）：

```bash
xcpc trd unfreeze icpc-regional
```

!!! warning "安全边界"
    解冻操作（`unfreeze`）仅开放给桌面端与本地 CLI，**不开放给 MCP 工具**，避免外部 AI Agent 意外解除已定稿手册的只读保护。
