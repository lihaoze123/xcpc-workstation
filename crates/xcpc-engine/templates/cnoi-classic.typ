// CNOI / NOI official-style statement & editorial template for XCPC Workstation.
// Faithfully reproduces the CCF CSP-J/S & NOI contest PDF typography (cf. Wallbreaker5th's
// fuzzy-cnoi-statement, MIT-0):
// - A4 with 2.4cm margins, 12pt Songti body with 2em first-line indent
// - Official cover page with transposed problem summary table, source filename table,
//   compiler options table, and numbered contest notices
// - Running header with contest name (left) + current problem title (right) + 0.3pt rule
// - Centered footer "第 N 页　　共 M 页" with classic blue total-page link
// - Bracketed 【章节】 headings indented by 1.5em, dotted emphasis for Chinese bold text
// - Iconic blue-bordered (0.5pt #0000ff) sample boxes with outside gray line numbers
// - Three-line academic tables for data constraints / subtasks
//
// Exports the exact same contract as `/xcpc/olymp-classic.typ` (`contest`, `problem`,
// `section`, `samples`, `tutorial`, `subsection`, `runin`) so any workspace or contest
// can switch templates via `template = "cnoi-classic"` without changing Markdown files.

#import "/xcpc/fonts.typ": cjk-song, cjk-hei, cjk-kai, cjk-fangsong

#let default-notices = (
  "文件名（程序名和输入输出文件名）必须使用英文小写。",
  "main 函数的返回值类型必须是 int，程序正常结束时的返回值必须是 0。",
  "若无特殊说明，结果的比较方式为全文比较（过滤行末空格及文末回车）。",
  "程序可使用的栈空间内存限制与题目的内存限制一致。",
  "评测在当前最新公布的 NOI Linux 下进行，各语言的编译器版本以此为准。",
)

