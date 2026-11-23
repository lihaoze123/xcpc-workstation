// minimal: 极简空白起点模板，方便从零编写自定义比赛模板。
// 只需导出 `#let render(doc)` 即可接管整本 PDF 排版；section / samples / subsection / runin
// 已由 `/xcpc/defaults.typ` 提供默认实现，按需在此定义同名函数即可覆盖。

#import "/xcpc/fonts.typ": cjk-fangsong, cjk-hei, cjk-song

#let with-cjk(latin, cjk) = ((name: latin, covers: "latin-in-cjk"), cjk)

#let schema = (
  name: (zh: "极简空白", en: "Minimal Blank"),
  description: (
    zh: "极简空白起点模板，方便从零定制比赛版式",
    en: "Minimal blank starter template for building custom contest layouts from scratch",
  ),
  groups: (
    (id: "layout", label: (zh: "页面与封面", en: "Page & Cover")),
  ),
  fields: (
    (
      key: "body-size",
      label: (zh: "正文字号", en: "Body Font Size"),
      type: "length",
      scope: "book",
      group: "layout",
      default: "11pt",
      presets: ("10pt", "11pt", "12pt"),
    ),
    (
      key: "margin",
      label: (zh: "页边距", en: "Page Margin"),
      type: "length",
      scope: "book",
      group: "layout",
      default: "2cm",
      presets: ("1.8cm", "2cm", "2.5cm"),
    ),
    (
      key: "show-cover",
      label: (zh: "显示封面", en: "Show Cover"),
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
  set text(
    font: with-cjk("Latin Modern Roman 12", cjk-song),
    size: body-size,
    lang: if doc.language == "english" { "en" } else { "zh" },
  )
  show strong: set text(font: with-cjk("Latin Modern Roman 12", cjk-hei))
  show math.equation: set text(font: "Latin Modern Math")
  show raw.where(block: false): set text(font: with-cjk("Latin Modern Mono 12", cjk-fangsong))
  show raw.where(block: true): set text(font: with-cjk("JetBrains Mono", cjk-fangsong))
  set par(justify: true)

  if show-cover {
    align(center + horizon)[
      #text(size: 20pt, weight: "bold", doc.name)
      #if doc.date != none { v(0.6em); text(size: 11pt, doc.date) }
    ]
    pagebreak()
  }

  let single = doc.at("single", default: false)
  for (idx, p) in doc.problems.enumerate() {
    if idx > 0 { pagebreak(weak: true) }
    let lbl = if single { none } else { p.at("label", default: none) }
    if editorial {
      [#metadata((label: lbl, title: p.title)) <xcpc-tutorial>]
    } else {
      [#metadata((label: lbl, title: p.title)) <xcpc-problem>]
    }
    let head = if lbl == none or lbl == "" { [#p.title] } else { [#lbl. #p.title] }
    block(sticky: true, below: 0.6em, text(size: 16pt, weight: "bold", head))
    p.body
  }
}
