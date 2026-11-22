// olymp-classic: the look of polygon-chinese-template@10f9959 (olymp.sty + ctexart, 12pt).
//
// The goal is to look the same, not to reproduce TeX's line-by-line arithmetic: the
// values below are the design parameters of olymp.sty and ctexart (their sizes,
// skips, rules and widths), and Typst does the typesetting. See docs/statement-style.md §1.

// TeX points (1/72.27 in) to Typst points (1/72 in).
#let tex(x) = x * 72 / 72.27 * 1pt

// ---- Declarative Template Schema ------------------------------------------------------

#let schema = (
  name: "XCPC 经典",
  description: "对齐 olymp.sty 的经典 ICPC / XCPC 版式",
  groups: (
    (id: "layout", label: "页面与字体"),
    (id: "problem", label: "单题排版"),
  ),
  fields: (
    (
      key: "body-size",
      label: "正文字号",
      type: "length",
      scope: "book",
      group: "layout",
      default: "12pt",
      presets: ("10pt", "11pt", "12pt", "13pt"),
      placeholder: "12pt",
    ),
    (
      key: "margin-x",
      label: "左右页边距",
      type: "length",
      scope: "book",
      group: "layout",
      default: "17.5mm",
      presets: ("15mm", "17.5mm", "20mm", "25mm"),
      placeholder: "17.5mm",
    ),
    (
      key: "margin-y",
      label: "上下页边距",
      type: "length",
      scope: "book",
      group: "layout",
      default: "26mm",
      presets: ("20mm", "26mm", "30mm"),
      placeholder: "26mm",
    ),
    (
      key: "cjk-body-font",
      label: "中文正文字体",
      type: "font",
      scope: "book",
      group: "layout",
      default: "",
      placeholder: "默认（方正书宋）",
    ),
    (
      key: "cjk-heading-font",
      label: "中文标题字体",
      type: "font",
      scope: "book",
      group: "layout",
      default: "",
      placeholder: "默认（方正黑体）",
    ),
    (
      key: "sample-layout",
      label: "样例排版",
      type: "enum",
      scope: "problem",
      group: "problem",
      default: "two-column",
      choices: (
        (value: "two-column", label: "双栏并排"),
        (value: "wide", label: "上下宽版"),
      ),
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

// ---- Design parameters ----------------------------------------------------------------

#let text-width = 175mm                       // olymp.sty, A4 portrait, 12pt
#let text-height = tex(697.09)                // 245mm, rounded to whole lines by ctex
#let text-left = tex(72.27 - 56.91 + 31.0)    // 1in + \hoffset + \oddsidemargin
#let text-top = tex(72.27 - 99.58 + 20.0 + 56.91 + 17.07) // + \topmargin + \headheight + \headsep

// ctex's \linespread{1.3} over LaTeX's baselines: about 1.57 × the font size at every size.
// A TeX strut: 70% of the baselineskip above the baseline, 30% below, so lines set with
// it and no leading sit one baselineskip apart. The body uses the normal size's strut
// throughout (TeX ends size groups before \par, so headings take the body's spacing too).
#let strut(size) = (top-edge: 0.7 * 1.57 * size, bottom-edge: -0.3 * 1.57 * size)
#let parindent = 2em                          // ctex
#let smallskip = tex(3)
#let bigskip = tex(12)
#let tabcolsep = tex(6)
#let rule = 0.4pt

#let sizes = (normal: tex(12), small: tex(10.95), footnote: tex(10), large: tex(14.4), Large: tex(17.28), LARGE: tex(20.74))

// ---- Fonts ----------------------------------------------------------------------------

#import "/xcpc/fonts.typ": cjk-fangsong, cjk-hei, cjk-kai, cjk-song

// Latin Modern ships optical sizes; fontspec picks 10 below 11pt, 12 up to 15pt, 17 above.
#let optical(family, size) = family + if size < tex(11) { "" } else if size < tex(15) { " 12" } else { " 17" }
#let with-cjk(latin, cjk) = ((name: latin, covers: "latin-in-cjk"), cjk)

#let serif(size: sizes.normal, cjk: cjk-song, body) = text(font: with-cjk(optical("Latin Modern Roman", size), cjk), size: size, body)
#let kai(size: sizes.normal, body) = serif(size: size, cjk: cjk-kai, body)
#let sans(size: sizes.normal, body) = text(font: with-cjk(optical("Latin Modern Sans", size), cjk-hei), size: size, body)
// Bold sans exists only at the 10pt design size; CJK bold is FZ Hei.
#let bold-sans(size, body) = text(font: with-cjk("Latin Modern Sans", cjk-hei), weight: "bold", size: size, body)
#let mono-font = with-cjk("Latin Modern Mono 12", cjk-fangsong)
#let code-font = with-cjk("JetBrains Mono", cjk-fangsong)
#let mono(body) = text(font: mono-font, body)

// ---- Words ----------------------------------------------------------------------------

#let words = (
  chinese: (
    problem: "Problem", input-file: "输入文件", output-file: "输出文件",
    time-limit: "时间限制", memory-limit: "内存限制", stdin: "标准输入", stdout: "标准输出",
    input: "输入格式", output: "输出格式", interaction: "交互", example: "样例", examples: "样例",
    explanation: "样例解释", note: "提示", notes: "提示", scoring: "评分", constraints: "约束",
    background: "背景知识", specification: "题目要求", illustration: "图表说明", subtasks: "子任务",
    blank: "此页故意留白", editorial: "题解",
  ),
  english: (
    problem: "Problem", input-file: "Input file:", output-file: "Output file:",
    time-limit: "Time limit:", memory-limit: "Memory limit:", stdin: "standard input", stdout: "standard output",
    input: "Input", output: "Output", interaction: "Interaction Protocol", example: "Example", examples: "Examples",
    explanation: "Explanation", note: "Note", notes: "Notes", scoring: "Scoring", constraints: "Constraints",
    background: "Background", specification: "Specification", illustration: "Illustration", subtasks: "Subtasks",
    blank: "This page is intentionally left blank", editorial: "Editorial",
  ),
)
#let lang-state = state("xcpc-language", "chinese")
#let blank-state = state("xcpc-blank-pages", false)
#let sample-layout-state = state("xcpc-sample-layout", "two-column")
#let cjk-song-state = state("xcpc-olymp-song", cjk-song)
#let cjk-hei-state = state("xcpc-olymp-hei", cjk-hei)
#let word(key) = context words.at(lang-state.get()).at(key)

// \problemend with `intentionallyblankpages`: every problem starts on an odd page; an
// even page before one says it is blank, in \problemheadfont, centered on the page.
#let blank-page(last: false) = {
  v(1fr)
  align(center, serif(size: sizes.LARGE, word("blank")))
  v(1fr)
  if not last { pagebreak() }
}
#let blank-page-if-even() = context if blank-state.get() and calc.even(counter(page).get().first()) {
  blank-page()
}

