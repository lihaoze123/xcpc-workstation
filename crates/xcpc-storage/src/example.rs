//! Built-in example workspace embedded in the binary for `xcpc init --example` and
//! `Preview.initExampleWorkspace` / `Preview.pickAndInitExampleWorkspace`.
//!
//! Contains three curated problems:
//! - `crc-check`: CRC polynomial division problem with algorithmic Typst figures
//!   (`figures.typ`), `@preview/mannot`, and `@preview/pinit` field annotations.
//! - `tree-beacon`: Graph/tree DP problem with a publication-grade `@preview/cetz`
//!   rooted-tree diagram (heavy-chain highlights & subtree shading) and Pinit editorial.
//! - `typst-showcase`: Concentrated showcase of CeTZ vector graphics, Pinit formula
//!   annotations, programmatic Typst scripting, and `templates/preamble.typ` macros.
//!
//! Two contests (`welcome-round` in `olymp-classic`, `oi-mock` in `cnoi-classic`)
//! live-reference the same problem bank to demonstrate multi-template publishing.

use std::fs;
use std::path::Path;

use xcpc_domain::Language;

use crate::example_en::EXAMPLE_FILES_EN;
use crate::example_packages::SHARED_PACKAGE_FILES;
use crate::example_snippets::{
    EXAMPLE_SNIPPET_FILES_EN, EXAMPLE_SNIPPET_FILES_ZH, SHARED_SNIPPET_CODE_FILES,
};
use crate::{Result, Workspace, init_workspace_with_template};

/// Static files seeded into an example workspace (relative to workspace root).
const EXAMPLE_FILES: &[(&str, &str)] = &[
    (
        "templates/preamble.typ",
        r##"// 工作区级 Typst 自定义宏（自动注入所有题面与题解编译作用域）
// 在题面或题解中使用 ```typst 代码块即可直接调用这里定义的宏。

#let insight(title: "关键观察", body) = pad(
  left: 1pt,
  block(
    breakable: false,
    width: 100%,
    inset: (left: 10pt, right: 10pt, y: 7pt),
    stroke: (left: 2pt + rgb("#0B6989")),
    fill: rgb("#F4F8FA"),
    radius: (right: 3pt),
    {
      set par(first-line-indent: 0pt)
      text(weight: "bold", fill: rgb("#0B6989"), size: 9.5pt, title)
      v(3pt, weak: true)
      body
    },
  ),
)

#let complexity(time, space) = block(
  inset: (x: 8pt, y: 5pt),
  stroke: 0.5pt + rgb("#D4D4D8"),
  radius: 3pt,
  fill: rgb("#FAFAFA"),
  {
    set par(first-line-indent: 0pt)
    text(size: 9.5pt)[
      #text(weight: "bold")[复杂度总结]：时间复杂度 #time，空间复杂度 #space。
    ]
  },
)
"##,
    ),
    // =========================================================================
    // Problem A: crc-check (循环冗余校验)
    // =========================================================================
    (
        "problems/crc-check/problem.toml",
        r#"title = "循环冗余校验"
rating = 1400
tags = ["implementation", "bitmask", "crc", "typst"]
group = "出版精选"

[limits]
time = "1s"
memory = "256MB"
"#,
    ),
    (
        "problems/crc-check/figures.typ",
        r##"// CRC 题面专用 Typst 算法可视化模块：
// 直接在编译期运行模 2 除法，动态生成传输对比图、异或竖式与长除法竖式。
#import "@preview/pinit:0.2.2": absolute-place, pin, pinit

#let accent = rgb("#0B6989")
#let color-g = rgb("#D6E6F7")
#let color-q = rgb("#FDEEB9")
#let color-r = rgb("#CDEED6")
#let color-pad = luma(228)
#let color-error = rgb("#F6C4C0")
#let bit-size = 5mm

#let xor-bit(a, b) = if a == b { "0" } else { "1" }

/// 计算模 2 除法的商、余数与每一步中间状态。
#let divide(dividend, g) = {
  let r = g.len() - 1
  let n = dividend.len() - r
  let x = dividend.clusters()
  let divisor = g.clusters()
  let quotient = ()
  let rows = ((kind: "x", bits: x, done: 0),)
  for i in range(n) {
    quotient.push(x.at(i))
    if x.at(i) == "1" {
      rows.push((kind: "g", at: i))
      for j in range(r + 1) {
        x.at(i + j) = xor-bit(x.at(i + j), divisor.at(j))
      }
      rows.push((kind: "x", bits: x, done: i + 1))
    }
  }
  (quotient: quotient, remainder: x.slice(n).join(), rows: rows)
}

#let digit(bit, fill: none, color: black) = grid.cell(
  fill: fill,
  text(fill: color, raw(bit)),
)

#let caption(body) = text(size: 0.85em, body)

/// 纯原生 Typst 竖式下拉箭头，跨越 `span` 行。
#let pull-down(span) = grid.cell(
  rowspan: span,
  align: center + horizon,
  {
    let h = bit-size * span - 1.6mm
    box(width: 3mm, height: h, {
      place(top + center, dy: 0.4mm, line(
        angle: 90deg,
        length: h - 1.5mm,
        stroke: (paint: luma(145), thickness: 0.55pt, dash: "densely-dotted"),
      ))
      place(bottom + center, dy: -0.2mm, polygon(
        fill: luma(145),
        (0pt, 0pt),
        (2.8pt, 0pt),
        (1.4pt, 2.8pt),
      ))
    })
  },
)

/// 同一帧的两次传输对比：无错传输 vs 翻转一位后的出错传输。
#let flow(m, g, flipped) = {
  let r = g.len() - 1
  let frame = m + divide(m + "0" * r, g).remainder
  let wrong = frame
    .clusters()
    .enumerate()
    .map(((i, bit)) => if i == flipped { xor-bit(bit, "1") } else { bit })
    .join()
  let frame-fill(i) = if i >= m.len() { color-r }
  let wrong-fill(i) = if i == flipped { color-error } else { frame-fill(i) }
  let bits(s, fill: _ => none) = s
    .clusters()
    .enumerate()
    .map(((i, bit)) => digit(bit, fill: fill(i)))
  let head(span, body) = grid.cell(colspan: span, caption(body))
  let to = text(fill: luma(50%), $arrow.r$)
  let verdict(body) = grid.cell(
    align: left + horizon,
    inset: (left: 1em),
    caption(body),
  )
  let row(sent, received, fill, conclusion) = (
    ..bits(sent, fill: frame-fill),
    to,
    ..bits(received, fill: fill),
    to,
    ..bits(divide(received, g).remainder),
    verdict(conclusion),
  )
  let columns = frame.len() * 2 + r + 3

  block(breakable: false, width: 100%, inset: (y: 4pt), {
    set par(first-line-indent: 0pt)
    align(center, grid(
      columns: (bit-size,) * frame.len()
        + (7mm,)
        + (bit-size,) * frame.len()
        + (7mm,)
        + (bit-size,) * r
        + (auto,),
      rows: (6.5mm, 6mm, 6mm, 6mm),
      align: center + horizon,
      grid.hline(y: 0, stroke: 0.9pt),
      grid.hline(y: 1, stroke: 0.4pt),
      grid.hline(y: 3, stroke: 0.9pt),
      head(frame.len(), [发送方]),
      [],
      head(frame.len(), [传输后]),
      [],
      head(r + 1, [接收方得到的余数]),
      ..row(frame, frame, frame-fill, [未发现错误]),
      ..row(frame, wrong, wrong-fill, [发现错误]),
      grid.cell(colspan: columns, caption[
        绿色是校验码，红色是传错的第 #(flipped + 1) 位
      ]),
    ))
  })
}

