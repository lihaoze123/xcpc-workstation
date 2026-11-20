// olymp-classic: the look of polygon-chinese-template@10f9959 (olymp.sty + ctexart, 12pt).
//
// The goal is to look the same, not to reproduce TeX's line-by-line arithmetic: the
// values below are the design parameters of olymp.sty and ctexart (their sizes,
// skips, rules and widths), and Typst does the typesetting. See docs/statement-style.md §1.

// TeX points (1/72.27 in) to Typst points (1/72 in).
#let tex(x) = x * 72 / 72.27 * 1pt

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

// Latin Modern ships optical sizes; fontspec picks 10 below 11pt, 12 up to 15pt, 17 above.
#let optical(family, size) = family + if size < tex(11) { "" } else if size < tex(15) { " 12" } else { " 17" }
#let with-cjk(latin, cjk) = ((name: latin, covers: "latin-in-cjk"), cjk)

#let serif(size: sizes.normal, cjk: "FZShuSong-Z01", body) = text(font: with-cjk(optical("Latin Modern Roman", size), cjk), size: size, body)
#let kai(size: sizes.normal, body) = serif(size: size, cjk: "FZKai-Z03", body)
#let sans(size: sizes.normal, body) = text(font: with-cjk(optical("Latin Modern Sans", size), "FZHei-B01"), size: size, body)
// Bold sans exists only at the 10pt design size; CJK bold is FZ Hei.
#let bold-sans(size, body) = text(font: with-cjk("Latin Modern Sans", "FZHei-B01"), weight: "bold", size: size, body)
#let mono-font = with-cjk("Latin Modern Mono 12", "FZFangSong-Z02")
#let code-font = with-cjk("JetBrains Mono", "FZFangSong-Z02")
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
  let column = 0.43 * text-width + 2 * tabcolsep
  let body = pad(left: parindent, table(
    columns: (column, column),
    stroke: rule,
    inset: (x: tabcolsep, y: 0pt),
    table.header(head("stdin"), head("stdout"), repeat: true),
    ..pairs.map(((input, output)) => (cell(input), cell(output))).flatten(),
  ))
  section(if pairs.len() == 1 { "example" } else { "examples" })
  context block(breakable: measure(body, width: text-width).height > text-height, below: smallskip, body)
}

#let problem(
  title: none,
  label: none,
  input: none,
  output: none,
  time-limit: none,
  memory-limit: none,
  show-metadata: true,
  body,
) = {
  pagebreak(weak: true)
  blank-page-if-even()
  [#metadata((label: label, title: title)) <xcpc-problem>]
  block(sticky: true, below: 3mm, bold-sans(sizes.LARGE)[#word("problem") #label. #title])
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
    text(font: with-cjk(optical("Latin Modern Roman", sizes.LARGE), "FZHei-B01"), size: sizes.LARGE, ..strut(sizes.LARGE))[#name \ 试题册]
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
    text(font: with-cjk(optical("Latin Modern Roman", sizes.LARGE), "FZHei-B01"), size: sizes.LARGE, ..strut(sizes.LARGE))[#name \ #word("editorial")]
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
  body,
) = {
  let editorial = book == "editorial"
  let chinese = language == "chinese"
  lang-state.update(language)
  blank-state.update(blank-pages)
  set document(title: name)
  set text(lang: if chinese { "zh" } else { "en" }, region: if chinese { "cn" } else { none })

  set text(..strut(sizes.normal))
  // olymp.sty: no space between paragraphs in Chinese, \parskip = 1ex (0.43em in Latin Modern) otherwise.
  set par(leading: 0pt, spacing: if chinese { 0pt } else { 0.43em }, justify: true, first-line-indent: (amount: parindent, all: true))
  set text(font: with-cjk("Latin Modern Roman 12", "FZShuSong-Z01"), size: sizes.normal)
  show raw.where(block: false): set text(font: mono-font, size: sizes.normal)
  show raw.where(block: true): set text(font: code-font, size: sizes.normal)
  // ctex's founder fontset: CJK bold is FZ Hei, CJK italic FZ Kai.
  show strong: set text(font: with-cjk("Latin Modern Roman 12", "FZHei-B01"))
  show emph: set text(font: with-cjk("Latin Modern Roman 12", "FZKai-Z03"))
  // Highlighted raw emits strong/emph; keep monospace inside raw.
  show raw.where(block: false): it => {
    show strong: set text(font: with-cjk("Latin Modern Mono 12", "FZHei-B01"))
    show emph: set text(font: with-cjk("Latin Modern Mono 12", "FZKai-Z03"))
    it
  }
  show raw.where(block: true): it => {
    show strong: set text(font: with-cjk("JetBrains Mono", "FZHei-B01"))
    show emph: set text(font: with-cjk("JetBrains Mono", "FZKai-Z03"))
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

  let is-cover() = query(<xcpc-cover>).any(c => c.location().page() == here().page())
  set page(
    width: 210mm,
    height: 297mm,
    margin: (left: text-left, right: 210mm - text-left - text-width, top: text-top, bottom: 297mm - text-top - text-height),
    // Running head: name left, date right, its baseline \headsep above the text; the
    // rule hangs 2pt below the ink (\\[2pt] then \hrule).
    header-ascent: 6mm,
    header: context if not is-cover() {
      set par(first-line-indent: 0pt, spacing: 0pt)
      set block(spacing: 0pt)
      let head = if editorial {
        // The problem whose editorial this page is in: the last one started by here.
        let current = query(<xcpc-tutorial>).filter(t => t.location().page() <= here().page()).at(-1, default: none)
        serif(if name == none or name == "" { word("editorial") } else { name })
        h(1fr)
        if current != none {
          let v = current.value
          serif(if v.label == none { v.title } else [#v.label#h(1em)#v.title])
        }
      } else { serif(name); h(1fr); serif(date) }
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
  [#metadata((left: text-left, top: text-top, width: text-width, height: text-height)) <xcpc-text-area>]
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
