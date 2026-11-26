# 模板与主题定制

XCPC Workstation 的所有出版模板都是工作区或比赛目录中可直接编辑的普通 `.typ` 文件。内置模板仅作为**预设种子**，在新建工作区或执行 `xcpc template add` 时释放到 `templates/<name>.typ`，运行期不存在不可修改的硬编码特权模板。

## 三套内置题面模板对比

| 模板 ID | 定位与风格 | 核心版式特征 | 典型模板参数（`#let schema`） |
| :--- | :--- | :--- | :--- |
| **`olymp-classic`** | XCPC / ICPC 经典风格（默认） | 严格对齐 `lihaoze123/polygon-chinese-template`（`olymp.sty` + `ctexart` 12pt）：A4 纵向、左对齐题头与两列限值表、横线页眉与 `Page X of Y` 页脚、双栏样例表（超长样例自动跨页重复表头）、带封面与试题列表页、A4 无装饰题解册。 | `cover`、`logo`、`show-metadata`、`blank-pages`、`body-size`、`margin-x`、`margin-y`、`cjk-body-font`、`cjk-heading-font`；单题 `show-metadata`、`sample-layout`（双栏/宽版）。 |
| **`cnoi-classic`** | CCF CSP-J/S 与 NOI 官方风格 | 2.4cm 页边距、带源程序文件名表/编译选项表/注意事项的官方封面、`【章节】` 标题、中文加粗着重号、带行号蓝色描边样例框、三线表数据范围、`第 N 页 共 M 页` 页脚。 | `cover`、`logo`、`compile-opts`、`show-submit-table`、`show-compile-table`、`notices`、`sample-border-color`、`sample-line-numbers`、`dotted-emphasis`、`blank-pages`；单题 `prob-type`、`source-file`。 |
| **`minimal`** | 极简空白起点模板 | 约 90 行精简源码，保留最小页面、字体与题目循环骨架，便于从零编写学校或组织专属版式。 | `body-size`、`margin`、`show-cover`。 |

!!! note "模板解析优先级"
    编译一场比赛时，引擎按以下顺序寻找生效模板：
    1. **比赛专属模板**：`contests/<id>/template.typ`（若存在，直接覆盖 `contest.toml` 中的 `template` 字段）。
    2. **工作区模板**：`templates/<name>.typ`（取自 `contest.toml` 的 `template`，缺省沿用 `workspace.toml` 的 `template`）。
    3. 如果目标文件不存在，编译直接报错提示缺失模板，不会静默回退到隐藏内置版本。

## 声明式 `#let schema` 与单入口契约

每个模板文件只需导出 `#let render(doc)` 函数即可完全接管整本 PDF 的封面、页眉页脚、多栏与分节排版；可选导出的 `#let schema` 字典则用于向桌面端属性面板、CLI 与 MCP 声明可配置参数：

```typst
#let schema = (
  name: (zh: "我的校赛模板", en: "My Contest Template"),
  description: "基于 olymp-classic 定制的校赛版式",
  groups: (
    (id: "cover", label: (zh: "封面设置", en: "Cover")),
  ),
  fields: (
    (
      key: "cover",
      label: (zh: "显示封面", en: "Show Cover"),
      type: "enum",
      scope: "book",           // book（整册）或 problem（单题）
      group: "cover",
      default: "auto",
      choices: (
        (value: "auto", label: "自动"),
        (value: "on", label: "开"),
        (value: "off", label: "关"),
      ),
    ),
    (
      key: "logo",
      label: "封面徽标",
      type: "image",
      scope: "book",
      group: "cover",
      default: "",
      when: (cover: "on"),     // 简单等值条件显隐
    ),
  ),
)
```

`schema` 支持 9 种参数类型：`bool`、`enum`、`string`、`lines`、`number`、`length`、`color`、`font`、`image`；面向用户展示的文本既可写纯字符串，也可写 `(zh: "...", en: "...")` 双语字典。