#let schema = (
  name: (zh: "OI 风格", en: "OI Classic"),
  description: (
    zh: "CCF CSP-J/S 与 NOI 官方风格题面与题解",
    en: "Official CCF CSP-J/S and NOI style statements and editorials",
  ),
  groups: (
    (id: "cover", label: (zh: "封面与考试说明", en: "Cover & Exam Notes")),
    (id: "samples", label: (zh: "样例与正文样式", en: "Samples & Body Style")),
    (id: "layout", label: (zh: "页面与字体", en: "Page & Fonts")),
    (id: "problem", label: (zh: "单题设置", en: "Problem Settings")),
  ),
  fields: (
    (
      key: "cover",
      label: (zh: "封面与考试说明", en: "Cover & Exam Notes"),
      type: "enum",
      scope: "book",
      group: "cover",
      default: "auto",
      choices: (
        (value: "auto", label: (zh: "自动（多题时显示）", en: "Auto (multi-problem)")),
        (value: "on", label: (zh: "总是显示", en: "Always")),
        (value: "off", label: (zh: "不显示", en: "Off")),
      ),
    ),
    (
      key: "logo",
      label: (zh: "封面徽标", en: "Cover Logo"),
      description: (
        zh: "显示在封面顶部，留空则不显示",
        en: "Displayed at the top of the cover; leave empty to hide",
      ),
      type: "image",
      scope: "book",
      group: "cover",
      default: "",
    ),
    (
      key: "compile-opts",
      label: (zh: "编译选项", en: "Compiler Options"),
      type: "string",
      scope: "book",
      group: "cover",
      default: "-O2 -std=c++14 -static",
    ),
    (
      key: "show-submit-table",
      label: (zh: "显示提交源程序文件名表", en: "Show Source Filename Table"),
      type: "bool",
      scope: "book",
      group: "cover",
      default: true,
    ),
    (
      key: "show-compile-table",
      label: (zh: "显示编译选项表", en: "Show Compiler Options Table"),
      type: "bool",
      scope: "book",
      group: "cover",
      default: true,
    ),
    (
      key: "notices",
      label: (zh: "注意事项", en: "Contest Notices"),
      description: (
        zh: "每行一条，显示在封面底部",
        en: "One item per line, shown at the bottom of the cover",
      ),
      type: "lines",
      scope: "book",
      group: "cover",
      default: default-notices,
    ),
    (
      key: "show-metadata",
      label: (zh: "无封面时显示题头限值行", en: "Show Limits Row Without Cover"),
      type: "bool",
      scope: "book",
      group: "samples",
      default: true,
    ),
    (
      key: "blank-pages",
      label: (zh: "奇数页起题（留白）", en: "Start Problems on Odd Pages"),
      type: "bool",
      scope: "book",
      group: "samples",
      default: false,
    ),
    (
      key: "sample-border-color",
      label: (zh: "样例框描边颜色", en: "Sample Box Border Color"),
      type: "color",
      scope: "book",
      group: "samples",
      default: "#0000ff",
    ),
    (
      key: "sample-line-numbers",
      label: (zh: "样例显示行号", en: "Show Sample Line Numbers"),
      type: "bool",
      scope: "book",
      group: "samples",
      default: true,
    ),
    (
      key: "dotted-emphasis",
      label: (zh: "中文加粗加着重号", en: "Dotted Emphasis for CJK Bold"),
      type: "bool",
      scope: "book",
      group: "samples",
      default: true,
    ),
    (
      key: "body-size",
      label: (zh: "正文字号", en: "Body Font Size"),
      type: "length",
      scope: "book",
      group: "layout",
      default: "12pt",
      presets: ("10.5pt", "11pt", "12pt", "14pt"),
      placeholder: "12pt",
    ),
    (
      key: "margin",
      label: (zh: "页边距", en: "Page Margin"),
      type: "length",
      scope: "book",
      group: "layout",
      default: "2.4cm",
      presets: ("2cm", "2.4cm", "2.54cm"),
      placeholder: "2.4cm",
    ),
    (
      key: "cjk-body-font",
      label: (zh: "中文正文字体", en: "CJK Body Font"),
      type: "font",
      scope: "book",
      group: "layout",
      default: "",
      placeholder: (zh: "默认（宋体）", en: "Default (Songti)"),
    ),
    (
      key: "cjk-heading-font",
      label: (zh: "中文标题字体", en: "CJK Heading Font"),
      type: "font",
      scope: "book",
      group: "layout",
      default: "",
      placeholder: (zh: "默认（黑体）", en: "Default (Heiti)"),
    ),
    (
      key: "show-metadata",
      label: (zh: "无封面时显示题头限值行", en: "Show Limits Row Without Cover"),
      type: "bool",
      scope: "problem",
      group: "problem",
      default: true,
    ),
    (
      key: "prob-type",
      label: (zh: "题目类型", en: "Problem Type"),
      type: "enum",
      scope: "problem",
      group: "problem",
      default: "传统型",
      choices: (
        (value: "传统型", label: (zh: "传统型", en: "Traditional")),
        (value: "交互型", label: (zh: "交互型", en: "Interactive")),
        (value: "提交答案", label: (zh: "提交答案", en: "Output-only")),
      ),
    ),
    (
      key: "source-file",
      label: (zh: "可执行文件名", en: "Executable Filename"),
      type: "string",
      scope: "problem",
      group: "problem",
      default: "",
      placeholder: (zh: "自动从输入文件推导", en: "Inferred from input filename"),
    ),
  ),
)

#let parse-len(s, fallback) = {
  if type(s) == length { return s }
  if type(s) != str { return fallback }
  let t = s.trim()
  if t == "" { return fallback }
  let m = t.match(regex("^([0-9]+(?:\\.[0-9]+)?)\\s*(pt|mm|cm|in|em)$"))
  if m == none { return fallback }
  let num = float(m.captures.at(0))
  let unit = m.captures.at(1)
  if unit == "pt" { num * 1pt }
  else if unit == "mm" { num * 1mm }
  else if unit == "cm" { num * 1cm }
  else if unit == "in" { num * 1in }
  else if unit == "em" { num * 1em }
  else { fallback }
}

