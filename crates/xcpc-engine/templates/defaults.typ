// Default statement & editorial helper hooks for XCPC Workstation templates.
//
// Imported automatically before any workspace template so minimal templates only need to
// export `#let render(doc)`. Any template that exports its own `section`, `samples`,
// `subsection`, or `runin` transparently overrides these defaults.

#let _default-titles = (
  input: "输入格式",
  output: "输出格式",
  interaction: "交互",
  example: "样例",
  examples: "样例",
  explanation: "样例解释",
  note: "提示",
  notes: "提示",
  scoring: "评分",
  constraints: "数据范围",
  background: "题目背景",
  specification: "题目要求",
  illustration: "图表说明",
  subtasks: "子任务",
)

#let section(key, title: none) = {
  let heading-text = if title != none { title } else { _default-titles.at(key, default: key) }
  v(0.8em, weak: true)
  block(sticky: true, below: 0.45em, text(weight: "bold", size: 1.1em, heading-text))
}

#let samples(..pairs) = {
  let list = pairs.pos()
  if list.len() == 0 { return }
  section(if list.len() == 1 { "example" } else { "examples" })
  let cell(body) = raw(body.trim("\n", at: end))
  table(
    columns: (1fr, 1fr),
    stroke: 0.5pt + luma(60%),
    inset: (x: 6pt, y: 4pt),
    table.header(strong[标准输入], strong[标准输出]),
    ..list.map(((inp, out)) => (cell(inp), cell(out))).flatten(),
  )
}

#let subsection(title) = {
  v(0.9em, weak: true)
  block(sticky: true, below: 0.4em, text(weight: "bold", size: 1.05em, title))
}

#let runin(title) = {
  strong(title)
  h(0.8em)
}