// ---- Statement parts ------------------------------------------------------------------

// \smallbreak\smallskip before, \smallskip after; weak spacing merges with the glue
// before it, as \smallbreak does. A heading the template has no word for gives its title.
#let section(key, title: none) = {
  v(2 * smallskip, weak: true)
  block(sticky: true, below: smallskip, bold-sans(sizes.Large, if title == none { word(key) } else { title }))
}

// One table for all samples, kept on one page (the heading moves with it), as the
// reference's tabular is. A table taller than a whole page, which the reference lets
// run off the page, breaks across pages instead, its head repeated on each.
#let samples(..pairs) = {
  let pairs = pairs.pos()
  let cell(body) = raw(body.trim("\n", at: end))
  let head(key) = align(center, mono(text(weight: "bold", word(key))))
  section(if pairs.len() == 1 { "example" } else { "examples" })
  context {
    let layout = sample-layout-state.get()
    let body = if layout == "wide" {
      let col = text-width - parindent
      pad(left: parindent, table(
        columns: (col,),
        stroke: rule,
        inset: (x: tabcolsep, y: 2pt),
        ..pairs.map(((input, output)) => (
          head("stdin"),
          cell(input),
          head("stdout"),
          cell(output),
        )).flatten(),
      ))
    } else {
      let column = 0.43 * text-width + 2 * tabcolsep
      pad(left: parindent, table(
        columns: (column, column),
        stroke: rule,
        inset: (x: tabcolsep, y: 0pt),
        table.header(head("stdin"), head("stdout"), repeat: true),
        ..pairs.map(((input, output)) => (cell(input), cell(output))).flatten(),
      ))
    }
    block(breakable: measure(body, width: text-width).height > text-height, below: smallskip, body)
  }
}

