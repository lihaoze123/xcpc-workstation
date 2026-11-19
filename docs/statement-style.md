# 题面样式与出版

## 1. 默认主题 `olymp-classic`

### 1.1 基准

`olymp-classic` 的**唯一基准**是 [`lihaoze123/polygon-chinese-template`](https://github.com/lihaoze123/polygon-chinese-template)，固定在 commit `10f99594ff6a92e73c96f1f4ae235717049c8446`。这个仓库是 `GassaFM/olymp.sty` 加上中文适配。

- **基准 PDF**：用该 commit 的 `olymp.sty` 和 `ctexart` 模板，通过 `tectonic` 编译生成（`scripts/build-olymp-ref.sh`），和对应的 Typst 源文件一起存放在 `tests/fixtures/olymp-ref/<用例>/`。生成时 `ctexart` 使用 `fontset=founder`，并把 fontconfig 限定为锁定的字体，确保和引擎读取同一套字体文件。
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
- 每道题结束后另起一页（`\clearpage`）。可选的 `intentionallyblankpages` 会在奇偶页需要时插入「此页故意留白」。

### 1.3 题头与限制表

- 题名**左对齐**（`\noindent`），字号 `\LARGE`，粗体无衬线：`Problem A. Title`。中文模式下关键词仍然是 `Problem`。
- 题头后留 `3mm`（`\afterproblemhead`），接着是两列的限制表 `tabular{l@{\extracolsep{1cm}}l}`，同样左对齐，表后留 `2mm`（`\afterconstraints`）。
- 中文模式下的字段名（不带冒号）依次为：`输入文件` / `输出文件`（文件名用等宽字体）、`时间限制`、`内存限制`。还可以加 `题目作者`、`题目开发者`、`题目来源`。
- `show-metadata = false` 时整张限制表都不显示。参考模板的 Polygon 模板（`statements.ftl`）默认就是隐藏的。
- 正文字号 `\normalsize`，首行缩进 `2em`。

### 1.4 章节标题

- 字号 `\Large`，粗体无衬线，前面留 `\smallbreak\smallskip`，后面留 `\smallskip`，并且禁止在标题后分页（Typst 中对应 `sticky: true`）。
- 中文关键词：`输入格式`、`输出格式`、`交互`、`样例`、`样例解释`、`提示`、`评分`、`约束`、`子任务 N`、`背景知识`、`题目要求`、`图表说明`。
- 中文模式把 `\le` / `\ge` 替换为 `\leqslant` / `\geqslant`（`⩽` / `⩾`）。

### 1.5 样例表

- 两列的 `tabular{|l|l|}`，每列宽 `0.43\linewidth`，使用等宽字体，保留所有空格和换行。
- 表头单元格居中，内容是 `\bf\texttt{\InputFileName}` / `\OutputFileName`。中文模式下显示为 `标准输入` / `标准输出`；中文没有等宽粗体，实际呈现为普通字重。
- 同一道题的多组样例放在**同一张表**里，每组占一行，行与行之间用 `\hline` 分隔，单元格底部留 `\medskip`。
- 可选变体有 `examplewide`（输入和输出上下排列）和 `examplethree`（多一列样例说明）。
- **分页**：LaTeX 的 `tabular` 不能跨页。剩余空间放不下时，基准会把「样例」标题连同整张表一起移到下一页，原页留白（见用例 `contest-zh` 的 C 题）。我们的行为相同：表格不拆分，标题通过 `sticky` 跟着表走。
- **与基准不同的地方（尚未实现）**：一整页都放不下的超长样例在基准里会溢出页面；我们计划改为跨页，并用 `table.header(repeat: true)` 重复表头。

### 1.6 封面与试题列表页

整本题册（不少于 2 道题）默认带封面，可以关闭：

- 用 `\maketitle`：标题前留 `4cm`，标题是黑体的「`{赛名}` 换行 `试题册`」；作者行用楷体，默认「出题人」；下面是日期。封面页不显示页眉和页脚。
- 页面底部浮动着「试题列表」：标题用黑体，表格字号 `\small`，格式是 `tabular{|p{1cm}<{\centering}|p{6cm}<{\centering}|}`。表格**只有题号和题名两列**，只有上下两条横线，行与行之间没有分隔线。
- 题表下方用 `\footnotesize` 写两行：`本试题册共 N 题，M 页。` 和 `如果您的试题册缺少页面，请立即通知志愿者。`

### 1.7 字体

| 用途 | 字体 | 来源 |
| :--- | :--- | :--- |
| 西文正文 / 标题 / 等宽 | Latin Modern Roman / Sans / Mono（与 XeLaTeX 默认一致） | nixpkgs `lmodern` |
| 数学 | Latin Modern Math（基准实际用的是 Computer Modern Type1：CMR/CMMI/CMSY/MSAM，字形相近，但盒子尺寸和间距略有不同） | nixpkgs `lmodern` |
| 中文正文（宋体） | 方正书宋 `FZShuSong-Z01` | `foundertype-ctex-fonts` |
| 中文标题（黑体，含 `\heiti`） | 方正黑体 `FZHei-B01` | 同上 |
| 中文作者行（楷体，`\kaishu`） | 方正楷体 `FZKai-Z03` | 同上 |
| 仿宋（`\fangsong`） | 方正仿宋 `FZFangSong-Z02` | 同上 |

- 方正字体包来自 `github:brsvh/chinese-fonts-overlay`，在 `flake.nix` 中锁定版本。开发环境通过 `XCPC_FONT_DIRS` 提供字体目录，引擎**优先**从这些目录加载字体，保证在任何机器上排版结果一致。
- 运行时如果找不到上述字体，回退到系统字体（`fontdb`）中的思源宋体 / 思源黑体，并在诊断信息中给出警告。

## 2. 主题定制

**当前只实现 `olymp-classic` 一套主题。** 比赛的 `contest.toml` 里只开放参考模板本身就有的开关：

| 键 | 含义 |
| :--- | :--- |
| `language` | `chinese` / `english`，决定题面关键词 |
| `show-metadata` | 是否显示限制表（可以按题覆盖） |
| `cover` | 是否生成封面和试题列表 |
| `blank-pages` | 是否为奇偶页插入「此页故意留白」 |
| `name` / `location` / `date` / `author` | 页眉和封面上显示的信息 |

更多预设、可视化的 Token 检查器、Typst 插槽覆写等定制体系，**等核心出版功能验证之后再考虑**。

## 3. 题解册

题解只输出 **A4 文字版**，沿用 `olymp-classic` 的版式，题头关键词用 `Problem Tutorial` 或 `题目解答`。每题一节，内容来自 `editorial.md`。**不做幻灯片。**

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
