# 题面样式与出版

## 1. 默认主题 `olymp-classic`

### 1.1 基准

`olymp-classic` 的**唯一基准**是 [`lihaoze123/polygon-chinese-template`](https://github.com/lihaoze123/polygon-chinese-template)，固定在 commit `10f99594ff6a92e73c96f1f4ae235717049c8446`。这个仓库是 `GassaFM/olymp.sty` 加上中文适配。

- **基准 PDF**：用该 commit 的 `olymp.sty` 和 `ctexart` 模板，通过 `tectonic` 编译生成（`scripts/build-olymp-ref.sh`），存放在 `tests/fixtures/olymp-ref/<用例>/`。同一目录下有两份内容相同的输入：参考模板的 Markdown（`contest.md`、`problems/`）和我们的工作区（`workspace/`，由 `scripts/fixture-to-workspace.py` 转换得到）。回归比较的是工作区经过完整链路（Markdown 转换 → `olymp-classic`）出的 PDF。生成时 `ctexart` 使用 `fontset=founder`，并把 fontconfig 限定为锁定的字体，确保和引擎读取同一套字体文件。
- **验收标准：看起来一样。** 把同一份内容的基准 PDF 和我们的 PDF 逐页并排对照，版式、字体、字号、层级、间距的比例都一致，正常阅读时看不出差别。**不要求**逐点重合：TeX 和 Typst 的断行、行距计算、数学排版本来就不同，这些差异予以尊重，不去模拟。但也不能差得太远：页眉、页脚、题头、表格这些固定元素的位置，应当和基准基本重合。
- **冲突时的处理**：本文的描述与基准 PDF 不一致时，以基准 PDF 为准，并修正本文。

### 1.1.1 实现原则：设计参数，不用魔数

- 模板里的数值只能是**设计参数**：直接取自 `olymp.sty`、`ctexart` 和参考模板源码中的字号、行距、`\smallskip`、`\afterproblemhead`（3mm）、`\tabcolsep`、线宽、列宽、版心尺寸等，并在注释里写明出处。
- **禁止**为了和某个基准对齐而反推偏移量，比如「下移 2.16bp」「右移 3.9bp」，也不要在 Typst 里模拟 TeX 的 `\lineskip`、`\topskip` 之类的行距细节。看起来不一样时，应该找是哪条**设计规则**写错了（字体、字号、缩进、哪段间距），而不是补一个数。例如页眉的规则是「文字基线离正文顶端 `\headsep`，横线在墨迹下方 2pt」，页脚是「横线下空 6pt 接文字」。
- 优先用 Typst 的原生写法：`page` 的 `header`/`footer`、`block` 的 `above`/`below`、`sticky`、weak 间距、`set`/`show` 规则。模板保持短小、易读。
- 每次修改模板后运行 `scripts/olymp-regress.sh`：检查所有用例的页数是否一致，并为每一页生成「基准 | 我们 | 叠加」的并排图（输出到 `target/olymp-regress/`），**逐页看图**确认。`scripts/compare-pdf.py` 能给出逐词坐标和像素差异，只作诊断参考，不作为验收指标。
- 用例要覆盖不同场景：中文/英文、显示/隐藏限制表、单组/多组样例、长样例、列表、行间公式、跨页长段落、样例后接正文等。新增版式规则时，同时补上对应的用例。

### 1.2 版面

- 文档类：`ctexart`，`12pt,a4paper,oneside`。
- 版心（`olymp.sty`，纵向 12pt）：`\textwidth=175mm`、`\textheight=245mm`、`\hoffset=-20mm`、`\voffset=-35mm`、`\headheight=2cm`、`\headsep=6mm`。
- **页眉**：左侧赛名，右侧日期；下方间隔 `2pt` 后是一条 `\hrule`（0.4pt）。封面页不显示页眉（`\thispagestyle{empty}`）。
- **页脚**：一条 `\hrule`，间隔 `6pt` 后用无衬线字体居中显示 `Page X of Y`。**中文模式下也是 `Page X of Y`**，因为 `\kw@Page` 和 `\kw@of` 在中文模式下没有翻译。封面的页码计为 0，所以第一道题从第 1 页开始。
- 每道题结束后另起一页（`\clearpage`）。可选的 `intentionallyblankpages`（`contest.toml` 的 `blank-pages`）让每道题从奇数页开始，空出的偶数页在版心正中用 `\LARGE` 写「此页故意留白」；最后一题之后同样补齐，所以整本是偶数页。
- **单题**：与参考模板的独立题面（`problem.template.tex`）一致，页眉左侧是「题面预览」（英文题面留空），右侧没有日期，没有封面，题号是 A。

### 1.3 题头与限制表

- 题名**左对齐**（`\noindent`），字号 `\LARGE`，粗体无衬线：`Problem A. Title`。中文模式下关键词仍然是 `Problem`。
- 题头后留 `3mm`（`\afterproblemhead`），接着是两列的限制表 `tabular{l@{\extracolsep{1cm}}l}`，同样左对齐，表后留 `2mm`（`\afterconstraints`）。
- 中文模式下的字段名（不带冒号）依次为：`输入文件` / `输出文件`（文件名用等宽字体）、`时间限制`、`内存限制`。还可以加 `题目作者`、`题目开发者`、`题目来源`。
- `show-metadata = false` 时整张限制表都不显示。参考模板的 Polygon 模板（`statements.ftl`）默认就是隐藏的。
- 正文字号 `\normalsize`，首行缩进 `2em`。
- **限制的写法**：`problem.toml` 里写 `time = "1s"`、`memory = "256MB"`，出版时按语言写成 `1 秒` / `256 MB`（中文）或 `1 second` / `256 megabytes`（英文），与参考模板的用例一致；不认识的写法原样输出。没有写输入输出文件名时显示 `标准输入` / `标准输出`（`standard input` / `standard output`）。

### 1.4 章节标题

- 字号 `\Large`，粗体无衬线，前面留 `\smallbreak\smallskip`，后面留 `\smallskip`，并且禁止在标题后分页（Typst 中对应 `sticky: true`）。
- 中文关键词：`输入格式`、`输出格式`、`交互`、`样例`、`样例解释`、`提示`、`评分`、`约束`、`子任务 N`、`背景知识`、`题目要求`、`图表说明`。
- 中文模式把 `\le` / `\ge` 替换为 `\leqslant` / `\geqslant`（`⩽` / `⩾`）。替换后的符号仍是关系符（`math.class("relation", …)`），否则紧跟在 `)` 或上标后面时会丢掉两侧的间距。
- **数学公式**：Markdown 里的 TeX 公式由 `mitex` 转成 Typst 数学代码，在 MiTeX 的作用域里求值。这个作用域（MiTeX 0.2.4 的 `specs/`，Apache-2.0）随引擎内嵌在 `/xcpc/mitex/`，只改了两处：不依赖 `@preview/xarrow` 包，以及去掉 Typst 0.15 已删除的 `kai` 符号。

### 1.5 样例表

- 两列的 `tabular{|l|l|}`，每列宽 `0.43\linewidth`，使用等宽字体，保留所有空格和换行。
- 表头单元格居中，内容是 `\bf\texttt{\InputFileName}` / `\OutputFileName`。中文模式下显示为 `标准输入` / `标准输出`；中文没有等宽粗体，实际呈现为普通字重。
- 同一道题的多组样例放在**同一张表**里，每组占一行，行与行之间用 `\hline` 分隔，单元格底部留 `\medskip`。
- 可选变体有 `examplewide`（输入和输出上下排列）和 `examplethree`（多一列样例说明）。
- **分页**：LaTeX 的 `tabular` 不能跨页。剩余空间放不下时，基准会把「样例」标题连同整张表一起移到下一页，原页留白（见用例 `contest-zh` 的 C 题）。我们的行为相同：表格不拆分，标题通过 `sticky` 跟着表走。
- **与基准不同的地方**：一整页都放不下的超长样例在基准里会溢出页面；我们改为跨页，每页重复表头（`table.header(repeat: true)`），一组样例的单元格也可以在中间断开，不加「（续）」之类的字。是否跨页按表格高度与版心高度比较决定，所以放得下的表格行为与基准完全一致。

### 1.6 封面与试题列表页

整本题册（不少于 2 道题）默认带封面，可以关闭：

- 用 `\maketitle`：标题前留 `4cm`，标题是黑体的「`{赛名}` 换行 `试题册`」；作者行用楷体，默认「出题人」；下面是日期。封面页不显示页眉和页脚。
- 页面底部浮动着「试题列表」：标题用黑体，表格字号 `\small`，格式是 `tabular{|p{1cm}<{\centering}|p{6cm}<{\centering}|}`。表格**只有题号和题名两列**，只有上下两条横线，行与行之间没有分隔线。
- 题表下方用 `\footnotesize` 写两行：`本试题册共 N 题，M 页。` 和 `如果您的试题册缺少页面，请立即通知志愿者。`

### 1.7 字体

| 用途 | 字体 | 来源 |
| :--- | :--- | :--- |
| 西文正文 / 标题 | Latin Modern Roman / Sans（与 XeLaTeX 默认一致） | nixpkgs `lmodern` |
| 等宽（样例、文件名与行内代码） | Latin Modern Mono 12（与基准一致） | nixpkgs `lmodern` |
| 代码块（`raw.where(block: true)`） | JetBrains Mono（带粗体与斜体字形） | nixpkgs `jetbrains-mono` |
| 数学 | Latin Modern Math（基准实际用的是 Computer Modern Type1：CMR/CMMI/CMSY/MSAM，字形相近，但盒子尺寸和间距略有不同） | nixpkgs `lmodern` |
| 中文正文（宋体） | 方正书宋 `FZShuSong-Z01` | `foundertype-ctex-fonts` |
| 中文标题（黑体，含 `\heiti`） | 方正黑体 `FZHei-B01` | 同上 |
| 中文作者行（楷体，`\kaishu`） | 方正楷体 `FZKai-Z03` | 同上 |
| 仿宋（`\fangsong`） | 方正仿宋 `FZFangSong-Z02` | 同上 |

- 方正字体包来自 `github:brsvh/chinese-fonts-overlay`，在 `flake.nix` 中锁定版本。开发环境通过 `XCPC_FONT_DIRS` 提供字体目录，引擎**优先**从这些目录加载字体，保证在任何机器上排版结果一致；release 构建同时内嵌 Latin Modern 与 JetBrains Mono 字体作为兜底。
- 设置了 `XCPC_FONT_DIRS` 时，引擎默认只加载该目录集合（以及 release 构建内嵌的 Latin Modern / JetBrains Mono），不扫描系统字体，保证跨机器排版与视觉回归确定性；CLI 可额外传入 `--system-fonts`。当 `XCPC_FONT_DIRS` 未设置或为空时（独立打包或 Windows 分发场景），引擎自动扫描可执行文件同级的 `fonts/` 目录并开启系统字体回退，按角色依次查找方正字体 → 思源 / Noto CJK → Windows 常见中文字体（`SimSun` / `SimHei` / `KaiTi` / `FangSong` 等），若发生替换则在编译诊断中给出非致命警告。

## 2. 模板与声明式定制

**所有生效模板都是工作区或比赛目录中平等的、可直接编辑的 `.typ` 文件。** 引擎不把任何内置模板当作不可修改的特权一等公民；内置的 `olymp-classic`、`cnoi-classic` 与 `minimal` 仅作为**预设种子**，在新建工作区或「从预设添加模板」时落盘到 `templates/<name>.typ`。

### 2.1 模板解析优先级

1. **比赛专属模板**：`contests/<id>/template.typ`（若存在，直接优先于 `contest.toml` 中的 `template` 字段；用于给单场比赛加校徽、改封面或深度定制，不污染工作区模板目录）；
2. **工作区模板**：`templates/<name>.typ`（由 `contest.toml` 的 `template` 或 `workspace.toml` 的 `template` 指定）；
3. **无内置回退**：若引用的模板文件不存在，编译直接报 `missing_template` 错误，并在界面提供「从预设安装」一键入口。

### 2.2 单入口数据驱动契约（`#let render(doc)`）

所有模板共享极简的数据驱动单入口 **`#let render(doc)`**（以及可选的 `#let schema = (...)`）：

```typst
#let render(doc) = {
  // doc 携带完整比赛/单题数据与合并后的 options：
  // - doc.name / doc.location / doc.date / doc.author / doc.language
  // - doc.book ("statements" | "editorial") / doc.single (bool)
  // - doc.pdf-title / doc.pdf-author
  // - doc.options (工作区 + 比赛合并后的 book 级模板参数字典)
  // - doc.problems: array of (
  //     id, label, title, author, rating, tags, sources,
  //     input, output, time-limit, memory-limit,
  //     samples: ((in, out), ...),
  //     options: (单题级合并后的模板参数字典),
  //     meta: (题解副标题元信息数组),
  //     body: content,  // 已转换的 Markdown 正文，完整保留 SourceMap 双向定位
  //   )
  ...
}
```

- 模板完全掌控封面、题表、是否每题换页、多栏布局等任意排版，不再受固定函数签名束缚。
- 正文转换中用到的 `section(key, title: none)`、`samples(..pairs)`、`subsection(title)`、`runin(title)` 为**可选覆写钩子**：模板导出它们即可定制章节标题与样例表；未导出时自动回退到 `/xcpc/defaults.typ` 中的默认实现。
- Core 仅保留纯业务元数据（赛名、日期、地点、署名、语言、题目与限值）；**`cover`、`blank-pages`、`show-metadata` 等版式开关不再由 Core 或 UI 硬编码**，全部改由各模板在自己的 `#let schema` 中自行声明，预设种子与用户模板完全平权。

### 2.3 模板声明式 Schema（`#let schema`）

每个模板可在自身的 `.typ` 文件内原生导出 `#let schema = (...)` 字典，单文件自包含、默认值直接复用模板常量不漂移。Core 通过内置 Typst 引擎轻量求值读取该字典，驱动 GUI 属性面板动态渲染控件，并同步开放给 CLI / MCP 校验与查询；未导出 `schema` 的自定义模板优雅降级为空扩展参数。

```typst
#let schema = (
  name: "OI 风格",                     // 显示名称（缺省用文件名）
  description: "CCF CSP-J/S 与 NOI 官方风格题面与题解",
  groups: (
    (id: "cover", label: "封面与考试说明"),
    (id: "samples", label: "样例与正文样式"),
    (id: "problem", label: "单题设置"),
  ),
  fields: (
    (
      key: "cover",
      label: "封面",
      type: "enum",                    // bool | enum | string | lines | number | length | color | font | image
      scope: "book",                   // book（整册）| problem（单题）
      group: "cover",                  // 可选，对应 groups[].id
      default: "auto",
      choices: ((value: "auto", label: "自动"), (value: "on", label: "开"), (value: "off", label: "关")),
    ),
    (
      key: "logo",
      label: "封面徽标",
      type: "image",                   // 工作区相对路径（如 "contests/<id>/assets/logo.png"），GUI 支持直接选图导入
      scope: "book",
      group: "cover",
      default: "",
      when: (cover: "on"),             // 可选，简单等值条件显隐
    ),
  ),
)
```

支持的实用排版参数类型共 9 种：`bool`、`enum`、`string`、`lines`（多行/列表）、`number`、`length`（带单位）、`color`、`font`、`image`（图片选择并自动导入至 `assets/`），配合 `group` 分组与简单等值 `when` 条件显隐。

### 2.4 三级作用域与命名空间隔离

模板参数按**模板名分命名空间**持久化（比赛专属 `contests/<id>/template.typ` 使用独立命名空间 `local`），切换模板时不丢失原模板配置、不同模板同名键互不冲突，切回即恢复：

1. **工作区默认**（`workspace.toml` 的 `[template-options.<template>]`）：作为整套工作区的默认值，单题预览与未覆写的比赛均继承于此；
2. **比赛级覆写**（`contest.toml` 的 `[template-options.<template>]`）：覆写该场比赛的 `scope: "book"` 参数；
3. **单题级参数**（`problem.toml` 的 `[template-options.<template>]` 与 `contest.toml` 中 `[[problem]]` 的 `[problem.options.<template>]`）：用于 `scope: "problem"` 参数；比赛中先取 `[[problem]]` 覆写，缺省回退到 `problem.toml`，再回退到模板默认值。

未显式设置的字段不写入 TOML；将覆写改回上级值或点击还原时自动删除对应键。所有写回统一走 `toml_edit` 保留注释与未知字段。

### 2.5 预设种子说明

- **`olymp-classic`**：默认输出严格对齐 `lihaoze123/polygon-chinese-template@10f9959`（视觉回归逐页一致），并在自身 `schema` 中声明 `cover`、`show-metadata`、`blank-pages`、封面徽标、字号/页边距、字体与单题 `show-metadata` / `sample-layout`（双栏 / 宽版）。
- **`cnoi-classic`**：CCF CSP-J/S 与 NOI 风格，在自身 `schema` 中声明 `cover`、`show-metadata`、`blank-pages`、封面徽标、编译选项、提交/编译表开关、注意事项、样例框描边色与行号、中文着重号、字号/页边距、字体以及单题 `prob-type` / `source-file`。
- **`minimal`**：约 40 行的极简空白起点模板，方便用户从零快速编写自定义比赛模板。
- **工作区级样式注入（`.xcpc/preamble.typ`）**：仍在模板渲染前注入作用域，用于跨模板共享自定义数学宏或 Typst 包导入。

## 3. 题解册

题解只输出 **A4 文字版**，**不做幻灯片**。参考模板只有 Beamer 版题解，没有 A4 题解的基准，所以题解册是我们自己的设计（2026-11-20 与用户确认）：

- **同一套书**：纸张、版心、字体、字号体系，以及页眉页脚的外框（横线和 `Page X of Y`）都与题面册相同，放在一起像同一套出版物。不再照搬 olymp.sty 的 `tutorial` 环境。
- **不加装饰**：正文和元素里不用分隔符（`·`、`|`）、分隔线、底色框和引导点，层级只靠字号、字重、缩进和留白表达。页眉页脚的横线属于和题面册共享的外框，不受这条约束。
- **扉页和目录**：比赛的题解册（题面册有封面时）第一页是扉页，与封面同构：赛名下一行写「题解」（英文 `Editorial`），然后是作者和日期。下半页是目录，题号（粗体无衬线）、题名、页码三列，沿用试题列表的 1cm 和 6cm 列宽，不加线；每行都是指向该题的链接，PDF 书签按题生成。扉页不显示页眉页脚，页码计为 0。单题导出没有扉页。
- **每题另起一页**，不插入留白页。没有 `editorial.md` 的题跳过，目录里也不列，只给出警告。
- **页眉**：左侧赛名（单题时为「题解」），右侧本页所属的题，如「B　灯笼街」。
- **题头**：题号与题名之间空 1em，`\LARGE` 粗体无衬线。下方空 `2\smallskip` 是一行 `\small` 浅灰无衬线的元信息：作者（「作者 X」/ `by X`）、标签、来源（如 `Codeforces 1999A`），各项之间只空 1.5em；没有元信息时不占行。不印 rating。题头之后空 `2\bigskip` 接正文。
- **正文**：字号、行距、首行缩进与题面相同。`##` 是 `\large` 粗体无衬线的小节，比题面册的小节低一级，前空 `\bigskip`、后空 `\smallskip`；`###` 及更深的标题像 `\paragraph` 一样与段落同行起头（粗体，后空 1em）。题解里不做章节识别。
- **代码块**：等宽（JetBrains Mono）、`\small`，整体缩进一个段首缩进（2em），不加底色、线和行号，可以跨页。单色高亮（`templates/mono.tmTheme`）：关键字粗体，注释斜体深灰，其余黑色，黑白打印同样清楚。
- **引用块**：按 LaTeX `quote` 环境，左右各缩进 `\leftmargini`（2.5em），首行不再缩进。
- **表格**：不加线，表头粗体。
- 中文粗体用方正黑体、斜体用方正楷体，与 ctex `fontset=founder` 一致（题面册同样如此）。
- 没有基准 PDF，题解册不做视觉回归；版式由单元测试和人工看图把关，示例见 `examples/workspace` 的 `spring-training`。

## 4. 题面体检与印刷前预检（`xcpc-lint`，Phase 2）

规则在 Rust 侧执行，既可以在出版工作室的状态栏实时提示（只在有问题时出现），也可以通过 MCP 工具提供给 Agent 自动修复：

- **排版与数学规范**：`<=` / `>=` / `!=` 没有写成 `\le` / `\ge` / `\ne`；中西文与公式之间缺空格；同一题面中变量大小写混用（如 `$N$` 和 `$n$`）；样例为空，或带有异常的尾随空白。
- **印刷前预检**：配图分辨率低于 300 DPI；文本或公式超出版心；奇偶页补白不正确；题表与实际题目数量、页码不一致。

## 5. 代码模板册（TRD，后续阶段）

把 `templates/` 下的代码排成 2 / 3 / 4 栏的参考手册：自动生成目录，带语法高亮，不设页数上限。每段代码印一个 KACTL 风格的 6 位哈希，计算时先去掉注释和空白。

## 6. 双语题面与语义级 Diff（后续阶段）

- `statement.zh.md` 和 `statement.en.md` 共用 `samples/`、限制和 `assets/`。出版工作室提供双语并排校对视图，两种语言可以分别导出。
- 以 Git 历史或冻结快照为基准，按章节展示语义级 Diff。章节的划分规则见 [product.md](./product.md#21-题面结构)。
- MCP 提供双语一致性校验：比对两种语言中的变量、数据范围和边界条件。