/// 异或真值规则与按位异或示例。
#let xor-figure = {
  let rule(a, b) = (
    digit(a),
    $xor$,
    digit(b),
    $=$,
    digit(xor-bit(a, b), fill: color-q),
  )
  let rules = grid(
    columns: (bit-size,) * 5 + (6mm,) + (bit-size,) * 5,
    rows: bit-size,
    row-gutter: 1.5mm,
    align: center + horizon,
    ..rule("0", "0"),
    [],
    ..rule("0", "1"),
    ..rule("1", "0"),
    [],
    ..rule("1", "1"),
  )

  let top = "1100".clusters()
  let bottom = "1010".clusters()
  let example = grid(
    columns: (bit-size,) * 5,
    rows: bit-size,
    align: center + horizon,
    grid.hline(y: 2, start: 1, stroke: 0.6pt),
    [],
    ..top.map(digit),
    $xor$,
    ..bottom.map(digit),
    [],
    ..range(4).map(i => digit(
      xor-bit(top.at(i), bottom.at(i)),
      fill: color-q,
    )),
  )

  block(breakable: false, width: 100%, inset: (y: 4pt), {
    set par(first-line-indent: 0pt)
    align(center, grid(
      columns: (auto, auto),
      column-gutter: 14mm,
      align: horizon,
      rules, example,
    ))
  })
}

