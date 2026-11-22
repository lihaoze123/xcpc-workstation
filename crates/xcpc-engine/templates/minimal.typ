// minimal: 极简空白起点模板，方便从零编写自定义比赛模板。
// 只需导出 `#let render(doc)` 即可接管整本 PDF 排版；section / samples / subsection / runin
// 已由 `/xcpc/defaults.typ` 提供默认实现，按需在此定义同名函数即可覆盖。

#let schema = (
  name: "极简空白",
  description: "极简空白起点模板，方便从零定制比赛版式",
  groups: (
    (id: "layout", label: "页面与封面"),
  ),
  fields: (
    (
      key: "body-size",
      label: "正文字号",
      type: "length",
      scope: "book",
      group: "layout",
      default: "11pt",
      presets: ("10pt", "11pt", "12pt"),
    ),
    (
      key: "margin",
      label: "页边距",
      type: "length",
      scope: "book",
      group: "layout",
      default: "2cm",
      presets: ("1.8cm", "2cm", "2.5cm"),
    ),
    (
      key: "show-cover",
      label: "显示封面",
      type: "bool",
      scope: "book",
      group: "layout",
      default: false,
    ),
  ),
)

#let render(doc) = {
  let opts = doc.at("options", default: (:))
  let body-size = eval(opts.at("body-size", default: "11pt"))
  let margin = eval(opts.at("margin", default: "2cm"))
  let show-cover = opts.at("show-cover", default: false) == true and not doc.at("single", default: false)
  let editorial = doc.at("book", default: "statements") == "editorial"

  set document(
    title: if doc.pdf-title != none { doc.pdf-title } else { doc.name },
    author: if doc.pdf-author != none { (doc.pdf-author,) } else { () },
  )
  set page(paper: "a4", margin: margin, numbering: "1 / 1")
  set text(size: body-size, lang: if doc.language == "english" { "en" } else { "zh" })
  set par(justify: true)

  if show-cover {
    align(center + horizon)[
      #text(size: 20pt, weight: "bold", doc.name)
      #if doc.date != none { v(0.6em); text(size: 11pt, doc.date) }
    ]
    pagebreak()
  }

  for (idx, p) in doc.problems.enumerate() {
    if idx > 0 { pagebreak(weak: true) }
    if editorial {
      [#metadata((label: p.label, title: p.title)) <xcpc-tutorial>]
    } else {
      [#metadata((label: p.label, title: p.title)) <xcpc-problem>]
    }
    block(sticky: true, below: 0.6em, text(size: 16pt, weight: "bold")[#p.label. #p.title])
    p.body
  }
}
