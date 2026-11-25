// trd-book：竖版 A4 中文讲义与复习册（衬线正文，适合数学推导、公式表与代码混排；灰度印刷）。
// 导出 `#let schema = (kind: "trd", ...)` 与 `#let render(doc)`，可自由编辑定制。
// 版式参数与出处见 docs/statement-style.md §5.1，全部以正文字号 s 的倍数表示。

#import "/xcpc/fonts.typ": cjk-hei, cjk-song

#let with-cjk(latin, cjk) = ((name: latin, covers: "latin-in-cjk"), cjk)

#let schema = (
  kind: "trd",
  name: (zh: "中文讲义与复习册", en: "TRD Study Book"),
  description: (
    zh: "竖版 A4 讲义与复习册，衬线正文，适合数学推导、公式卡片与代码混排",
    en: "Portrait A4 study guide with serif body text for derivations, formula cards, and code",
  ),
  groups: (
    (id: "layout", label: (zh: "页面与字体", en: "Page & Typography")),
    (id: "code", label: (zh: "代码与校验哈希", en: "Code & Verification Hash")),
  ),
  fields: (
    (
      key: "columns",
      label: (zh: "栏数", en: "Columns"),
      type: "enum",
      scope: "book",
      group: "layout",
      default: "1",
      choices: (
        (value: "1", label: (zh: "单栏（推荐）", en: "1 Column (Recommended)")),
        (value: "2", label: (zh: "双栏", en: "2 Columns")),
      ),
    ),
    (
      key: "font-size",
      label: (zh: "正文字号", en: "Font Size"),
      type: "length",
      scope: "book",
      group: "layout",
      default: "10pt",
      presets: ("9pt", "9.5pt", "10pt", "10.5pt", "11pt"),
      placeholder: "10pt",
    ),
    (
      key: "hash-interval",
      label: (zh: "哈希间隔（行）", en: "Hash Interval (Lines)"),
      description: (
        zh: "每隔 N 行及函数块末尾印出「行号 累计哈希」并附核对命令；0 表示只印总哈希",
        en: "Print “line hash” every N lines and at block ends, plus the check command; 0 = total hash only",
      ),
      type: "number",
      scope: "book",
      group: "code",
      default: 10,
      min: 0,
      max: 50,
      step: 1,
    ),
    (
      key: "strip-comments",
      label: (zh: "印刷时剥离代码注释", en: "Strip Code Comments"),
      type: "bool",
      scope: "book",
      group: "code",
      default: false,
    ),
  ),
)

// ---------------------------------------------------------------------------
// 设计参数（docs/statement-style.md §5.1）
// ---------------------------------------------------------------------------

#let ink = luma(0) // 正文、标题、总哈希
#let ink-2 = luma(90) // 元信息、依赖、页眉页脚
#let ink-3 = luma(150) // 行号、分块哈希、续行标记、目录引导点

#let page-margin = (top: 20mm, bottom: 18mm, x: 18mm)
#let col-gutter = 7mm
#let code-ratio = 0.86 // 代码字号 = 0.86 s
#let code-pitch = 1.3 // 代码行距 = 1.3 × 代码字号
#let text-pitch = 1.6 // 正文行距 = 1.6 s

#let serif = with-cjk("Latin Modern Roman 12", cjk-song)
#let sans = with-cjk("Latin Modern Sans 12", cjk-hei)
#let mono = with-cjk("JetBrains Mono", cjk-hei)

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

#let parse-cols(v, fallback) = {
  let n = if type(v) == int { v }
    else if type(v) == float { int(v) }
    else if type(v) == str {
      let t = v.trim()
      if t == "1" { 1 } else if t == "2" { 2 } else { fallback }
    } else { fallback }
  calc.clamp(n, 1, 2)
}

#let parse-int(v, fallback) = {
  if type(v) == int { v }
  else if type(v) == float { int(v) }
  else if type(v) == str and v.trim().match(regex("^[0-9]+$")) != none { int(v.trim()) }
  else { fallback }
}

// 单栏宽度：竖版 A4 去掉两侧页边距与栏间距后均分。
#let column-width(cols) = (210mm - 2 * page-margin.x - (cols - 1) * col-gutter) / cols

// 行首空白宽度（字符数，制表符按 4 计）。
#let indent-chars(s) = {
  let n = 0
  for c in s.clusters() {
    if c == " " { n += 1 } else if c == "\t" { n += 4 } else { break }
  }
  n
}