#let problem(
  title: none,
  label: none,
  input: none,
  output: none,
  time-limit: none,
  memory-limit: none,
  show-metadata: true,
  options: (:),
  body,
) = {
  pagebreak(weak: true)
  blank-page-if-even()
  let sample-layout = options.at("sample-layout", default: "two-column")
  sample-layout-state.update(sample-layout)
  [#metadata((label: label, title: title)) <xcpc-problem>]
  block(sticky: true, below: 3mm, context text(font: with-cjk("Latin Modern Sans", cjk-hei-state.get()), weight: "bold", size: sizes.LARGE)[#word("problem") #label. #title])
  if show-metadata {
    // No file name: standard input and output, which the reference sets as file names.
    let file(name, key) = mono(if name == none { word(key) } else { name })
    let rows = (
      ("input-file", file(input, "stdin")), ("output-file", file(output, "stdout")),
      ("time-limit", time-limit), ("memory-limit", memory-limit),
    ).filter(((_, v)) => v != none)
    // tabular{l@{\extracolsep{1cm}}l}, set at olymp.sty's 6.5mm indent.
    pad(left: 6.5mm + tabcolsep, grid(columns: 2, column-gutter: 1cm, ..rows.map(((k, v)) => (word(k), v)).flatten()))
  }
  v(2mm)
  body
}

// ---- Cover ----------------------------------------------------------------------------

// \maketitle (title \LARGE, author and date \large) and the problem list floating at the
// bottom. polygon-chinese-template writes the cover in Chinese whatever the language.
#let cover(name, author, date) = {
  [#metadata(none) <xcpc-cover>]
  // \null, \vskip 2em, center's \topsep, then the title's own \vspace{4cm}.
  v(1.57 * sizes.normal + 2em + tex(9) + 4cm)
  align(center, {
    text(font: with-cjk(optical("Latin Modern Roman", sizes.LARGE), cjk-hei), size: sizes.LARGE, ..strut(sizes.LARGE))[#name \ 试题册]
    v(1.5em)
    text(..strut(sizes.large), kai(size: sizes.large, if author == none { "出题人" } else { author }))
    v(1em)
    text(..strut(sizes.large), serif(size: sizes.large, date))
  })
  place(bottom + center, float: true, context {
    let problems = query(<xcpc-problem>)
    set align(center)
    sans[试题列表]
    v(0.5em)
    text(..strut(sizes.small), serif(size: sizes.small, table(
      columns: (1cm + 2 * tabcolsep, 6cm + 2 * tabcolsep),
      align: center,
      inset: (x: tabcolsep, y: 0pt),
      stroke: (x, y) => (x: rule, top: if y == 0 { rule }, bottom: if y == problems.len() - 1 { rule }),
      ..problems.map(p => (p.value.label, p.value.title)).flatten(),
    )))
    v(2em)
    text(..strut(sizes.footnote), serif(size: sizes.footnote)[
      本试题册共 #problems.len() 题，#counter(page).final().first() 页。\
      如果您的试题册缺少页面，请立即通知志愿者。
    ])
    v(tex(9))                                 // the center environment's \topsep
  })
  pagebreak()
}

// ---- Editorial book ------------------------------------------------------------------
//
// The reference has no A4 editorial (its tutorials are Beamer slides), so this is our
// design, in the statements' book: the same page, fonts, sizes and running head and
// foot. Inside it, hierarchy is size, weight and space, never ornaments: no rules, no
// dots between things, no tinted boxes. Its measures are the design parameters above.

#let muted = luma(40%)                        // the meta line and comments: dark enough to print

// The title page: the cover's title, author and date, with "Editorial" for "Problem
// Set", and at the bottom the contents (label, title, page), as the cover has its
// problem list, set in the list's columns (1cm, 6cm) without its rules.
#let title-page(name, author, date) = {
  [#metadata(none) <xcpc-cover>]
  v(1.57 * sizes.normal + 2em + tex(9) + 4cm)
  align(center, {
    text(font: with-cjk(optical("Latin Modern Roman", sizes.LARGE), cjk-hei), size: sizes.LARGE, ..strut(sizes.LARGE))[#name \ #word("editorial")]
    v(1.5em)
    text(..strut(sizes.large), kai(size: sizes.large, if author == none { "出题人" } else { author }))
    v(1em)
    text(..strut(sizes.large), serif(size: sizes.large, date))
  })
  place(bottom + center, float: true, context {
    let rows = query(<xcpc-tutorial>).map(t => {
      let at = t.location()
      let go(body) = link(at, body)
      (
        go(bold-sans(sizes.small, t.value.label)),
        go(serif(size: sizes.small, t.value.title)),
        go(serif(size: sizes.small, text(number-width: "tabular", str(counter(page).at(at).first())))),
      )
    })
    text(..strut(sizes.small), grid(
      columns: (1cm, 6cm, 1cm),
      align: (left, left, right),
      ..rows.flatten(),
    ))
    v(2em + tex(9))
  })
  pagebreak()
}

// A problem's editorial, from a new page: the label and title as a problem's head,
// then, quieter, what is known about it (author, tags, sources), set apart by space.
// The head is also the PDF's bookmark.
#let tutorial(label: none, title: none, meta: (), body) = {
  pagebreak(weak: true)
  [#metadata((label: label, title: title)) <xcpc-tutorial>]
  place(hide(heading(level: 1, outlined: false, bookmarked: true, numbering: none)[#label #title]))
  let head = if label == none { title } else [#label#h(1em)#title]
  block(sticky: true, below: if meta.len() > 0 { 2 * smallskip } else { 2 * bigskip }, bold-sans(sizes.LARGE, head))
  if meta.len() > 0 {
    block(sticky: true, below: 2 * bigskip, text(fill: muted, sans(size: sizes.small, meta.join(h(1.5em)))))
  }
  body
}

// `##` in an editorial: one step below the statements' sections (the head is the top),
// \large, with a \bigskip before it.
#let subsection(title) = {
  v(bigskip, weak: true)
  block(sticky: true, below: smallskip, bold-sans(sizes.large, title))
}

// `###`: run in, as \paragraph is: bold, then the paragraph goes on after a quad.
#let runin(title) = { strong(title); h(1em) }

// What differs inside an editorial: code indented by the paragraph indent instead of
// framed, code a size smaller with bold keywords and italic comments
// (`mono.tmTheme`: prints in black and white), quotations as LaTeX's `quote`, tables
// without rules.
#let editorial-style(body) = {
  set raw(theme: "/xcpc/mono.tmTheme")
  show raw.where(block: true): it => {
    set text(font: code-font, size: sizes.small, ..strut(sizes.small))
    set par(justify: false, first-line-indent: 0pt)
    block(above: bigskip, below: bigskip, inset: (left: parindent), width: 100%, it)
  }
  // quote: \leftmargini (2.5em) on both sides, its first line not indented again.
  show quote.where(block: true): it => block(above: bigskip, below: bigskip, inset: (x: 2.5em), {
    set par(first-line-indent: 0pt)
    it.body
  })
  set table(stroke: none, inset: (x: tabcolsep, y: smallskip))
  show table.cell.where(y: 0): strong
  body
}

// ---- Contest --------------------------------------------------------------------------

#let contest(
  name: none,
  location: none,
  date: none,
  author: none,
  language: "chinese",
  show-cover: true,
  blank-pages: false,
  // "statements", or "editorial": a title page with the contents for the cover, and
  // the problem in hand on the right of the running head instead of the date.
  book: "statements",
  pdf-title: none,
  pdf-author: none,
  options: (:),
  body,
) = {
  let editorial = book == "editorial"
  let chinese = language == "chinese"
  let opt-body-size = options.at("body-size", default: "12pt")
  let eff-body-size = if type(opt-body-size) == str and opt-body-size.trim() != "" and opt-body-size.trim() != "12pt" {
    parse-len(opt-body-size, sizes.normal)
  } else {
    sizes.normal
  }
  let opt-margin-x = options.at("margin-x", default: "17.5mm")
  let custom-mx = type(opt-margin-x) == str and opt-margin-x.trim() != "" and opt-margin-x.trim() != "17.5mm"
  let eff-left = if custom-mx { parse-len(opt-margin-x, text-left) } else { text-left }
  let eff-right = if custom-mx { parse-len(opt-margin-x, 210mm - text-left - text-width) } else { 210mm - text-left - text-width }
  let eff-text-width = if custom-mx { 210mm - eff-left - eff-right } else { text-width }

  let opt-margin-y = options.at("margin-y", default: "26mm")
  let custom-my = type(opt-margin-y) == str and opt-margin-y.trim() != "" and opt-margin-y.trim() != "26mm"
  let eff-top = if custom-my { parse-len(opt-margin-y, text-top) } else { text-top }
  let eff-bottom = if custom-my { parse-len(opt-margin-y, 297mm - text-top - text-height) } else { 297mm - text-top - text-height }
  let eff-text-height = if custom-my { 297mm - eff-top - eff-bottom } else { text-height }

  let opt-song = options.at("cjk-body-font", default: "")
  let eff-song = if type(opt-song) == str and opt-song.trim() != "" { opt-song.trim() } else { cjk-song }
  let opt-hei = options.at("cjk-heading-font", default: "")
  let eff-hei = if type(opt-hei) == str and opt-hei.trim() != "" { opt-hei.trim() } else { cjk-hei }

  lang-state.update(language)
  blank-state.update(blank-pages)
  cjk-song-state.update(eff-song)
  cjk-hei-state.update(eff-hei)
  let doc-title = if pdf-title != none and pdf-title != "" { pdf-title } else { name }
  let doc-author = if pdf-author != none and pdf-author != "" { pdf-author } else if author != none and author != "" { author } else { () }
  set document(title: doc-title, author: doc-author)
  set text(lang: if chinese { "zh" } else { "en" }, region: if chinese { "cn" } else { none })

  set text(..strut(eff-body-size))
  // olymp.sty: no space between paragraphs in Chinese, \parskip = 1ex (0.43em in Latin Modern) otherwise.
  set par(leading: 0pt, spacing: if chinese { 0pt } else { 0.43em }, justify: true, first-line-indent: (amount: parindent, all: true))
  set text(font: with-cjk("Latin Modern Roman 12", eff-song), size: eff-body-size)
  show raw.where(block: false): set text(font: mono-font, size: eff-body-size)
  show raw.where(block: true): set text(font: code-font, size: eff-body-size, ligatures: false, features: (calt: 0))
  // ctex's founder fontset: CJK bold is FZ Hei, CJK italic FZ Kai.
  show strong: set text(font: with-cjk("Latin Modern Roman 12", eff-hei))
  show emph: set text(font: with-cjk("Latin Modern Roman 12", cjk-kai))
  // Highlighted raw emits strong/emph; keep monospace inside raw.
  show raw.where(block: false): it => {
    show strong: set text(font: with-cjk("Latin Modern Mono 12", eff-hei))
    show emph: set text(font: with-cjk("Latin Modern Mono 12", cjk-kai))
    it
  }
  show raw.where(block: true): it => {
    show strong: set text(font: with-cjk("JetBrains Mono", eff-hei))
    show emph: set text(font: with-cjk("JetBrains Mono", cjk-kai))
    it
  }
  show math.equation: set text(font: "Latin Modern Math")
  // olymp.sty's `chinese` option makes \le and \ge slanted. The replacement keeps the
  // relation's class: a bare text would lose its spacing after a closing `)` or `^9`.
  let slanted(s) = math.class("relation", s)
  show math.equation: it => if chinese { show "≤": slanted(sym.lt.eq.slant); show "≥": slanted(sym.gt.eq.slant); it } else { it }
  // itemize: \leftmargini 2.5em, \labelsep 0.5em, \topsep + \partopsep around.
  set list(marker: box(width: 2em, align(right)[•]), indent: 0pt, body-indent: 0.5em, spacing: 0pt)
  show list: set block(above: tex(12), below: tex(12))
  show list: set par(first-line-indent: 0pt)
  // Displays: TeX picks long (12pt) or short (0–6.5pt) skips from the length of the line
  // before; one medium skip looks right for both.
  show math.equation.where(block: true): set block(above: tex(6), below: tex(6))
  // Clean academic three-line table defaults for body Markdown tables (samples/cover pass explicit stroke & inset).
  set table(
    stroke: (_, y) => (
      top: if y == 0 { 0.9pt + black } else if y == 1 { 0.45pt + black } else { none },
      bottom: none,
    ),
    inset: (x: 10pt, y: 4.5pt),
  )

  let is-cover() = query(<xcpc-cover>).any(c => c.location().page() == here().page())
  set page(
    width: 210mm,
    height: 297mm,
    margin: (left: eff-left, right: eff-right, top: eff-top, bottom: eff-bottom),
    // Running head: name left, date right, its baseline \headsep above the text; the
    // rule hangs 2pt below the ink (\\[2pt] then \hrule).
    header-ascent: 6mm,
    header: context if not is-cover() {
      set par(first-line-indent: 0pt, spacing: 0pt)
      set block(spacing: 0pt)
      let head = if editorial {
        // The problem whose editorial this page is in: the last one started by here.
        let current = query(<xcpc-tutorial>).filter(t => t.location().page() <= here().page()).at(-1, default: none)
        serif(cjk: eff-song, if name == none or name == "" { word("editorial") } else { name })
        h(1fr)
        if current != none {
          let v = current.value
          serif(cjk: eff-song, if v.label == none { v.title } else [#v.label#h(1em)#v.title])
        }
      } else { serif(cjk: eff-song, name); h(1fr); serif(cjk: eff-song, date) }
      let depth = measure(text(top-edge: "baseline", bottom-edge: "bounds", head)).height
      block(text(top-edge: "bounds", bottom-edge: "baseline", head))
      block(height: 0pt, move(dy: depth + 2pt, line(length: 100%, stroke: rule)))
    },
    // Running foot: a rule, 6pt, then the ink of "Page X of Y" in sans.
    footer-descent: tex(30) - sizes.normal,
    footer: context if not is-cover() {
      set par(first-line-indent: 0pt, spacing: 0pt)
      line(length: 100%, stroke: rule)
      v(6pt)
      align(center, sans(text(top-edge: "bounds")[Page #counter(page).get().first() of #counter(page).final().first()]))
    },
  )

  show: if editorial { editorial-style } else { it => it }
  if show-cover {
    counter(page).update(0)
    if editorial { title-page(name, author, date) } else { cover(name, author, date) }
  }
  // Where the text may go, for the engine's overflow check.
  [#metadata((left: eff-left, top: eff-top, width: eff-text-width, height: eff-text-height)) <xcpc-text-area>]
  body
  // The last problem's \problemend: the book ends on an even page. The parity is the
  // end of the text's, not where the blank page itself would start.
  if blank-pages {
    [#metadata(none) <xcpc-end>]
    context if calc.odd(counter(page).at(query(<xcpc-end>).last().location()).first()) {
      pagebreak()
      blank-page(last: true)
    }
  }
}
