// CNOI / OI style statement & editorial template for XCPC Workstation.
// Inspired by the classic CNOI / NOI contest PDF layout (cf. fuzzy-cnoi-statement):
// centered competition title, problem summary table on the cover, bracketed 【章节】
// headings, and boxed per-sample input/output blocks.
//
// Exports the exact same contract as `/xcpc/olymp-classic.typ` (`contest`, `problem`,
// `section`, `samples`, `tutorial`, `subsection`, `runin`) so any workspace or contest
// can switch templates via `template = "cnoi-classic"` without changing Markdown files.

#import "/xcpc/fonts.typ": cjk-song, cjk-hei, cjk-kai, cjk-fangsong

#let with-cjk(latin, cjk) = ((name: latin, covers: "latin-in-cjk"), cjk)
#let song-font = with-cjk("Latin Modern Roman 12", cjk-song)
#let hei-font = with-cjk("Latin Modern Sans 12", cjk-hei)
#let kai-font = with-cjk("Latin Modern Roman 12", cjk-kai)
#let mono-font = with-cjk("Latin Modern Mono 12", cjk-fangsong)
#let code-font = with-cjk("JetBrains Mono", cjk-fangsong)

#let lang-state = state("xcpc-language", "chinese")
#let blank-state = state("xcpc-blank-pages", false)
#let problem-list = state("xcpc-cnoi-problems", ())