// 代码块：逐行渲染。打哈希的行右侧印「行号 哈希」，其余行占满整栏；
// 超出栏宽的行悬挂缩进到「原缩进 + 2 字符」，并在续行行首印 ↪。
#let render-code(code, lang, hash-blocks, size) = {
  let hmap = (:)
  // 累计哈希没有变化的标记（如块尾之后的空行）不重复印出；末行总哈希始终保留。
  let prev = none
  for b in hash-blocks {
    if b.hash != prev or b.at("is-final", default: false) {
      if b.at("is-final", default: false) and prev == b.hash {
        let keys = hmap.keys().filter(k => hmap.at(k).hash == b.hash)
        for k in keys { let _ = hmap.remove(k) }
      }
      hmap.insert(str(b.line), b)
    }
    prev = b.hash
  }
  let pitch = size * code-pitch
  let gap = pitch - size

  set raw(theme: "/xcpc/mono.tmTheme")
  show raw.where(block: true): set text(
    font: mono,
    size: size,
    ligatures: false,
    features: (calt: 0),
    top-edge: 0.8em,
    bottom-edge: -0.2em,
  )
  show raw.where(block: true): it => {
    show strong: set text(font: mono, weight: "bold")
    show emph: set text(font: mono, style: "normal")
    it
  }
  show linebreak: none
  show raw.line: it => context {
    let cw = measure(text(font: mono, size: size, "0")).width
    let entry = hmap.at(str(it.number), default: none)
    let gutter = if entry == none { none } else {
      let final = entry.at("is-final", default: false)
      text(size: size * 0.86, {
        text(fill: ink-3, str(it.number))
        h(0.5em)
        if final { text(fill: ink, weight: "bold", entry.hash) } else { text(fill: ink-3, entry.hash) }
      })
    }
    let lead = indent-chars(it.text)
    let body = {
      set par(hanging-indent: (lead + 2) * cw, leading: gap, justify: false)
      it.body
    }
    block(width: 100%, spacing: gap, layout(sz => {
      let gw = if gutter == none { 0pt } else { measure(gutter).width + 1.5 * cw }
      let tw = sz.width - gw
      // 与段落同处一个序列的 place 让 set par 生效；测量与排版共用同一个块。
      let mb = block(width: tw, { body; place(top + left, []) })
      let h = measure(mb).height
      let n = calc.max(1, int(calc.ceil(h / pitch - 0.05)))
      let main = block(width: tw, {
        mb
        for k in range(1, n) {
          place(top + left, dx: lead * cw, dy: k * pitch, text(fill: ink-3, "↪"))
        }
      })
      if gutter == none { main } else {
        grid(columns: (tw, gw), align: (left, right + bottom), main, gutter)
      }
    }))
  }
  block(width: 100%, above: 0.45em, below: 0pt, raw(code, lang: lang, block: true))
}