#let parse-color(s, fallback) = {
  if type(s) == color { return s }
  if type(s) != str { return fallback }
  let t = s.trim()
  if t.match(regex("^#(?:[0-9a-fA-F]{6}|[0-9a-fA-F]{3}|[0-9a-fA-F]{8})$")) != none {
    rgb(t)
  } else {
    fallback
  }
}

#let with-cjk(latin, cjk) = ((name: latin, covers: "latin-in-cjk"), cjk)
#let song-font = with-cjk("Latin Modern Roman 12", cjk-song)
#let hei-font = with-cjk("Latin Modern Sans 12", cjk-hei)
#let kai-font = with-cjk("Latin Modern Roman 12", cjk-kai)
#let mono-font = with-cjk("Latin Modern Mono 12", cjk-fangsong)
#let code-font = with-cjk("JetBrains Mono", cjk-fangsong)

#let size-er = 22pt
#let size-xiaoer = 18pt
#let size-xiaosan = 15pt
#let size-si = 14pt
#let size-xiaosi = 12pt
#let size-wu = 10.5pt
#let size-xiaowu = 9pt

#let lang-state = state("xcpc-language", "chinese")
#let blank-state = state("xcpc-blank-pages", false)
#let cover-state = state("xcpc-cnoi-has-cover", false)
#let problem-list = state("xcpc-cnoi-problems", ())
#let current-problem-state = state("xcpc-cnoi-current", none)
#let sample-border-state = state("xcpc-cnoi-sample-border", rgb("#0000ff"))
#let sample-linenos-state = state("xcpc-cnoi-sample-linenos", true)
#let body-size-state = state("xcpc-cnoi-body-size", size-xiaosi)
#let hei-font-state = state("xcpc-cnoi-hei-font", hei-font)

#let words = (
  chinese: (
    title: "题目名称",
    prob-type: "题目类型",
    traditional: "传统型",
    source-file: "可执行文件名",
    input-file: "输入文件名",
    output-file: "输出文件名",
    time-limit: "每个测试点时限",
    memory-limit: "内存限制",
    stdin: "标准输入",
    stdout: "标准输出",
    submit-filename: "提交源程序文件名",
    compile-opts: "编译选项",
    for-cpp-left: "对于 C++",
    for-cpp-right: "语言",
    notices-title: "注意事项（请仔细阅读）",
    legend: "题目描述",
    background: "题目背景",
    input: "输入格式",
    output: "输出格式",
    interaction: "交互方式",
    samples: "样例",
    sample-in: "样例输入",
    sample-out: "样例输出",
    explanation: "样例解释",
    constraints: "数据范围",
    subtasks: "子任务",
    scoring: "评分方式",
    note: "提示",
    specification: "题目要求",
    illustration: "图表说明",
    editorial: "题解",
    blank: "此页留白",
  ),
  english: (
    title: "Problem",
    prob-type: "Type",
    traditional: "Traditional",
    source-file: "Executable",
    input-file: "Input File",
    output-file: "Output File",
    time-limit: "Time Limit",
    memory-limit: "Memory Limit",
    stdin: "standard input",
    stdout: "standard output",
    submit-filename: "Source Filenames",
    compile-opts: "Compiler Options",
    for-cpp-left: "For C++",
    for-cpp-right: "",
    notices-title: "Notices (Please Read Carefully)",
    legend: "Problem Statement",
    background: "Background",
    input: "Input Format",
    output: "Output Format",
    interaction: "Interaction",
    samples: "Samples",
    sample-in: "Sample Input",
    sample-out: "Sample Output",
    explanation: "Explanation",
    constraints: "Constraints",
    subtasks: "Subtasks",
    scoring: "Scoring",
    note: "Notes",
    specification: "Specification",
    illustration: "Illustration",
    editorial: "Editorial",
    blank: "This page is intentionally left blank.",
  ),
)

#let word(key) = context words.at(lang-state.get()).at(key)