/// 由 `divide()` 实时驱动的彩色模 2 长除法竖式。
#let ladder(m, g) = {
  let n = m.len()
  let r = g.len() - 1
  let result = divide(m + "0" * r, g)
  let divisor = g.clusters()
  let dividend = result.rows.first().bits
  let steps = result.rows.filter(row => row.kind == "g").map(row => row.at)
  let works = result.rows.filter(row => row.kind == "x").slice(1)
  let offset = r + 1
  let notes = offset + n + r
  let row-count = 2 + 2 * steps.len()

  let next(k) = if k + 1 < steps.len() { steps.at(k + 1) }
  let last-shown(k) = if next(k) == none { n + r - 1 } else { next(k) + r }
  let note(body) = grid.cell(
    align: left + horizon,
    inset: (left: 1em),
    caption(body),
  )
  let skipped(row, stop) = range(row.done, n)
    .filter(j => (stop == none or j < stop) and row.bits.at(j) == "0")
    .map(j => [第 #(j + 1) 位是 `0`：跳过])

  let cells = (:)
  let covered = ()
  let put(cells, y, x, cell) = {
    cells.insert(str(y) + "," + str(x), cell)
    cells
  }

  for (i, bit) in result.quotient.enumerate() {
    cells = put(cells, 0, offset + i, digit(bit, fill: color-q))
  }
  cells = put(cells, 0, notes, note[黄色是商 $Q$])

  for (j, bit) in divisor.enumerate() {
    cells = put(cells, 1, j, digit(bit, fill: color-g))
  }
  for (j, bit) in dividend.enumerate() {
    let fill = if j >= n { color-pad }
    cells = put(cells, 1, offset + j, digit(bit, fill: fill))
  }
  let first = steps.at(0, default: none)
  let intro = ([灰色是补的 #r 个 `0`],)
  intro += skipped(result.rows.first(), first)
  cells = put(cells, 1, notes, note(intro.join[；]))

  let lines = (
    grid.hline(y: 1, start: offset, end: notes, stroke: 0.7pt),
    grid.vline(x: offset, start: 1, end: 2, stroke: 0.7pt),
  )

  for (k, i) in steps.enumerate() {
    let y = 2 + 2 * k
    cells = put(cells, y, offset + i - 1, grid.cell($xor$))
    for (j, bit) in divisor.enumerate() {
      cells = put(cells, y, offset + i + j, digit(bit, fill: color-g))
    }
    cells = put(cells, y, notes, note[第 #(i + 1) 位是 `1`：对齐 $G$，做异或])
    lines.push(grid.hline(
      y: y + 1,
      start: offset + i,
      end: offset + i + r + 1,
      stroke: 0.6pt,
    ))

    let stop = next(k)
    let done = k == steps.len() - 1
    for j in range(i, last-shown(k) + 1) {
      let bit = works.at(k).bits.at(j)
      let cell = if done and j >= n {
        digit(bit, fill: color-r)
      } else if j < (if stop == none { n } else { stop }) {
        digit(bit, color: luma(165))
      } else {
        digit(bit)
      }
      cells = put(cells, y + 1, offset + j, cell)
    }
    let remarks = skipped(works.at(k), stop)
    if done { remarks.push[绿色是余数 $R$] }
    if remarks.len() > 0 {
      cells = put(cells, y + 1, notes, note(remarks.join[；]))
    }
  }

  if first != none {
    for j in range(first + r + 1, n + r) {
      let k = range(steps.len()).find(k => last-shown(k) >= j)
      let span = 2 * k + 1
      cells = put(cells, 2, offset + j, pull-down(span))
      covered += range(3, 2 + span).map(y => str(y) + "," + str(offset + j))
    }
  }

  let body = ()
  for y in range(row-count) {
    for x in range(notes + 1) {
      let key = str(y) + "," + str(x)
      if key in cells {
        body.push(cells.at(key))
      } else if key not in covered {
        body.push([])
      }
    }
  }

  block(breakable: false, width: 100%, inset: (y: 4pt), {
    set par(first-line-indent: 0pt)
    align(center, grid(
      columns: (bit-size,) * notes + (auto,),
      rows: bit-size,
      align: center + horizon,
      ..lines,
      ..body,
    ))
  })
}

/// 用 pinit 给字面输入/输出行的每个字段添加下划线与居中短标注。
#let fields(id, prefix, ..parts) = {
  let names = parts.pos().enumerate().map(((i, _)) => id + "-" + str(i))
  let text-line = parts
    .pos()
    .enumerate()
    .map(((i, part)) => [
      #pin(names.at(i) + "-a")#raw(part.at(0))#pin(names.at(i) + "-b")
    ])
    .join(h(1.4em))
  let marks = parts
    .pos()
    .enumerate()
    .map(((i, part)) => pinit(
      names.at(i) + "-a",
      names.at(i) + "-b",
      callback: (a, b) => {
        absolute-place(dx: a.x, dy: a.y + 3.5pt, line(
          length: b.x - a.x,
          stroke: 1pt + accent,
        ))
        absolute-place(
          dx: (a.x + b.x) / 2 - 1.5cm,
          dy: a.y + 5.5pt,
          box(width: 3cm, align(center, text(
            size: 0.85em,
            fill: accent,
            part.at(1),
          ))),
        )
      },
    ))
    .join()
  block(breakable: false, width: 100%, inset: (top: 4pt, bottom: 16pt), {
    set par(first-line-indent: 0pt)
    align(center)[#prefix#h(0.8em)#text-line]
    marks
  })
}
"##,
    ),
    (
        "problems/crc-check/statement.md",
        r##"数据在传输过程中可能受到干扰，导致某些位出错。循环冗余校验（Cyclic Redundancy Check，CRC）是一种常用的检错方法，以太网和 ZIP 压缩文件都使用它。

发送方在信息后面接一段校验码，再一起发出。接收方对收到的内容再做一次除法：余数全为 `0` 时未发现错误，否则说明传输出错：

```typst
#import "/problems/crc-check/figures.typ": flow
#flow("101001", "1101", 3)
```

现在由你担任发送方。给定两个 01 串：信息串 $M$ 和生成串 $G$。请在 $M$ 末尾添加 $r$ 个 `0` 得到 $X$，再用模 2 除法计算 $X$ 除以 $G$ 的商 $Q$ 和余数 $R$。余数 $R$ 就是校验码，它的位数 $r$ 为：

```typst
#import "@preview/mannot:0.4.0": annot, markul
#block(breakable: false, width: 100%, inset: (y: 4pt), {
  set par(first-line-indent: 0pt)
  $ r = markul(|G| - 1, #<len>) #annot(<len>, pos: right, dx: 1.8em, annot-text-props: (size: 0.85em))[#h(0.35em)生成串的长度减 1] $
})
```

**01 串**：只含 `0` 和 `1` 的字符串称为 01 串，$|S|$ 表示字符串 $S$ 的长度。

**异或**：两位相同得 `0`，不同得 `1`。两个等长的 01 串异或，就是逐位异或：

```typst
#import "/problems/crc-check/figures.typ": xor-figure
#xor-figure
```

**模 2 除法**步骤如下：

1. 从左到右依次看 $X$ 的前 $|M|$ 位。如果当前位是 `1`，商写 `1`，并把 $G$ 的第一位对齐当前位，用 $G$ 与 $X$ 中对齐的 $|G|$ 位逐位异或，结果写回 $X$；如果当前位是 `0`，商写 `0`，$X$ 不变。
2. 写下的 $|M|$ 个商位连起来是商 $Q$，此时 $X$ 的最后 $r$ 位是余数 $R$。

当 $M =$ `101001`、$G =$ `1101` 时，$r = 3$，除法竖式如下：

```typst
#import "/problems/crc-check/figures.typ": ladder
#ladder("101001", "1101")
```

## 输入格式

第一行包含一个整数 $T$（$1 \le T \le 1000$），表示数据组数。

接下来 $T$ 行，每行包含用一个空格隔开的 01 串 $M$ 和 $G$（$|M| \ge 1$，$|G| \ge 2$）。$G$ 的第一个字符是 `1`，$M$ 可以以 `0` 开头或全为 `0`。所有 $|M|$ 之和与所有 $|G|$ 之和都不超过 $5000$。

```typst
#import "/problems/crc-check/figures.typ": fields
#fields("input", [例如样例第一组：], ("101001", $M$), ("1101", $G$))
```

## 输出格式

每组数据输出一行，包含用一个空格隔开的 $Q$ 和 $R$。

$Q$ 去掉前导 `0`（全为 `0` 时输出一个 `0`）；$R$ 恰好输出 $r$ 位，保留前导 `0`。

```typst
#import "/problems/crc-check/figures.typ": fields
#fields("output", [对应的输出：], ("110101", $Q$), ("001", $R$))
```

## 数据范围

对于所有测试数据，保证 $1 \le T \le 1000$，$\sum |M| \le 5000$，$\sum |G| \le 5000$。

| 测试点 | $\sum \lvert M \rvert, \sum \lvert G \rvert \le$ | 特殊性质 |
| :---: | :---: | :---: |
| $1 \sim 4$ | $50$ | $T = 1$ |
| $5 \sim 7$ | $1000$ | $M$ 不含前导 `0` |
| $8 \sim 10$ | $5000$ | 无 |
"##,
    ),
    (
        "problems/crc-check/samples/01.in",
        "2\n101001 1101\n0011 101\n",
    ),
    ("problems/crc-check/samples/01.ans", "110101 001\n11 10\n"),
    (
        "problems/crc-check/editorial.md",
        r##"直接按照模 2 除法的定义逐位模拟即可。

```typst
#insight(title: "边界细节")[
  1. 商 $Q$ 需要去掉前导 `0`，若所有位均为 `0` 则输出单个 `0`。
  2. 余数 $R$ 必须恰好保留 $r = |G| - 1$ 位（包括前导 `0`），不能截断。
]
```

## 参考代码

```cpp
#include <iostream>
#include <string>
int main() {
    std::ios::sync_with_stdio(false); std::cin.tie(nullptr);
    int T;
    if (!(std::cin >> T)) return 0;
    while (T--) {
        std::string m, g;
        std::cin >> m >> g;
        int n = m.size(), r = g.size() - 1;
        std::string x = m + std::string(r, '0'), q(n, '0');
        for (int i = 0; i < n; ++i) {
            if (x[i] == '1') {
                q[i] = '1';
                for (int j = 0; j <= r; ++j)
                    x[i + j] = (x[i + j] == g[j]) ? '0' : '1';
            }
        }
        auto p = q.find('1');
        auto ans = (p == q.npos) ? "0" : q.substr(p);
        std::cout << ans << ' ' << x.substr(n) << '\n';
    }
    return 0;
}
```

```typst
#complexity($cal(O)(sum |M| dot |G|)$, $cal(O)(|M| + |G|)$)
```
"##,
    ),
    // =========================================================================
    // Problem B: tree-beacon (树上信标 — 精美图论题)
    // =========================================================================
    (
        "problems/tree-beacon/problem.toml",
        r#"title = "树上信标"
rating = 1900
tags = ["trees", "dp", "heavy-light", "graph"]
group = "出版精选"

[limits]
time = "1s"
memory = "512MB"
"#,
    ),
    (
        "problems/tree-beacon/statement.md",
        r##"给定一棵包含 $n$ 个节点的有根树，节点编号为 $1, 2, \ldots, n$，根节点为 $1$。第 $u$ 个节点上有一座能量信标，其初始能量值为 $w_u$。

你需要点亮若干座信标，使被点亮的信标集合 $S$ 满足**互不相邻约束**：若节点 $u \in S$，则其父节点 $p_u \notin S$。在此基础上，你可以额外选定一条从根 $1$ 出发的向下路径作为**重链主干**；重链主干上的边称为重边，其余边称为轻边。每条边连接的两个端点依然不能同时被点亮。

```typst
#import "@preview/cetz:0.5.3"

#align(center, block(breakable: false, inset: (y: 4pt), {
  set text(size: 9pt)
  cetz.canvas(length: 0.9cm, {
    import cetz.draw: *

    // 子树 T_2 柔和背景区域（留足内边距避免虚线穿过点权标签）
    merge-path(
      fill: rgb("#0B6989").transparentize(92%),
      stroke: (paint: rgb("#0B6989").transparentize(55%), thickness: 0.7pt, dash: "dashed"),
      close: true,
      {
        bezier((2.0, 3.35), (-0.65, -0.25), (-0.05, 3.0), (-0.75, 1.1))
        bezier((-0.65, -0.25), (4.05, -0.55), (0.35, -1.05), (3.15, -1.05))
        bezier((4.05, -0.55), (2.0, 3.35), (4.3, 0.9), (3.55, 2.9))
      },
    )
    content((0.0, 2.65), text(size: 8pt, fill: rgb("#0B6989"), weight: "bold")[子树 $T_2$])

    let pos = (
      "1": (4.3, 3.65),
      "2": (2.1, 2.2),
      "3": (6.3, 2.2),
      "4": (0.75, 0.8),
      "5": (3.0, 0.8),
      "6": (5.4, 0.8),
      "7": (7.2, 0.8),
      "8": (3.0, -0.45),
    )
    let weights = (
      "1": "4", "2": "6", "3": "5", "4": "3",
      "5": "7", "6": "2", "7": "8", "8": "4",
    )
    let heavy = (("1", "2"), ("2", "5"), ("5", "8"))
    let light = (("1", "3"), ("2", "4"), ("3", "6"), ("3", "7"))
    let chosen = ("2", "7", "8")

    // 轻边（虚线）
    for (u, v) in light {
      line(pos.at(u), pos.at(v), stroke: (paint: rgb("#94A3B8"), thickness: 0.9pt, dash: "densely-dashed"))
    }
    // 重边（加粗青蓝）
    for (u, v) in heavy {
      line(pos.at(u), pos.at(v), stroke: (paint: rgb("#0B6989"), thickness: 1.8pt))
    }

    // 节点与点权标注（全部位于安全空白处，零重叠零压线）
    let w-spec = (
      "1": ((0.48, 0.0), "west"),
      "2": ((-0.08, 0.45), "south"),
      "3": ((0.48, 0.0), "west"),
      "4": ((0.0, -0.46), "north"),
      "5": ((-0.48, 0.0), "east"),
      "6": ((-0.25, 0.40), "south-east"),
      "7": ((0.48, 0.0), "west"),
      "8": ((-0.48, 0.0), "east"),
    )
    for (id, p) in pos {
      let lit = id in chosen
      circle(
        p,
        radius: 0.34,
        fill: if lit { rgb("#0B6989") } else { white },
        stroke: if lit { 1.3pt + rgb("#09546D") } else { 0.9pt + rgb("#64748B") },
      )
      content(
        p,
        text(size: 9pt, weight: "bold", fill: if lit { white } else { rgb("#1E293B") }, id),
      )
      let (off, anc) = w-spec.at(id)
      content(
        (p.at(0) + off.at(0), p.at(1) + off.at(1)),
        anchor: anc,
        text(size: 7.8pt, fill: rgb("#475569"))[$w=#weights.at(id)$],
      )
    }

    // 右下紧凑图例（统一左对齐，与上方节点留足垂直间距）
    let lx = 5.45
    let ly = -0.35
    line((lx, ly), (lx + 0.55, ly), stroke: 1.8pt + rgb("#0B6989"))
    content((lx + 0.72, ly), anchor: "west", text(size: 7.8pt, fill: rgb("#334155"))[重链路径])
    line((lx, ly - 0.42), (lx + 0.55, ly - 0.42), stroke: (paint: rgb("#94A3B8"), thickness: 0.9pt, dash: "densely-dashed"))
    content((lx + 0.72, ly - 0.42), anchor: "west", text(size: 7.8pt, fill: rgb("#334155"))[普通轻边])
    circle((lx + 0.28, ly - 0.84), radius: 0.14, fill: rgb("#0B6989"), stroke: none)
    content((lx + 0.72, ly - 0.84), anchor: "west", text(size: 7.8pt, fill: rgb("#334155"))[点亮信标])
  })
}))
```

请你求出在满足**互不相邻约束**的前提下，所有被点亮信标的能量之和的最大值。

## 输入格式

第一行包含一个正整数 $n$（$1 \le n \le 2 \times 10^5$），表示树的节点数。

第二行包含 $n$ 个整数 $w_1, w_2, \ldots, w_n$（$0 \le w_u \le 10^9$），表示每个节点的能量值。

接下来 $n - 1$ 行，每行包含两个正整数 $u, v$（$1 \le u, v \le n$），表示树上的一条无向边。

## 输出格式

输出一行一个整数，表示点亮信标的最大能量总和。

## 样例解释

样例 1 对应上图：点亮节点 $\{1, 4, 5, 6, 7\}$（总能量 $4 + 3 + 7 + 2 + 8 = 24$）或 $\{2, 6, 7, 8\}$（总能量 $6 + 2 + 8 + 4 = 20$），最大值为 $24$。

## 数据范围

对于所有测试数据，保证 $1 \le n \le 2 \times 10^5$，$0 \le w_u \le 10^9$。

| 测试点 | $n \le$ | 特殊性质 |
| :---: | :---: | :---: |
| $1 \sim 3$ | $20$ | 无 |
| $4 \sim 5$ | $2 \times 10^5$ | 树是一条链 |
| $6 \sim 7$ | $2 \times 10^5$ | 树是菊花图 |
| $8 \sim 10$ | $2 \times 10^5$ | 无 |
"##,
    ),
    (
        "problems/tree-beacon/samples/01.in",
        "8\n4 6 5 3 7 2 8 4\n1 2\n1 3\n2 4\n2 5\n3 6\n3 7\n5 8\n",
    ),
    ("problems/tree-beacon/samples/01.ans", "24\n"),
    (
        "problems/tree-beacon/samples/02.in",
        "4\n10 5 5 5\n1 2\n1 3\n1 4\n",
    ),
    ("problems/tree-beacon/samples/02.ans", "15\n"),
    (
        "problems/tree-beacon/editorial.md",
        r##"这是经典的**树上最大权独立集**模型。以 $1$ 为根进行深度优先搜索，对每个节点 $u$ 维护子树 $T_u$ 内的两态动态规划值：$f_{u, 0}$ 表示不点亮 $u$ 时的最大能量和，$f_{u, 1}$ 表示点亮 $u$ 时的最大能量和。

```typst
#insight(title: "子树无后效性")[
  当 $u$ 被点亮时，其所有直接子节点 $v in "son"(u)$ 均不可被点亮；
  当 $u$ 未被点亮时，每个子节点 $v$ 可独立地在「点亮」与「不点亮」之间取更优者。
]
```

## 状态转移方程

```typst
#import "@preview/pinit:0.2.2": *

#block(breakable: false, width: 100%, inset: (top: 24pt, bottom: 26pt), {
  set par(first-line-indent: 0pt)
  align(center, text(size: 11pt)[
    $ f_(u, 0) = sum_(v in "son"(u)) #pin(1) max(f_(v, 0), f_(v, 1)) #pin(2), quad
      f_(u, 1) = #pin(3) w_u #pin(4) + sum_(v in "son"(u)) #pin(5) f_(v, 0) #pin(6) $
  ])
  pinit-highlight(1, 2, fill: rgb("#0B6989").transparentize(85%), radius: 3pt)
  pinit-highlight(3, 4, fill: rgb("#D97706").transparentize(84%), radius: 3pt)
  pinit-highlight(5, 6, fill: rgb("#059669").transparentize(84%), radius: 3pt)

  pinit-point-from((1, 2), pin-dy: -11pt, offset-dx: -16pt, offset-dy: -22pt,
    body-dx: -44pt, body-dy: -14pt, fill: rgb("#0B6989"))[
    #text(size: 8.5pt, fill: rgb("#0B6989"), weight: "bold")[子节点自由决策]
  ]
  pinit-point-from((3, 4), pin-dy: 9pt, offset-dx: -14pt, offset-dy: 18pt,
    body-dx: -36pt, body-dy: 3pt, fill: rgb("#B45309"))[
    #text(size: 8.5pt, fill: rgb("#B45309"), weight: "bold")[点亮 $u$ 的收益]
  ]
  pinit-point-from((5, 6), pin-dy: 9pt, offset-dx: 16pt, offset-dy: 18pt,
    body-dx: -12pt, body-dy: 3pt, fill: rgb("#047857"))[
    #text(size: 8.5pt, fill: rgb("#047857"), weight: "bold")[子节点强制不选]
  ]
})
```

## 参考代码

```cpp
#include <algorithm>
#include <iostream>
#include <vector>

int main() {
    std::ios::sync_with_stdio(false);
    std::cin.tie(nullptr);

    int n;
    if (!(std::cin >> n)) return 0;
    std::vector<long long> w(n + 1);
    for (int i = 1; i <= n; ++i) std::cin >> w[i];

    std::vector<std::vector<int>> adj(n + 1);
    for (int i = 1; i < n; ++i) {
        int u, v;
        std::cin >> u >> v;
        adj[u].push_back(v);
        adj[v].push_back(u);
    }

    std::vector<int> parent(n + 1, 0), order;
    order.reserve(n);
    order.push_back(1);
    for (int i = 0; i < n; ++i) {
        int u = order[i];
        for (int v : adj[u]) {
            if (v == parent[u]) continue;
            parent[v] = u;
            order.push_back(v);
        }
    }

    std::vector<long long> f0(n + 1, 0), f1(n + 1, 0);
    for (int i = n - 1; i >= 0; --i) {
        int u = order[i];
        f1[u] = w[u];
        for (int v : adj[u]) {
            if (v == parent[u]) continue;
            f0[u] += std::max(f0[v], f1[v]);
            f1[u] += f0[v];
        }
    }
    std::cout << std::max(f0[1], f1[1]) << '\n';
    return 0;
}
```

```typst
#complexity($cal(O)(n)$, $cal(O)(n)$)
```
"##,
    ),
    // =========================================================================
    // Problem C: typst-showcase (Typst 排版与绘图能力展示)
    // =========================================================================
    (
        "problems/typst-showcase/problem.toml",
        r#"title = "Typst 排版与绘图能力展示"
rating = 2400
tags = ["typst", "cetz", "pinit", "showcase"]
group = "出版精选"

[limits]
time = "1s"
memory = "512MB"
"#,
    ),
    (
        "problems/typst-showcase/statement.md",
        r##"本篇用于集中展示 **XCPC Workstation** 内置 Typst 引擎的排版、外部库与编程能力：在同一份 Markdown 题面中自由混排 LaTeX 公式（MiTeX）、`@preview/cetz` 矢量绘图、`@preview/pinit` 公式标注、Typst 脚本自动生成以及工作区级自定义宏（`templates/preamble.typ`）。

### 1. CeTZ 矢量图论与几何绘图

借助 `@preview/cetz:0.5.3`，无需外部绘图软件或位图导出，直接在题面源码中用坐标与语义声明绘制高清矢量图（左：最短路高亮有向图；右：二维平面凸包与坐标系）：

```typst
#import "@preview/cetz:0.5.3"

#align(center, block(breakable: false, inset: (y: 4pt), {
  set text(size: 8.5pt)
  grid(
  columns: (auto, auto),
  column-gutter: 18pt,
  align: center + horizon,
  // 左图：带权有向图与最短路高亮
  cetz.canvas(length: 0.9cm, {
    import cetz.draw: *
    set-style(
      stroke: (thickness: 0.75pt, paint: rgb("#71717A")),
      mark: (fill: rgb("#71717A"), scale: 0.55),
    )

    let nodes = (
      S: (0, 1.3),
      A: (1.8, 2.4),
      B: (1.8, 0.2),
      C: (3.8, 2.2),
      D: (3.8, 0.4),
      T: (5.6, 1.3),
    )

    let edges = (
      ("S", "B", "5", (0, -0.25)),
      ("A", "B", "1", (-0.28, 0)),
      ("B", "C", "6", (-0.15, 0.25)),
      ("B", "D", "2", (0, -0.25)),
      ("C", "D", "3", (0.28, 0)),
      ("D", "T", "4", (0.12, -0.24)),
    )
    for (u, v, w, off) in edges {
      let p1 = nodes.at(u)
      let p2 = nodes.at(v)
      line(p1, p2, mark: (end: ">"), name: "e")
      content(
        ((p1.at(0) + p2.at(0)) / 2 + off.at(0), (p1.at(1) + p2.at(1)) / 2 + off.at(1)),
        text(size: 8pt, fill: rgb("#71717A"), w),
      )
    }

    let sp = (
      ("S", "A", "2", (-0.12, 0.26)),
      ("A", "C", "2", (0, 0.26)),
      ("C", "T", "1", (0.14, 0.26)),
    )
    for (u, v, w, off) in sp {
      let p1 = nodes.at(u)
      let p2 = nodes.at(v)
      line(p1, p2,
        stroke: (thickness: 1.5pt, paint: rgb("#0B6989")),
        mark: (end: ">", fill: rgb("#0B6989"), stroke: rgb("#0B6989"), scale: 0.65),
      )
      content(
        ((p1.at(0) + p2.at(0)) / 2 + off.at(0), (p1.at(1) + p2.at(1)) / 2 + off.at(1)),
        text(size: 8.5pt, weight: "bold", fill: rgb("#0B6989"), w),
      )
    }

    for (name, pos) in nodes {
      let on-sp = name in ("S", "A", "C", "T")
      circle(
        pos,
        radius: 0.34,
        fill: if on-sp { rgb("#EBF5F8") } else { white },
        stroke: if on-sp { 1.3pt + rgb("#0B6989") } else { 0.8pt + rgb("#71717A") },
      )
      content(
        pos,
        text(size: 9pt, weight: if on-sp { "bold" } else { "regular" },
          fill: if on-sp { rgb("#09546D") } else { rgb("#27272A") }, name),
      )
    }
    content((2.8, -0.55), text(size: 8.5pt, fill: rgb("#52525B"))[带权有向图最短路 $S -> A -> C -> T$（总权 $5$）])
  }),
  // 右图：二维平面凸包与坐标系
  cetz.canvas(length: 0.78cm, {
    import cetz.draw: *
    grid((0, 0), (4.5, 3.2), step: 1, stroke: 0.35pt + rgb("#E4E4E7"))
    line((-0.3, 0), (4.8, 0), stroke: 0.8pt + rgb("#52525B"), mark: (end: ">", fill: rgb("#52525B"), scale: 0.5))
    line((0, -0.3), (0, 3.5), stroke: 0.8pt + rgb("#52525B"), mark: (end: ">", fill: rgb("#52525B"), scale: 0.5))
    content((4.9, -0.25), text(size: 8pt, fill: rgb("#52525B"))[$x$])
    content((-0.28, 3.55), text(size: 8pt, fill: rgb("#52525B"))[$y$])

    let hull = ((0.8, 0.9), (2.4, 0.65), (4.1, 1.55), (3.4, 2.9), (1.35, 2.7))
    line(..hull, close: true, fill: rgb("#0B6989").transparentize(86%), stroke: 1.2pt + rgb("#0B6989"))

    for pt in ((1.85, 1.65), (2.75, 1.5), (2.25, 2.15)) {
      circle(pt, radius: 0.08, fill: rgb("#A1A1AA"), stroke: none)
    }
    let labels = ($P_1$, $P_2$, $P_3$, $P_4$, $P_5$)
    let label-specs = (
      ((-0.18, -0.05), "east"),
      ((0.0, 0.18), "south"),
      ((0.18, 0.0), "west"),
      ((0.15, 0.16), "south-west"),
      ((-0.15, 0.16), "south-east"),
    )
    for (idx, pt) in hull.enumerate() {
      circle(pt, radius: 0.1, fill: rgb("#0B6989"), stroke: white + 0.7pt)
      let (off, anc) = label-specs.at(idx)
      content((pt.at(0) + off.at(0), pt.at(1) + off.at(1)),
        anchor: anc,
        text(size: 8pt, fill: rgb("#09546D"), labels.at(idx)))
    }
    content((2.3, -0.7), text(size: 8.5pt, fill: rgb("#52525B"))[平面点集凸包（Andrew 算法）])
  }),
)}))
```

### 2. Pinit 公式精准高亮与箭头标注

借助 `@preview/pinit:0.2.2`，可以在任意复杂数学公式中放置锚点，用柔和高亮块与指向箭头拆解递推项、斜率优化结构或复杂度贡献（尤为适合题解与讲评）：

```typst
#import "@preview/pinit:0.2.2": *

#block(breakable: false, width: 100%, inset: (top: 24pt, bottom: 32pt), {
  set par(first-line-indent: 0pt)
  align(center, text(size: 11.5pt)[
    $ "dp"_i = min_(0 <= j < i) ( #pin(1) "dp"_j + S_j^2 #pin(2) - #pin(3) 2 S_i #pin(4) dot #pin(5) S_j #pin(6) ) + S_i^2 + C $
  ])
  pinit-highlight(1, 2, fill: rgb("#0B6989").transparentize(85%), radius: 3pt)
  pinit-highlight(3, 4, fill: rgb("#D97706").transparentize(84%), radius: 3pt)
  pinit-highlight(5, 6, fill: rgb("#059669").transparentize(84%), radius: 3pt)

  pinit-point-from((1, 2), pin-dy: -11pt, offset-dx: -18pt, offset-dy: -24pt,
    body-dx: -48pt, body-dy: -14pt, fill: rgb("#0B6989"))[
    #text(size: 8.5pt, fill: rgb("#0B6989"), weight: "bold")[纵截距 $y(j)$（仅与决策 $j$ 有关）]
  ]
  pinit-point-from((3, 4), pin-dy: 9pt, offset-dx: -20pt, offset-dy: 20pt,
    body-dx: -68pt, body-dy: 3pt, fill: rgb("#B45309"))[
    #text(size: 8.5pt, fill: rgb("#B45309"), weight: "bold")[查询斜率 $k(i)$（单调递增）]
  ]
  pinit-point-from((5, 6), pin-dy: 9pt, offset-dx: 28pt, offset-dy: 20pt,
    body-dx: 4pt, body-dy: 3pt, fill: rgb("#047857"))[
    #text(size: 8.5pt, fill: rgb("#047857"), weight: "bold")[横坐标 $x(j)$]
  ]
})
```

### 3. Typst 编程语言能力（排版即编程）

Typst 内置图灵完备的脚本语言：用几行 `for` 循环与递归/函数即可在编译期动态计算并生成图形与表格，彻底告别手抄重复数据。例如下方同时用脚本生成 **6 层帕斯卡三角（奇数着青蓝呈谢尔宾斯基分形）** 与 **自动编号的渐近复杂度速查表**：

```typst
#align(center, block(breakable: false, inset: (y: 2pt), grid(
  columns: (auto, auto),
  column-gutter: 22pt,
  align: center + horizon,
  {
    let n = 6
    let rows = ((1,),)
    for r in range(1, n) {
      let prev = rows.last()
      let cur = (1,)
      for j in range(1, r) {
        cur.push(prev.at(j - 1) + prev.at(j))
      }
      cur.push(1)
      rows.push(cur)
    }
    stack(
      dir: ttb,
      spacing: 2.5pt,
      ..rows.map(row => align(center, stack(
        dir: ltr,
        spacing: 2.5pt,
        ..row.map(v => {
          let odd = calc.rem(v, 2) == 1
          box(
            width: 17pt, height: 15pt, radius: 2.5pt,
            fill: if odd { rgb("#EBF5F8") } else { rgb("#F4F4F5") },
            stroke: 0.5pt + if odd { rgb("#0B6989") } else { rgb("#D4D4D8") },
            align(center + horizon, text(
              size: 7.5pt,
              weight: if odd { "bold" } else { "regular" },
              fill: if odd { rgb("#09546D") } else { rgb("#71717A") },
              str(v),
            )),
          )
        }),
      ))),
      v(4pt),
      text(size: 8.5pt, fill: rgb("#52525B"))[脚本生成 $binom(n, k)$（奇数高亮）],
    )
  },
  {
    let specs = (
      ("1", $n <= 20$, $cal(O)(2^n)$, "状态压缩 DP"),
      ("2", $n <= 500$, $cal(O)(n^3)$, "区间 DP / Floyd"),
      ("3", $n <= 5000$, $cal(O)(n^2)$, "二维递推 / 暴力"),
      ("4", $n <= 2 times 10^5$, $cal(O)(n log n)$, "数据结构 / 斜率优化"),
      ("5", $n <= 10^7$, $cal(O)(n)$, "线性贪心 / 单调队列"),
    )
    set text(size: 8.5pt)
    table(
      columns: (auto, auto, auto, auto),
      inset: (x: 6pt, y: 3pt),
      stroke: 0.45pt + rgb("#D4D4D8"),
      fill: (_, y) => if y == 0 { rgb("#F4F8FA") } else { none },
      align: (center, center, center, left),
      [*档位*], [*数据规模*], [*目标复杂度*], [*常见算法*],
      ..specs.flatten(),
    )
  },
)))
```

```typst
#insight(title: "自定义工作区宏（templates/preamble.typ）")[
  本提示框由 `templates/preamble.typ` 中的 `#insight` 宏生成，全工作区所有题面与题解均可直接调用；
  同时支持任意 `@preview/...` 官方生态包按需自动拉取缓存，以及 `@local/...` 本地自定义包。
]
```

## 输入格式

输入一行两个整数 $a, b$（$0 \le a, b \le 10^9$），用于验证样例排版效果。

```typst
#import "/problems/crc-check/figures.typ": fields
#fields("showcase-in", [例如样例输入：], ("2026", $a$), ("1122", $b$))
```

## 输出格式

输出一行一个整数，表示 $a + b$ 的值。
"##,
    ),
    ("problems/typst-showcase/samples/01.in", "2026 1122\n"),
    ("problems/typst-showcase/samples/01.ans", "3148\n"),
    (
        "problems/typst-showcase/editorial.md",
        r##"本篇演示题解册的典型排版结构：正文与公式沿用工作区统一字号，代码块使用内嵌的 JetBrains Mono 字体，并通过 `templates/preamble.typ` 提供的自定义宏统一呈现关键观察与复杂度结论。

```typst
#insight(title: "题解写作建议")[
  在题解中同样可以直接 `#import "@preview/cetz:0.5.3"` 绘制状态转移图，或用 `#import "@preview/pinit:0.2.2": *` 对关键递推式做逐步批注。
]
```

## 参考代码

```cpp
#include <iostream>

int main() {
    long long a, b;
    if (std::cin >> a >> b) {
        std::cout << a + b << '\n';
    }
    return 0;
}
```

```typst
#complexity($cal(O)(1)$, $cal(O)(1)$)
```
"##,
    ),
    // =========================================================================
    // Local Polygon Packages (4 full --strict-passing packages)
    // =========================================================================
    (
        "problems/interval-sum/problem.toml",
        r#"title = "区间求和"
rating = 900
tags = ["prefix-sum", "implementation"]
group = "本地出题包"
author = "XCPC 命题组"

[limits]
time = "1s"
memory = "256MB"
input = ""
output = ""

[[source]]
platform = "polygon"
id = "interval-sum"

[package]
checker = "ncmp"

[[test]]
gen = "gen_random"
args = ["--n", "10", "--q", "10", "--seed", "{i}"]
repeat = 2
tag = "small"

[[test]]
gen = "gen_random"
args = ["--n", "50000", "--q", "50000", "--seed", "{i}"]
repeat = 2
tag = "large"

[solutions.std]
main = true

[solutions.alt_fenwick]
expect = "AC"

[solutions.brute]
expect = ["AC", "TLE"]
tags = ["small"]

[solutions.wa_int32]
expect = "WA"
"#,
    ),
    (
        "problems/interval-sum/statement.md",
        r##"给定一个长度为 $n$ 的整数数组 $a_1, a_2, \dots, a_n$，以及 $q$ 次询问。每次询问给出区间 $[l, r]$，请你计算 $\sum_{i=l}^{r} a_i$。

## 输入格式

第一行包含两个整数 $n, q$（$1 \le n, q \le 10^5$）。

第二行包含 $n$ 个整数 $a_1, a_2, \dots, a_n$（$-10^9 \le a_i \le 10^9$）。

接下来 $q$ 行，每行包含两个整数 $l, r$（$1 \le l \le r \le n$）。

## 输出格式

输出 $q$ 行，每行一个整数表示对应区间的和。
"##,
    ),
    (
        "problems/interval-sum/editorial.md",
        r##"预处理 64 位前缀和 $S_i = \sum_{j=1}^{i} a_j$，每次询问 $O(1)$ 输出 $S_r - S_{l-1}$。注意答案可能超过 32 位有符号整数范围。
"##,
    ),
    (
        "problems/hidden-peak/problem.toml",
        r#"title = "隐藏的山峰"
rating = 1500
tags = ["interactive", "binary-search", "subtasks"]
group = "本地出题包"
author = "XCPC 命题组"

[limits]
time = "1s"
memory = "256MB"
input = ""
output = ""

[[source]]
platform = "polygon"
id = "hidden-peak"

[package]

[[subtask]]
id = "1"
score = 30
scoring = "min"

[[subtask]]
id = "2"
score = 70
depends = ["1"]
scoring = "min"

[[test]]
gen = "gen_peak"
args = ["--n", "12", "--peak", "7", "--seed", "{i}"]
repeat = 2
subtask = "1"
tag = "small"

[[test]]
gen = "gen_peak"
args = ["--n", "100000", "--seed", "{i}"]
repeat = 3
subtask = "2"
tag = "large"

[solutions.std]
main = true

[solutions.alt_ternary]
expect = "AC"

[solutions.brute_linear]
score = 30
subtasks = { "1" = "AC", "2" = "WA" }

[solutions.deadlock_no_flush]
expect = "ILE"
"#,
    ),
    (
        "problems/hidden-peak/statement.md",
        r##"这是一道**交互题**。

评测机隐藏了一个长度为 $n$（$3 \le n \le 10^5$）的**严格单峰数组** $a_1, a_2, \dots, a_n$，存在唯一峰顶 $p$（$2 \le p \le n - 1$）满足 $a_1 < \dots < a_p > \dots > a_n$。你需要通过询问找出 $p$。

## 交互方式

首先从标准输入读入两个整数 $n, Q$。之后可执行：

- `? i`（$1 \le i \le n$）：询问 $a_i$，交互器返回一个整数。
- `! p`（$1 \le p \le n$）：提交峰顶下标并退出程序。

每次输出后务必刷新缓冲区（如 `std::endl`），否则会触发 `ILE`。

## 子任务

- **子任务 1（30 分）**：$3 \le n \le 20$，$Q = 20$。
- **子任务 2（70 分）**：$3 \le n \le 10^5$，$Q = 40$，依赖子任务 1。
"##,
    ),
    (
        "problems/hidden-peak/editorial.md",
        r##"对下标区间 $[l, r]$ 二分，每次询问 $a_{\text{mid}}$ 与 $a_{\text{mid}+1}$：若 $a_{\text{mid}} < a_{\text{mid}+1}$ 则令 $l = \text{mid} + 1$，否则令 $r = \text{mid}$。总询问次数不超过 $2\lceil \log_2 n \rceil \le 34$。
"##,
    ),
    (
        "problems/secret-relay/problem.toml",
        r#"title = "双轮密信中继"
rating = 1800
tags = ["interactive", "communication", "bitmasks"]
group = "本地出题包"
author = "XCPC 命题组"

[limits]
time = "1s"
memory = "256MB"
input = ""
output = ""

[[source]]
platform = "polygon"
id = "secret-relay"

[package]
runs = 2

[[test]]
gen = "gen_msg"
args = ["--n", "8", "--flip", "0", "--seed", "1"]
tag = "clean"

[[test]]
gen = "gen_msg"
args = ["--n", "16", "--seed", "{i}"]
repeat = 2
tag = "small"

[[test]]
gen = "gen_msg"
args = ["--n", "64", "--seed", "{i}"]
repeat = 2
tag = "large"

[solutions.std]
main = true

[solutions.alt_hamming]
expect = "AC"

[solutions.wa_raw]
expect = "WA"

[solutions.ile_hang]
expect = "ILE"
"#,
    ),
    (
        "problems/secret-relay/statement.md",
        r##"这是一道**两轮通信题**（`runs = 2`）。你的程序会被独立运行两次：

1. **第 1 轮（编码端）**：收到长度为 $n$（$1 \le n \le 64$）的 01 串 $s$，输出长度不超过 $n + 8$ 的编码 01 串 $c$。
2. 中继信道对 $c$ 进行至多 **1 位翻转**，得到受损串 $c'$。
3. **第 2 轮（解码端）**：程序重新启动，收到 $n$ 与 $c'$，还原并输出原串 $s$。

## 交互方式

每轮第一行为阶段标识 `encode` 或 `decode`，第二行为 `n` 与对应 01 串；输出一行结果并刷新缓冲区后退出。两轮 CPU 时间累计计入时限。
"##,
    ),
    (
        "problems/secret-relay/editorial.md",
        r##"使用标准汉明码：在 1-based 的 $2^0, 2^1, \dots$ 位置放置校验位，其余位置顺序填入原串 $s$。第 2 轮计算各校验组异或和即得翻转位的 1-based 下标（为 0 则无错），纠错后提取数据位即可。
"##,
    ),
    (
        "problems/weighted-walk/problem.toml",
        r#"title = "带权阶梯漫步"
rating = 1700
tags = ["math", "subtasks", "partial-score"]
group = "本地出题包"
author = "XCPC 命题组"

[limits]
time = "1s"
memory = "256MB"
input = ""
output = ""

[[source]]
platform = "polygon"
id = "weighted-walk"

[package]
checker = "checker.cpp"

[[subtask]]
id = "1"
score = 30
scoring = "min"

[[subtask]]
id = "2"
score = 70
depends = ["1"]
scoring = "min"

[[test]]
gen = "gen_n"
args = ["--lo", "1", "--hi", "10", "--seed", "{i}"]
repeat = 2
subtask = "1"
tag = "small"

[[test]]
gen = "gen_n"
args = ["--lo", "100", "--hi", "100000", "--seed", "{i}"]
repeat = 2
subtask = "2"
tag = "large"

[solutions.std]
main = true

[solutions.alt_loop]
expect = "AC"

[solutions.brute]
score = 30
subtasks = { "1" = "AC", "2" = "WA" }

[solutions.approx_half]
score = [60, 70]
"#,
    ),
    (
        "problems/weighted-walk/statement.md",
        r##"给定正整数 $n$（$1 \le n \le 10^5$），求 $\sum_{i=1}^{n} i$。本题配有自定义 `checker.cpp`：若输出与最优答案相差恰好为 $1$，则通过 `quitp(0.5)` 获得该测试点 $50\%$ 的部分分。

## 输入格式

一行一个正整数 $n$。

## 输出格式

一行一个整数表示答案。

## 子任务

- **子任务 1（30 分）**：$1 \le n \le 10$。
- **子任务 2（70 分）**：$1 \le n \le 10^5$，依赖子任务 1。
"##,
    ),
    (
        "problems/weighted-walk/editorial.md",
        r##"直接使用等差数列求和公式 $\frac{n(n+1)}{2}$（注意使用 64 位整数）。`approx_half` 演示了在子任务 2 上触发 `quitp(0.5)` 从而获得 $30 + 70 \times 0.5 = 65$ 分的效果。
"##,
    ),
    // =========================================================================
    // Library Showcase Problems (5 lightweight bank problems across 2 groups)
    // =========================================================================
    (
        "problems/bipartite-coloring/problem.toml",
        r#"title = "二分图染色"
rating = 1300
tags = ["graphs", "bfs", "bipartite"]
group = "省赛候选"

[limits]
time = "1s"
memory = "256MB"

[[source]]
platform = "codeforces"
id = "1144F"
"#,
    ),
    (
        "problems/bipartite-coloring/statement.md",
        r##"给定一张包含 $n$ 个点、$m$ 条边的无向简单图，判断它是否为二分图；若是，输出一组合法的 $1/2$ 染色方案。

## 输入格式

第一行两个整数 $n, m$（$1 \le n, m \le 2 \times 10^5$）。接下来 $m$ 行每行两个整数 $u, v$ 表示一条无向边。

## 输出格式

若不是二分图输出 `NO`；否则第一行输出 `YES`，第二行输出 $n$ 个整数（$1$ 或 $2$）。
"##,
    ),
    (
        "problems/bipartite-coloring/editorial.md",
        r##"对每个连通分量运行一次 BFS/DFS 交替染色，若发现同色邻接点则无解，时间复杂度 $O(n + m)$。
"##,
    ),
    (
        "problems/flow-network-cut/problem.toml",
        r#"title = "网络最小割"
rating = 2200
tags = ["flows", "graphs", "min-cut"]
group = "省赛候选"

[limits]
time = "2s"
memory = "512MB"

[[source]]
platform = "atcoder"
id = "abc326_g"
"#,
    ),
    (
        "problems/flow-network-cut/statement.md",
        r##"给定一张有向网络，包含 $n$ 个点（源点 $1$、汇点 $n$）与 $m$ 条带非负容量的有向边，求从 $1$ 到 $n$ 的最小割容量。

## 输入格式

第一行两个整数 $n, m$（$2 \le n \le 200$，$1 \le m \le 5000$）。接下来 $m$ 行每行三个整数 $u, v, c$。

## 输出格式

输出一个整数表示最小割容量。
"##,
    ),
    (
        "problems/flow-network-cut/editorial.md",
        r##"由最大流最小割定理，直接运行 Dinic 算法求最大流即可，时间复杂度 $O(n^2 m)$。
"##,
    ),
    (
        "problems/suffix-automaton-lcp/problem.toml",
        r#"title = "后缀自动机最长公共前缀"
rating = 2500
tags = ["strings", "sam", "data-structures"]
group = "省赛候选"

[limits]
time = "2s"
memory = "512MB"

[[source]]
platform = "qoj"
id = "8412"
"#,
    ),
    (
        "problems/suffix-automaton-lcp/statement.md",
        r##"给定一个仅含小写英文字母的字符串 $S$（$|S| \le 2 \times 10^5$）与 $q$ 次询问，每次给出两个起始位置 $i, j$，求后缀 $S[i\dots]$ 与 $S[j\dots]$ 的最长公共前缀长度。

## 输入格式

第一行字符串 $S$，第二行整数 $q$，接下来 $q$ 行每行两个整数 $i, j$。

## 输出格式

输出 $q$ 行，每行一个整数表示对应 LCP 长度。
"##,
    ),
    (
        "problems/suffix-automaton-lcp/editorial.md",
        r##"对反串建后缀自动机（SAM），两个原串后缀的 LCP 等价于反串前缀在 parent tree 上 LCA 的 `len` 值。
"##,
    ),
    (
        "problems/bracket-balance/problem.toml",
        r#"title = "括号匹配修复"
rating = 1100
tags = ["greedy", "stacks"]
group = "待整理"

[limits]
time = "1s"
memory = "256MB"

[[source]]
platform = "codeforces"
id = "1214C"
"#,
    ),
    (
        "problems/bracket-balance/statement.md",
        r##"给定一个长度为 $n$（$n$ 为偶数）的括号串，每次操作可以翻转任意一个位置的括号方向，求使其变为合法括号序列的最少操作次数。

## 输入格式

第一行一个偶数 $n$（$2 \le n \le 2 \times 10^5$），第二行一个长度为 $n$ 的括号串。

## 输出格式

输出一个整数表示最少翻转次数。
"##,
    ),
    (
        "problems/sliding-window-median/problem.toml",
        r#"title = "滑动窗口中位数"
tags = ["data-structures", "two-pointers"]
group = "待整理"

[limits]
time = "1s"
memory = "256MB"
"#,
    ),
    (
        "problems/sliding-window-median/statement.md",
        r##"给定长度为 $n$ 的整数序列 $a$ 与窗口大小 $k$（$k$ 为奇数），依次输出每个长度为 $k$ 的连续子数组的中位数。

## 输入格式

第一行两个整数 $n, k$（$1 \le k \le n \le 2 \times 10^5$），第二行 $n$ 个整数 $a_1, \dots, a_n$。

## 输出格式

输出 $n - k + 1$ 个整数，以空格分隔。
"##,
    ),
    // =========================================================================
    // Contests: XCPC (`olymp-classic`) & CNOI (`cnoi-classic`, all 4 packaged)
    // =========================================================================
    (
        "contests/welcome-round/contest.toml",
        r#"name = "XCPC Workstation 示例赛"
location = "XCPC Workstation"
date = "2026 年 11 月"
language = "chinese"
cover = true

[[problem]]
label = "A"
id = "crc-check"

[[problem]]
label = "B"
id = "tree-beacon"

[[problem]]
label = "C"
id = "typst-showcase"
"#,
    ),
    (
        "contests/oi-mock/contest.toml",
        r#"name = "2026 CCF 非专业级软件能力认证（示例）"
location = "CSP-S 模拟第二轮认证 · 完整可导出数据包"
date = "2026 年 11 月 24 日 14:30 ~ 18:30"
language = "chinese"
template = "cnoi-classic"
cover = true

[[problem]]
label = "A"
id = "interval-sum"
[problem.limits]
time = "1s"
memory = "256MB"
input = "sum.in"
output = "sum.out"

[[problem]]
label = "B"
id = "weighted-walk"
[problem.limits]
time = "1s"
memory = "256MB"
input = "walk.in"
output = "walk.out"

[[problem]]
label = "C"
id = "hidden-peak"
[problem.limits]
time = "1s"
memory = "256MB"
input = "标准输入"
output = "标准输出"

[[problem]]
label = "D"
id = "secret-relay"
[problem.limits]
time = "1s"
memory = "256MB"
input = "标准输入"
output = "标准输出"
"#,
    ),
];

const EXAMPLE_GROUPS_ZH: &[&str] = &["出版精选", "本地出题包", "省赛候选", "待整理"];
const EXAMPLE_GROUPS_EN: &[&str] = &[
    "Publishing Showcase",
    "Local Polygon",
    "Regional Candidates",
    "Inbox",
];

/// Initializes an XCPC workspace at `dir` and populates it with the embedded Chinese example
/// workspace (12 problems across 4 groups, including 3 Typst showcases, 4 full `--strict`-passing
/// Polygon packages covering traditional / interactive+subtasks / 2-run communication / OI quitp,
/// 5 library showcase problems, and 2 contests: `welcome-round` and `oi-mock`).
///
/// Existing files are left untouched so re-running is safe and idempotent.
pub fn init_example_workspace(dir: impl AsRef<Path>) -> Result<Workspace> {
    init_example_workspace_with_options(dir, None, Language::Chinese)
}

/// Initializes the built-in Chinese example workspace at `dir` with an optional default
/// `template` written to `workspace.toml` (e.g. `"olymp-classic"` or `"cnoi-classic"`).
pub fn init_example_workspace_with_template(
    dir: impl AsRef<Path>,
    template: Option<&str>,
) -> Result<Workspace> {
    init_example_workspace_with_options(dir, template, Language::Chinese)
}

/// Initializes the built-in example workspace at `dir` in `language` (`Language::Chinese` or
/// `Language::English`), with an optional default `template` written to `workspace.toml`.
///
/// - `Language::Chinese`: materializes `olymp-classic` + `cnoi-classic` and seeds 12 Chinese
///   problems, 4 groups, and 2 contests (`welcome-round`, `oi-mock`).
/// - `Language::English`: materializes `olymp-classic` + `minimal` and seeds 12 English
///   problems (`statement.en.md`), 4 groups, and 2 contests (`welcome-round`, `minimal-round`).
///
/// Existing files are left untouched so re-running is safe and idempotent.
pub fn init_example_workspace_with_options(
    dir: impl AsRef<Path>,
    template: Option<&str>,
    language: Language,
) -> Result<Workspace> {
    let ws = init_workspace_with_template(dir, template)?;
    let (presets, files, snippet_files, groups) = match language {
        Language::Chinese => (
            &["olymp-classic", "cnoi-classic", "trd-compact", "trd-book"][..],
            EXAMPLE_FILES,
            EXAMPLE_SNIPPET_FILES_ZH,
            EXAMPLE_GROUPS_ZH,
        ),
        Language::English => (
            &["olymp-classic", "minimal", "trd-compact", "trd-book"][..],
            EXAMPLE_FILES_EN,
            EXAMPLE_SNIPPET_FILES_EN,
            EXAMPLE_GROUPS_EN,
        ),
    };
    if ws.workspace_groups().is_empty() {
        for &g in groups {
            let _ = ws.ensure_group_registered(g);
        }
    }
    for &preset_id in presets {
        let rel = format!("templates/{preset_id}.typ");
        let abs = ws.resolve(&rel)?;
        if fs::symlink_metadata(&abs).is_err()
            && let Some(src) = xcpc_engine::preset_source(preset_id)
        {
            ws.write(&rel, src.as_bytes())?;
        }
    }
    for &(rel, content) in files
        .iter()
        .chain(SHARED_PACKAGE_FILES.iter())
        .chain(SHARED_SNIPPET_CODE_FILES.iter())
        .chain(snippet_files.iter())
    {
        let abs = ws.resolve(rel)?;
        if fs::symlink_metadata(&abs).is_err() {
            ws.write(rel, content.as_bytes())?;
        }
    }
    Ok(ws)
}