#let render(doc) = {
  let opts = doc.at("options", default: (:))
  let single = doc.at("single", default: false)
  let chinese = doc.at("language", default: "chinese") != "english"
  let cols = parse-cols(opts.at("columns", default: 1), 1)
  let s = parse-len(opts.at("font-size", default: "10pt"), 10pt)
  let code-size = s * code-ratio
  let hash-interval = parse-int(opts.at("hash-interval", default: 10), 10)

  let title-text = doc.at("name", default: "")
  let author-text = doc.at("author", default: none)
  let date-text = doc.at("date", default: none)
  let verify-cmd = doc.at(
    "verify-cmd",
    default: "sed -n '1,Np' a.cpp | cpp -dD -P -fpreprocessed | tr -d '[:space:]' | md5sum | cut -c-6",
  )

  // slug → 标题，用于依赖行（优先使用 doc.snippet-titles，再合并全书条目）。
  let titles = doc.at("snippet-titles", default: (:))
  for ch in doc.at("chapters", default: ()) {
    for item in ch.at("items", default: ()) { titles.insert(item.slug, item.title) }
  }

  set document(
    title: if doc.at("pdf-title", default: none) != none { doc.pdf-title } else { title-text },
    author: if doc.at("pdf-author", default: none) != none { (doc.pdf-author,) } else { () },
  )

  // 单条预览：页宽等于所在册的一栏，高度随内容；整本：竖版 A4。
  set page(
    width: if single { column-width(cols) + 2 * 4mm } else { 210mm },
    height: if single { auto } else { 297mm },
    margin: if single { 4mm } else { page-margin },
    header: if single { none } else {
      context {
        // 首页由书名区承担，不印页眉；其余页左侧书名、右侧当前章名。
        if here().page() > 1 {
          set text(font: sans, size: 0.85 * s, fill: ink-2)
          let pg = here().page()
          let hs = query(heading.where(level: 1))
          let on = hs.filter(h => h.location().page() == pg)
          let before = hs.filter(h => h.location().page() < pg)
          let cur = if on.len() > 0 { on.map(h => h.body).join(" / ") } else if before.len() > 0 { before.last().body } else { [] }
          grid(columns: (1fr, auto), align: (left, right), title-text, cur)
        }
      }
    },
    footer: if single { none } else {
      context {
        set text(font: sans, size: 0.85 * s, fill: ink-2)
        align(center, counter(page).display("1 / 1", both: true))
      }
    },
  )

  set text(font: serif, size: s, lang: if chinese { "zh" } else { "en" }, fill: ink)
  show strong: set text(font: with-cjk("Latin Modern Roman 12", cjk-hei))
  show math.equation: set text(font: "Latin Modern Math")
  show raw.where(block: false): set text(font: mono, size: code-size)
  set par(leading: (text-pitch - 1) * s, spacing: 0.8 * s, justify: true)
  set list(indent: 0pt, body-indent: 0.6em, spacing: (text-pitch - 1) * s)
  set enum(indent: 0pt, body-indent: 0.6em, spacing: (text-pitch - 1) * s)
  set table(stroke: 0.4pt + ink-3, inset: (x: 0.5em, y: 0.3em))
  show table.cell.where(y: 0): set text(font: sans, weight: "bold")

  // 章：通栏黑细线 + 灰色两位章号 + 粗体章名。
  show heading.where(level: 1): it => block(
    width: 100%,
    above: 1.6 * s * text-pitch,
    below: 0.8 * s * text-pitch,
    sticky: true,
    {
      line(length: 100%, stroke: 1pt + ink)
      v(0.35 * s)
      text(font: sans, size: 1.45 * s, weight: "bold", {
        text(fill: ink-3, context numbering("01", query(heading.where(level: 1).before(here())).len()))
        h(0.6em)
        it.body
      })
    },
  )
  // 条目标题：粗体无衬线。
  show heading.where(level: 2): it => text(font: sans, weight: "bold", size: 1.15 * s, it.body)

  let front = if single { none } else {
    // 书名区：放在首页上部，紧接着目录与正文，不单独占页。
    block(below: 1.2 * s * text-pitch, {
      text(font: sans, weight: "bold", size: 2 * s, title-text)
      let parts = (author-text, date-text).filter(x => x != none and x != "")
      if parts.len() > 0 {
        v(0.4 * s)
        text(font: sans, fill: ink-2, size: s, parts.join(h(1.5em)))
      }
    })
    block(below: 1.0 * s * text-pitch, {
      set text(size: 0.95 * s)
      set outline.entry(fill: box(width: 1fr, repeat(text(fill: ink-3, "."), gap: 0.2em)))
      show outline.entry.where(level: 1): it => {
        v(0.5 * s, weak: true)
        text(font: sans, weight: "bold", it)
      }
      outline(title: none, depth: 2, indent: 1.5em)
    })
    if hash-interval > 0 {
      block(below: 1.0 * s * text-pitch, {
        set par(hanging-indent: 2em, justify: false)
        text(font: sans, weight: "bold", if chinese { "核对" } else { "Check" })
        h(0.6em)
        text(font: mono, size: code-size, verify-cmd)
        linebreak()
        text(fill: ink-2, size: 0.9 * s, if chinese {
          [N 为行号；代码右侧「行号 哈希」即前 N 行的累计哈希，敲错时可按行二分定位。]
        } else {
          [N is the line number; “line hash” beside the code is the cumulative hash of lines 1..N.]
        })
      })
    }
  }

  let item-block(item) = {
    [#metadata((label: item.slug, title: item.title)) <xcpc-trd-item>]
    let deps = item.at("deps", default: ())
    let tc = item.at("time-complexity", default: none)
    let sc = item.at("space-complexity", default: none)
    let cx = (tc, sc).filter(x => x != none and x != "")

    block(width: 100%, above: if single { 0pt } else { 1.2 * s * text-pitch }, below: 0.4 * s, sticky: true, {
      let title = heading(level: 2, item.title)
      if cx.len() == 0 { title } else {
        let cx-text = text(font: mono, size: code-size, fill: ink-2, cx.join(" · "))
        layout(sz => {
          // 标题与复杂度放得下就同一行，否则复杂度换到下一行右对齐。
          if measure(title).width + measure(cx-text).width + s <= sz.width {
            grid(columns: (1fr, auto), align: (left + bottom, right + bottom), title, cx-text)
          } else {
            title
            align(right, cx-text)
          }
        })
      }
      if deps.len() > 0 {
        v(0.2 * s)
        text(font: sans, size: 0.9 * s, fill: ink-2, {
          "→ "
          deps
            .map(d => {
              let t = titles.at(d, default: d)
              if single { t } else {
                t
                context {
                  let m = query(<xcpc-trd-item>).find(x => x.value.label == d)
                  if m != none { if chinese { [ · 第 #m.location().page() 页] } else { [ · p. #m.location().page()] } }
                }
              }
            })
            .join(if chinese { "、" } else { ", " })
        })
      }
    })

    if item.at("notes", default: none) != none {
      block(width: 100%, above: 0.4 * s, below: 0pt, sticky: item.at("has-code", default: false), item.notes)
    }

    let code = item.at("code", default: "")
    if item.at("has-code", default: false) and code != "" {
      render-code(code, item.at("code-lang", default: "cpp"), item.at("hash-blocks", default: ()), code-size)
    }
  }

  let body = {
    for ch in doc.at("chapters", default: ()) {
      if not single and ch.title != "" { heading(level: 1, ch.title) }
      for item in ch.at("items", default: ()) { item-block(item) }
    }
  }

  front
  if cols > 1 and not single { columns(cols, gutter: col-gutter, body) } else { body }
}