#let words = (
  chinese: (
    title: "题目名称",
    source-file: "可执行文件名",
    input-file: "输入文件名",
    output-file: "输出文件名",
    time-limit: "时间限制",
    memory-limit: "空间限制",
    stdin: "标准输入",
    stdout: "标准输出",
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
    page-of: (n, total) => [第 #n 页　共 #total 页],
  ),
  english: (
    title: "Problem",
    source-file: "Executable",
    input-file: "Input File",
    output-file: "Output File",
    time-limit: "Time Limit",
    memory-limit: "Memory Limit",
    stdin: "standard input",
    stdout: "standard output",
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
    page-of: (n, total) => [Page #n of #total],
  ),
)

#let word(key) = context words.at(lang-state.get()).at(key)

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
  v(10pt, weak: true)
  block(breakable: false, sticky: true, below: 6pt)[
    #context if lang-state.get() == "chinese" {
      text(font: hei-font, weight: "bold", size: 12pt)[【#heading-text】]
    } else {
      text(font: hei-font, weight: "bold", size: 12pt)[#heading-text]
    }
  ]
}

#let samples(..pairs) = {
  let list = pairs.pos()
  for (idx, pair) in list.enumerate() {
    let n = idx + 1
    let (inp, out) = pair
    v(8pt, weak: true)
    block(breakable: false, sticky: true, below: 4pt)[
      #context if lang-state.get() == "chinese" {
        text(font: hei-font, weight: "bold", size: 11.5pt)[【样例 #n 输入】]
      } else {
        text(font: hei-font, weight: "bold", size: 11.5pt)[Sample #n Input]
      }
    ]
    block(
      width: 100%,
      stroke: 0.5pt + luma(65%),
      fill: luma(98%),
      inset: (x: 8pt, y: 6pt),
      radius: 2pt,
      below: 6pt,
    )[
      #set par(first-line-indent: 0pt, leading: 0.55em)
      #text(font: mono-font, size: 10pt, raw(inp.trim("\n", at: end)))
    ]
    block(breakable: false, sticky: true, below: 4pt)[
      #context if lang-state.get() == "chinese" {
        text(font: hei-font, weight: "bold", size: 11.5pt)[【样例 #n 输出】]
      } else {
        text(font: hei-font, weight: "bold", size: 11.5pt)[Sample #n Output]
      }
    ]
    block(
      width: 100%,
      stroke: 0.5pt + luma(65%),
      fill: luma(98%),
      inset: (x: 8pt, y: 6pt),
      radius: 2pt,
      below: 8pt,
    )[
      #set par(first-line-indent: 0pt, leading: 0.55em)
      #text(font: mono-font, size: 10pt, raw(out.trim("\n", at: end)))
    ]
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
  body,
) = {
  blank-page-if-even()
  pagebreak(weak: true)
  let tag = if label != none and label != "" { [#label. #title] } else { [#title] }
  let in-text = if input != none and input != "" { raw(input) } else { word("stdin") }
  let out-text = if output != none and output != "" { raw(output) } else { word("stdout") }
  let time-text = if time-limit != none { time-limit } else { "–" }
  let mem-text = if memory-limit != none { memory-limit } else { "–" }

  problem-list.update(arr => arr + ((
    label: label,
    title: title,
    input: input,
    output: output,
    time: time-limit,
    memory: memory-limit,
  ),))

  block(width: 100%, breakable: false, below: 12pt)[
    #metadata((label: label, title: title)) <xcpc-problem>
    #align(center)[
      #text(font: hei-font, weight: "bold", size: 16pt)[#tag]
    ]
    #if show-metadata {
      v(6pt)
      align(center)[
        #table(
          columns: (auto, auto, auto, auto),
          inset: (x: 10pt, y: 4pt),
          stroke: 0.5pt + luma(60%),
          align: center + horizon,
          text(font: hei-font, size: 9.5pt, word("input-file")),
          text(font: hei-font, size: 9.5pt, word("output-file")),
          text(font: hei-font, size: 9.5pt, word("time-limit")),
          text(font: hei-font, size: 9.5pt, word("memory-limit")),
          text(size: 9.5pt, in-text),
          text(size: 9.5pt, out-text),
          text(size: 9.5pt, time-text),
          text(size: 9.5pt, mem-text),
        )
      ]
    }
  ]

  body
}

#let cover(name, location, date, author) = {
  v(24pt)
  align(center)[
    #text(font: hei-font, weight: "bold", size: 22pt)[#name]
    #v(10pt)
    #let sub = (location, date, author).filter(x => x != none and x != "")
    #if sub.len() > 0 {
      text(font: kai-font, size: 12pt, fill: luma(30%))[#sub.join("　")]
    }
  ]
  v(22pt)
  context {
    let rows = problem-list.final()
    if rows.len() > 0 {
      set text(size: 10pt)
      table(
        columns: (auto, 1fr, auto, auto, auto, auto),
        inset: (x: 7pt, y: 5pt),
        stroke: 0.5pt + luma(50%),
        align: center + horizon,
        table.header(
          text(font: hei-font, weight: "bold")[\#],
          text(font: hei-font, weight: "bold", word("title")),
          text(font: hei-font, weight: "bold", word("input-file")),
          text(font: hei-font, weight: "bold", word("output-file")),
          text(font: hei-font, weight: "bold", word("time-limit")),
          text(font: hei-font, weight: "bold", word("memory-limit")),
        ),
        ..rows.map(r => (
          [#r.label],
          align(left, [#r.title]),
          if r.input != none and r.input != "" { raw(r.input) } else { word("stdin") },
          if r.output != none and r.output != "" { raw(r.output) } else { word("stdout") },
          if r.time != none { [#r.time] } else { [–] },
          if r.memory != none { [#r.memory] } else { [–] },
        )).flatten(),
      )
    }
  }
  pagebreak(weak: true)
}

#let tutorial(label: none, title: none, meta: (), body) = {
  pagebreak(weak: true)
  block(breakable: false, sticky: true, below: 10pt)[
    #metadata((label: label, title: title)) <xcpc-problem>
    #text(font: hei-font, weight: "bold", size: 15pt)[#if label != none [#label. ]#title]
    #if meta.len() > 0 {
      v(3pt)
      text(size: 9.5pt, fill: luma(40%))[#meta.join(" / ")]
    }
  ]
  body
}

#let subsection(title) = {
  v(8pt, weak: true)
  block(breakable: false, sticky: true, below: 4pt)[
    #text(font: hei-font, weight: "bold", size: 11.5pt)[#title]
  ]
}

#let runin(title) = {
  strong(title)
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
  body,
) = {
  let lang = if language == "english" { "english" } else { "chinese" }
  set document(
    title: if pdf-title != none { pdf-title } else { name },
    author: if pdf-author != none { (pdf-author,) } else if author != none { (author,) } else { () },
  )
  set page(
    paper: "a4",
    margin: (top: 25mm, bottom: 24mm, left: 22mm, right: 22mm),
    header: context {
      if counter(page).get().first() > 1 or not show-cover {
        set text(font: kai-font, size: 9.5pt, fill: luma(35%))
        grid(
          columns: (1fr, auto),
          [#name],
          if book == "editorial" { word("editorial") } else { [] },
        )
        v(-6pt)
        line(length: 100%, stroke: 0.4pt + luma(55%))
      }
    },
    footer: context {
      let cur = counter(page).get().first()
      let total = counter(page).final().first()
      let fmt = words.at(lang).at("page-of")
      align(center, text(font: song-font, size: 9.5pt, fill: luma(35%), fmt(cur, total)))
    },
  )
  set text(
    font: song-font,
    size: 11pt,
    lang: if lang == "chinese" { "zh" } else { "en" },
    cjk-latin-spacing: auto,
  )
  set par(
    justify: true,
    leading: 0.72em,
    first-line-indent: if lang == "chinese" { 2em } else { 0pt },
  )
  show raw.where(block: true): set text(font: code-font, size: 9.5pt)
  show raw.where(block: false): set text(font: mono-font, size: 10.5pt)
  show math.equation: set text(font: "Latin Modern Math")

  lang-state.update(lang)
  blank-state.update(blank-pages)

  if show-cover {
    cover(name, location, date, author)
  }

  body
}