### 三级参数作用域与命名空间隔离

模板参数按**模板名分命名空间**存储（比赛专属 `contests/<id>/template.typ` 使用 `local` 命名空间），切换模板时原参数不会丢失：

1. **工作区默认**：`workspace.toml` 中的 `[template-options.<template>]`
2. **比赛级覆写**：`contest.toml` 中的 `[template-options.<template>]`（`scope: "book"`）
3. **单题级参数**：`problem.toml` 中的 `[template-options.<template>]`，以及 `contest.toml` 中 `[[problem]]` 下的 `[problem.options.<template>]`（`scope: "problem"`）

## `templates/preamble.typ` 与离线内置 Typst 包

### 自定义公共宏（`templates/preamble.typ`）

在工作区中创建 `templates/preamble.typ`（保留文件名，不会作为独立比赛模板列出），其中的定义会在所有模板与正文编译前自动注入作用域。适合存放全工作区共享的数学宏、配色常量或常用包导入：

```typst
// templates/preamble.typ
#import "@preview/cetz:0.5.3"
#let mod(m) = $space (op("mod") m)$
```

桌面端会监听 `templates/preamble.typ` 的变更，保存后即时热刷新预览；比赛定稿冻结时也会将其一并固化进 `snapshot/`。

### 离线内置包与 Typst Universe

引擎内置了以下 5 个高频 `@preview` 绘图与标注包的离线镜像，即使在完全断网的赛场或内网环境也能零延迟编译：

- `@preview/cetz:0.5.3`（矢量绘图）
- `@preview/oxifmt:1.0.0`（字符串格式化，`cetz` 依赖）
- `@preview/pinit:0.2.2`（相对定位标注）
- `@preview/mannot:0.4.0`（数学公式高亮与标注）
- `@preview/tiptoe:0.4.0`（箭头样式）

其他 `@preview` 包会按标准 Typst 路径（`TYPST_PACKAGE_PATH`、`TYPST_PACKAGE_CACHE_PATH`）查找并在联网时按需下载缓存；设置环境变量 `XCPC_OFFLINE=1` 可完全禁止网络下载。

## 使用 `xcpc template` 管理模板

```bash
# 列出工作区已有模板及可安装的预设种子
xcpc template list

# 查看指定模板解析后的 #let schema 字段与默认值
xcpc template schema olymp-classic

# 将内置预设种子释放到 templates/<name>.typ
xcpc template add cnoi-classic
xcpc template add minimal my-school

# 复制现有工作区模板作为自定义起点
xcpc template fork olymp-classic icpc-regional-2026
```

## 应用外观主题定制（全局 `theme.toml`）

出版模板控制的是 **PDF 版式**（随工作区进 Git），而桌面客户端本身的配色、圆角与编辑器字体属于**个人全局偏好**，存放在用户数据目录 `<dataDir>/theme.toml`（不写入工作区）。

在桌面端 **「设置 → 外观 → 打开 theme.toml」** 可自动生成并定位该文件，保存后无需重启即自动生效：

```toml
# <dataDir>/theme.toml（所有字段均可选）

[colors.light]
primary = "#0B6989"
background = "#FAFAFA"
card = "#FFFFFF"
sidebar = "#E4E4E7"

[colors.dark]
primary = "#0E7FA6"
background = "#09090B"
card = "#111113"
sidebar = "#141417"

[editor]
font_family = "JetBrains Mono"
font_size = 14
line_height = 22

[radius]
base = 8
```

- `[colors.light]` / `[colors.dark]` 支持覆写 22 个语义色彩 Token（十六进制颜色值），包括 `background`、`foreground`、`card`、`muted`、`border`、`primary`、`accent`、`secondary`、`sidebar`、`ring`、`destructive` 等。
- `[editor]` 支持自定义 `font_family`、`font_size`（11–22）与 `line_height`（16–36）。
- `[radius]` 的 `base` 控制全局基础圆角半径（0–20）。