#let derive-slug(input, output, label) = {
  if input != none and input != "" {
    input.trim(".in", at: end)
  } else if output != none and output != "" {
    output.trim(".out", at: end)
  } else if label != none and label != "" {
    lower(label)
  } else {
    "prog"
  }
}

#let format-problem-heading(label, title, input, output) = {
  if input != none and input != "" {
    let slug = derive-slug(input, output, label)
    [#title（#slug）]
  } else if label != none and label != "" {
    [#title（#label）]
  } else {
    [#title]
  }
}

#let blank-page() = {
  pagebreak(weak: true)
  align(center + horizon, text(font: kai-font, size: 11pt, fill: luma(45%), word("blank")))
  pagebreak(weak: true)
}

#let blank-page-if-even() = context if blank-state.get() and calc.even(counter(page).get().first()) {
  blank-page()
}

#let section(key, title: none) = {
  let heading-text = if title != none { title } else { word(key) }
  v(13pt, weak: true)
  block(breakable: false, sticky: true, below: 8pt)[
    #set par(first-line-indent: 0pt)
    #context if lang-state.get() == "chinese" {
      pad(left: 1.5em, text(font: hei-font, weight: "regular", size: size-xiaosi)[【#heading-text】])
    } else {
      text(font: hei-font, weight: "bold", size: size-xiaosi)[#heading-text]
    }
  ]
}

#let cnoi-sample-box(content-str) = {
  let trimmed = content-str.trim("\n", at: end)
  let lines = if trimmed == "" { ("",) } else { trimmed.split("\n") }
  context {
    let border-col = sample-border-state.get()
    let show-linenos = sample-linenos-state.get()
    let bsize = body-size-state.get()
    pad(
      left: if show-linenos { 0.5em } else { 0pt },
      rect(
        width: 100%,
        stroke: 0.5pt + border-col,
        inset: (x: 8pt, y: 0.65em),
        {
          set par(first-line-indent: 0pt, leading: 0.65em, spacing: 0.3em)
          for (idx, ln) in lines.enumerate() {
            if show-linenos {
              block(above: 0pt, below: if idx + 1 < lines.len() { 0.3em } else { 0pt }, grid(
                columns: (0em, 1fr),
                align: (right + horizon, left + horizon),
                move(
                  dx: -1.25em,
                  text(font: song-font, size: 0.8em, fill: luma(45%), str(idx + 1)),
                ),
                text(font: mono-font, size: bsize, raw(if ln == "" { " " } else { ln })),
              ))
            } else {
              block(above: 0pt, below: if idx + 1 < lines.len() { 0.3em } else { 0pt },
                text(font: mono-font, size: bsize, raw(if ln == "" { " " } else { ln })),
              )
            }
          }
        },
      ),
    )
  }
}

#let samples(..pairs) = {
  let list = pairs.pos()
  for (idx, pair) in list.enumerate() {
    let n = idx + 1
    let (inp, out) = pair
    v(11pt, weak: true)
    block(breakable: false, sticky: true, below: 7pt)[
      #set par(first-line-indent: 0pt)
      #context if lang-state.get() == "chinese" {
        pad(left: 1.5em, text(font: hei-font, weight: "regular", size: size-xiaosi)[【样例 #n 输入】])
      } else {
        text(font: hei-font, weight: "bold", size: size-xiaosi)[Sample #n Input]
      }
    ]
    cnoi-sample-box(inp)
    v(10pt, weak: true)
    block(breakable: false, sticky: true, below: 7pt)[
      #set par(first-line-indent: 0pt)
      #context if lang-state.get() == "chinese" {
        pad(left: 1.5em, text(font: hei-font, weight: "regular", size: size-xiaosi)[【样例 #n 输出】])
      } else {
        text(font: hei-font, weight: "bold", size: size-xiaosi)[Sample #n Output]
      }
    ]
    cnoi-sample-box(out)
    v(6pt, weak: true)
  }
}

#let problem(
  label: none,
  title: "",
  input: none,
  output: none,
  time-limit: none,
  memory-limit: none,
  show-metadata: true,
  options: (:),
  body,
) = {
  blank-page-if-even()
  pagebreak(weak: true)
  let prob-type = options.at("prob-type", default: none)
  let custom-source = options.at("source-file", default: "")
  let effective-slug = if type(custom-source) == str and custom-source.trim() != "" {
    custom-source.trim()
  } else {
    none
  }
  let heading-display = if effective-slug != none {
    [#title（#effective-slug）]
  } else {
    format-problem-heading(label, title, input, output)
  }
  let in-text = if input != none and input != "" { raw(input) } else { word("stdin") }
  let out-text = if output != none and output != "" { raw(output) } else { word("stdout") }
  let time-text = if time-limit != none { time-limit } else { "1.0 秒" }
  let mem-text = if memory-limit != none { memory-limit } else { "512 MiB" }

  problem-list.update(arr => arr + ((
    label: label,
    title: title,
    input: input,
    output: output,
    time: time-limit,
    memory: memory-limit,
    prob-type: prob-type,
    source-file: effective-slug,
  ),))
  current-problem-state.update(heading-display)

  block(width: 100%, breakable: false, above: 4pt, below: 10pt)[
    #set par(first-line-indent: 0pt)
    #metadata((label: label, title: title, display: heading-display)) <xcpc-problem>
    #align(center)[
      #context text(font: hei-font-state.get(), weight: "regular", size: size-xiaoer)[#heading-display]
    ]
    #context if show-metadata and not cover-state.get() {
      v(4pt)
      align(center)[
        #text(size: size-xiaowu, fill: luma(35%))[
          #word("time-limit")：#time-text
          #h(1.2em)
          #word("memory-limit")：#mem-text
          #h(1.2em)
          #word("input-file")：#in-text
          #h(1.2em)
          #word("output-file")：#out-text
        ]
      ]
    }
  ]

  section("legend")
  body
}

#let cover(name, location, date, author, options: (:)) = {
  let compile-opts = options.at("compile-opts", default: "-O2 -std=c++14 -static")
  let show-submit-table = options.at("show-submit-table", default: true)
  let show-compile-table = options.at("show-compile-table", default: true)
  let notices = options.at("notices", default: default-notices)
  let logo = options.at("logo", default: "")
  set par(first-line-indent: 0pt)
  if type(logo) == str and logo.trim() != "" {
    v(8pt)
    align(center, image("/" + logo.trim(), height: 2cm))
    v(8pt)
  } else {
    v(14pt)
  }
  align(center)[
    #context text(font: hei-font-state.get(), weight: "regular", size: size-er)[#name]
    #if location != none and location != "" {
      v(8pt)
      text(font: kai-font, size: size-xiaoer)[#location]
    }
    #if author != none and author != "" {
      v(6pt)
      context text(font: hei-font-state.get(), size: size-xiaosan)[#author]
    }
    #if date != none and date != "" {
      v(8pt)
      text(font: song-font, size: size-si)[时间：#date]
    }
  ]
  v(14pt)

  context {
    let rows = problem-list.final()
    if rows.len() > 0 {
      let n = rows.len()
      let first-col = if n <= 3 { 24% } else { 1.25fr }
      let cols = (first-col,) + (1fr,) * n
      let is-zh = lang-state.get() == "chinese"
      let slug-of(r) = if r.source-file != none and r.source-file != "" {
        r.source-file
      } else {
        derive-slug(r.input, r.output, r.label)
      }

      set text(size: size-wu)
      table(
        columns: cols,
        inset: (x: 6pt, y: 4.5pt),
        stroke: 0.4pt + black,
        align: left + horizon,
        [#word("title")], ..rows.map(r => [#r.title]),
        [#word("prob-type")], ..rows.map(r => if r.prob-type != none and r.prob-type != "" { [#r.prob-type] } else { [#word("traditional")] }),
        [#word("source-file")], ..rows.map(r => raw(slug-of(r))),
        [#word("input-file")], ..rows.map(r => if r.input != none and r.input != "" { raw(r.input) } else { word("stdin") }),
        [#word("output-file")], ..rows.map(r => if r.output != none and r.output != "" { raw(r.output) } else { word("stdout") }),
        [#word("time-limit")], ..rows.map(r => if r.time != none { [#r.time] } else { [1.0 秒] }),
        [#word("memory-limit")], ..rows.map(r => if r.memory != none { [#r.memory] } else { [512 MiB] }),
      )

      if show-submit-table {
        v(6pt)
        pad(left: 1.5em, text(size: size-wu, word("submit-filename")))
        v(2pt)
        table(
          columns: cols,
          inset: (x: 6pt, y: 4.5pt),
          stroke: 0.4pt + black,
          align: left + horizon,
          [#word("for-cpp-left") #word("for-cpp-right")],
          ..rows.map(r => raw(slug-of(r) + ".cpp")),
        )
      }

      if show-compile-table {
        v(6pt)
        pad(left: 1.5em, text(size: size-wu, word("compile-opts")))
        v(2pt)
        table(
          columns: cols,
          inset: (x: 6pt, y: 4.5pt),
          stroke: 0.4pt + black,
          align: (left + horizon, center + horizon),
          [#word("for-cpp-left") #word("for-cpp-right")],
          table.cell(colspan: n, raw(compile-opts)),
        )
      }

      if is-zh and type(notices) == array and notices.len() > 0 {
        v(10pt)
        pad(left: 1.5em, strong(text(size: size-wu, word("notices-title"))))
        v(4pt)
        set text(size: size-wu)
        set enum(indent: 1.5em, spacing: 0.45em)
        enum(..notices.map(n => [#n]))
      }
    }
  }
  pagebreak(weak: true)
}

#let editorial-cover(name, location, date, author, logo: "") = {
  set par(first-line-indent: 0pt)
  if type(logo) == str and logo.trim() != "" {
    v(24pt)
    align(center, image("/" + logo.trim(), height: 2cm))
    v(12pt)
  } else {
    v(42pt)
  }
  align(center)[
    #text(font: hei-font, weight: "regular", size: size-er)[#name]
    #v(10pt)
    #text(font: hei-font, weight: "regular", size: size-xiaoer)[参考题解]
    #if location != none and location != "" {
      v(10pt)
      text(font: kai-font, size: size-xiaosan)[#location]
    }
    #if author != none and author != "" {
      v(6pt)
      text(font: song-font, size: size-si)[#author]
    }
    #if date != none and date != "" {
      v(8pt)
      text(font: song-font, size: size-xiaosi, fill: luma(35%))[时间：#date]
    }
  ]
  v(28pt)
  context {
    let probs = query(selector(<xcpc-problem>))
    if probs.len() > 0 {
      set text(size: size-wu)
      align(center, table(
        columns: (4em, 1fr, 5em),
        inset: (x: 10pt, y: 6pt),
        stroke: (x, y) => (
          top: if y == 0 { 1.2pt + black } else if y == 1 { 0.6pt + black } else { none },
          bottom: none,
        ),
        align: (center + horizon, left + horizon, right + horizon),
        table.header([*题号*], [*题目名称*], [*页码*]),
        ..probs.map(el => {
          let pg = el.location().page()
          let v = el.value
          (
            [#v.label],
            link(el.location(), [#v.title]),
            link(el.location(), [第 #pg 页]),
          )
        }).flatten(),
        table.hline(stroke: 1.2pt + black),
      ))
    }
  }
  pagebreak(weak: true)
}

#let tutorial(label: none, title: none, meta: (), body) = {
  pagebreak(weak: true)
  let heading-display = if label != none and label != "" { [#title（#label）] } else { [#title] }
  current-problem-state.update(heading-display)
  block(width: 100%, breakable: false, sticky: true, below: 12pt)[
    #set par(first-line-indent: 0pt)
    #metadata((label: label, title: title, display: heading-display)) <xcpc-problem>
    #align(center)[
      #text(font: hei-font, weight: "regular", size: size-xiaoer)[#heading-display]
    ]
    #if meta.len() > 0 {
      v(4pt)
      align(center)[
        #text(size: size-xiaowu, fill: luma(40%))[#meta.join("　")]
      ]
    }
  ]
  body
}

#let subsection(title) = {
  v(10pt, weak: true)
  block(width: 100%, breakable: false, sticky: true, below: 5pt)[
    #set par(first-line-indent: 0pt)
    #text(font: hei-font, weight: "regular", size: size-xiaosi)[#title]
  ]
}

#let runin(title) = {
  text(font: hei-font, weight: "regular", title)
  h(0.8em)
}

#let contest(
  name: "",
  location: none,
  date: none,
  author: none,
  language: "chinese",
  show-cover: false,
  blank-pages: false,
  book: "statements",
  pdf-title: none,
  pdf-author: none,
  options: (:),
  body,
) = {
  let lang = if language == "english" { "english" } else { "chinese" }
  let eff-body-size = parse-len(options.at("body-size", default: "12pt"), size-xiaosi)
  let eff-margin = parse-len(options.at("margin", default: "2.4cm"), 2.4cm)
  let eff-border-color = parse-color(options.at("sample-border-color", default: "#0000ff"), rgb("#0000ff"))
  let eff-linenos = options.at("sample-line-numbers", default: true)
  let eff-dotted = options.at("dotted-emphasis", default: true)
  let opt-song = options.at("cjk-body-font", default: "")
  let eff-song-font = if type(opt-song) == str and opt-song.trim() != "" { with-cjk("Latin Modern Roman 12", opt-song.trim()) } else { song-font }
  let opt-hei = options.at("cjk-heading-font", default: "")
  let eff-hei-font = if type(opt-hei) == str and opt-hei.trim() != "" { with-cjk("Latin Modern Sans 12", opt-hei.trim()) } else { hei-font }

  set document(
    title: if pdf-title != none { pdf-title } else { name },
    author: if pdf-author != none { (pdf-author,) } else if author != none { (author,) } else { () },
  )
  set page(
    paper: "a4",
    margin: eff-margin,
    header: context {
      let page-num = counter(page).get().first()
      let probs = query(selector(<xcpc-problem>)).filter(el => el.location().page() <= page-num)
      let cur = if probs.len() > 0 { probs.last().value.display } else { none }
      if (page-num > 1 or not show-cover) and cur != none {
        set par(first-line-indent: 0pt)
        set text(font: eff-song-font, size: size-wu, fill: black)
        block(width: 100%, {
          grid(
            columns: (1fr, auto),
            [#name],
            [#cur],
          )
          v(3pt)
          line(length: 100%, stroke: 0.35pt + black)
        })
      }
    },
    footer: context {
      let page-num = counter(page).get().first()
      if page-num > 1 or not show-cover {
        set par(first-line-indent: 0pt)
        set text(font: eff-song-font, size: size-wu, fill: black)
        let total = counter(page).final().first()
        align(center)[
          #if lang == "chinese" [
            第 #page-num 页#h(2em)共 #link((page: total, x: 0pt, y: 0pt), text(fill: rgb("#0000ff"), [#total])) 页
          ] else [
            Page #page-num #h(1.5em) of #link((page: total, x: 0pt, y: 0pt), text(fill: rgb("#0000ff"), [#total]))
          ]
        ]
      }
    },
  )

  set text(
    font: eff-song-font,
    size: eff-body-size,
    lang: if lang == "chinese" { "zh" } else { "en" },
    region: if lang == "chinese" { "CN" } else { "US" },
  )
  set par(
    justify: true,
    leading: 0.7em,
    spacing: 0.6em,
    first-line-indent: if lang == "chinese" { (amount: 2em, all: true) } else { 0pt },
  )
  set list(indent: 2em, marker: ([•], [◦], [▪]))
  set enum(indent: 2em)

  show emph: set text(font: kai-font)
  show strong: it => {
    set text(font: eff-hei-font, weight: "regular")
    if eff-dotted and lang == "chinese" and book == "statements" {
      show regex("\p{sc=Hani}+"): s => {
        underline(
          s,
          offset: 3pt,
          stroke: (
            cap: "round",
            thickness: 0.1em,
            dash: (array: (0em, 1em), phase: 0.5em),
          ),
        )
      }
      it
    } else {
      it
    }
  }
  show heading: it => {
    show underline: u => u.body
    it
  }
  show block.where(sticky: true): it => {
    show underline: u => u.body
    it
  }
  show math.equation: set text(font: "Latin Modern Math", size: eff-body-size)
  show raw.where(block: false): set text(font: mono-font, size: eff-body-size)
  show raw.where(block: true): it => {
    set text(font: code-font, size: 9.5pt, ligatures: false, features: (calt: 0))
    set par(first-line-indent: 0pt, leading: 0.55em)
    block(
      width: 100%,
      stroke: 0.5pt + luma(70%),
      fill: luma(98%),
      inset: (x: 8pt, y: 6pt),
      radius: 2pt,
      it,
    )
  }

  // Academic three-line style for Markdown tables (e.g. data constraints / subtasks)
  set table(
    stroke: (x, y) => (
      left: if x > 0 { 0.4pt + black } else { none },
      top: if y == 0 { 1.5pt + black } else if y == 1 { 0.9pt + black } else { 0.4pt + black },
      bottom: 1.5pt + black,
    ),
    inset: (x: 8pt, y: 4.5pt),
    align: center + horizon,
  )
  show table: it => {
    show underline: u => u.body
    pad(y: 4pt, align(center, it))
  }

  lang-state.update(lang)
  blank-state.update(blank-pages)
  cover-state.update(show-cover)
  sample-border-state.update(eff-border-color)
  sample-linenos-state.update(eff-linenos)
  body-size-state.update(eff-body-size)
  hei-font-state.update(eff-hei-font)

  if show-cover {
    if book == "editorial" {
      editorial-cover(name, location, date, author, logo: options.at("logo", default: ""))
    } else {
      cover(name, location, date, author, options: options)
    }
  }

  body
}

// ---- Single-entry render contract -----------------------------------------------------

#let render(doc) = {
  let opts = doc.at("options", default: (:))
  let book-kind = doc.at("book", default: "statements")
  let editorial = book-kind == "editorial"
  let single = doc.at("single", default: false)
  let problems = doc.at("problems", default: ())

  let cover-opt = opts.at("cover", default: "auto")
  let show-cover = if single {
    false
  } else if cover-opt == "on" or cover-opt == true {
    true
  } else if cover-opt == "off" or cover-opt == false {
    false
  } else {
    problems.len() >= 2
  }
  let blank-pages = if editorial { false } else { opts.at("blank-pages", default: false) == true }
  let book-show-meta = opts.at("show-metadata", default: true) != false

  contest(
    name: doc.at("name", default: ""),
    location: doc.at("location", default: none),
    date: doc.at("date", default: none),
    author: doc.at("author", default: none),
    language: doc.at("language", default: "chinese"),
    show-cover: show-cover,
    blank-pages: blank-pages,
    book: book-kind,
    pdf-title: doc.at("pdf-title", default: none),
    pdf-author: doc.at("pdf-author", default: none),
    options: opts,
    {
      for p in problems {
        if editorial {
          let lbl = if single { none } else { p.at("label", default: none) }
          tutorial(
            label: lbl,
            title: p.at("title", default: none),
            meta: p.at("meta", default: ()),
            p.at("body", default: []),
          )
        } else {
          let p-opts = p.at("options", default: (:))
          let show-meta = p-opts.at("show-metadata", default: book-show-meta) != false
          let lbl = if single { none } else { p.at("label", default: none) }
          problem(
            label: lbl,
            title: p.at("title", default: ""),
            input: p.at("input", default: none),
            output: p.at("output", default: none),
            time-limit: p.at("time-limit", default: none),
            memory-limit: p.at("memory-limit", default: none),
            show-metadata: show-meta,
            options: p-opts,
            p.at("body", default: []),
          )
        }
      }
    },
  )
}
