//! English built-in example workspace files embedded in the binary.
//!
//! Mirrors the Chinese example workspace (`crc-check`, `tree-beacon`, `typst-showcase`)
//! with two English contests (`welcome-round` in `olymp-classic`, `minimal-round` in
//! `minimal`) and `statement.en.md` so single-problem previews default to English.

pub(crate) const EXAMPLE_FILES_EN: &[(&str, &str)] = &[
    (
        "templates/preamble.typ",
        r##"// Workspace-level Typst macros (automatically injected into all statements and editorials).
// Call these helpers directly inside any ```typst block.

#let insight(title: "Key Observation", body) = pad(
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
      #text(weight: "bold")[Complexity Summary]: Time #time, Space #space.
    ]
  },
)
"##,
    ),
    // =========================================================================
    // Problem A: crc-check (Cyclic Redundancy Check)
    // =========================================================================
    (
        "problems/crc-check/problem.toml",
        r#"title = "Cyclic Redundancy Check"
rating = 1400
tags = ["implementation", "bitmask", "crc", "typst"]
group = "Publishing Showcase"

[limits]
time = "1s"
memory = "256MB"
"#,
    ),
    (
        "problems/crc-check/figures.typ",
        r##"// Algorithmic Typst visualizers for CRC:
// Executes modulo-2 division at compile time to draw transmission, XOR, and long-division figures.
#import "@preview/pinit:0.2.2": absolute-place, pin, pinit

#let accent = rgb("#0B6989")
#let color-g = rgb("#D6E6F7")
#let color-q = rgb("#FDEEB9")
#let color-r = rgb("#CDEED6")
#let color-pad = luma(228)
#let color-error = rgb("#F6C4C0")
#let bit-size = 5mm

#let xor-bit(a, b) = if a == b { "0" } else { "1" }

/// Computes the quotient, remainder, and intermediate rows of modulo-2 division.
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

/// Pure Typst vertical pull-down arrow spanning `span` rows.
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

/// Compares an error-free transmission against a single-bit flip.
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
      head(frame.len(), [Sender]),
      [],
      head(frame.len(), [Received]),
      [],
      head(r + 1, [Receiver Remainder]),
      ..row(frame, frame, frame-fill, [No error]),
      ..row(frame, wrong, wrong-fill, [Error detected]),
      grid.cell(colspan: columns, caption[
        Green: checksum; red: bit #(flipped + 1) corrupted during transmission
      ]),
    ))
  })
}

/// XOR truth table and bitwise XOR illustration.
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

/// Color-coded modulo-2 long division driven live by `divide()`.
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
    .map(j => [Bit #(j + 1) is `0`: skip])

  let cells = (:)
  let covered = ()
  let put(cells, y, x, cell) = {
    cells.insert(str(y) + "," + str(x), cell)
    cells
  }

  for (i, bit) in result.quotient.enumerate() {
    cells = put(cells, 0, offset + i, digit(bit, fill: color-q))
  }
  cells = put(cells, 0, notes, note[Yellow: quotient $Q$])

  for (j, bit) in divisor.enumerate() {
    cells = put(cells, 1, j, digit(bit, fill: color-g))
  }
  for (j, bit) in dividend.enumerate() {
    let fill = if j >= n { color-pad }
    cells = put(cells, 1, offset + j, digit(bit, fill: fill))
  }
  let first = steps.at(0, default: none)
  let intro = ([Gray: #r padded `0`s],)
  intro += skipped(result.rows.first(), first)
  cells = put(cells, 1, notes, note(intro.join[; ]))

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
    cells = put(cells, y, notes, note[Bit #(i + 1) is `1`: align $G$ and XOR])
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
    if done { remarks.push[Green: remainder $R$] }
    if remarks.len() > 0 {
      cells = put(cells, y + 1, notes, note(remarks.join[; ]))
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

/// Annotates literal input/output fields with underlines and centered labels via pinit.
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
        "problems/crc-check/statement.en.md",
        r##"Data transmitted over a noisy channel may suffer bit errors. Cyclic Redundancy Check (CRC) is a widely used error-detecting code employed in Ethernet frames and ZIP archives.

The sender appends a short checksum to the message before transmitting it. The receiver divides the received frame again: an all-`0` remainder indicates no error was detected, whereas a non-zero remainder signals corruption:

```typst
#import "/problems/crc-check/figures.typ": flow
#flow("101001", "1101", 3)
```

You act as the sender. Given two binary strings — the message $M$ and the generator $G$ — append $r$ trailing `0` bits to $M$ to form $X$, then perform modulo-2 division of $X$ by $G$ to obtain the quotient $Q$ and remainder $R$. The remainder $R$ is the checksum, and its bit length $r$ is:

```typst
#import "@preview/mannot:0.4.0": annot, markul
#block(breakable: false, width: 100%, inset: (y: 4pt), {
  set par(first-line-indent: 0pt)
  $ r = markul(|G| - 1, #<len>) #annot(<len>, pos: right, dx: 1.8em, annot-text-props: (size: 0.85em))[#h(0.35em)length of generator minus 1] $
})
```

**Binary string**: a string consisting only of `0` and `1`; $|S|$ denotes the length of $S$.

**XOR**: two equal bits yield `0`, and two different bits yield `1`. Bitwise XOR of two equal-length binary strings applies XOR at each position:

```typst
#import "/problems/crc-check/figures.typ": xor-figure
#xor-figure
```

**Modulo-2 division** proceeds as follows:

1. Scan the first $|M|$ positions of $X$ from left to right. If the current bit is `1`, write `1` to the quotient, align the first bit of $G$ with this position, and replace the aligned $|G|$ bits of $X$ with their bitwise XOR against $G$. If the current bit is `0`, write `0` to the quotient and leave $X$ unchanged.
2. The $|M|$ quotient bits form $Q$, and the last $r$ bits of $X$ form the remainder $R$.

For $M =$ `101001` and $G =$ `1101`, we have $r = 3$, and the division proceeds as:

```typst
#import "/problems/crc-check/figures.typ": ladder
#ladder("101001", "1101")
```

## Input

The first line contains an integer $T$ ($1 \le T \le 1000$), the number of test cases.

Each of the next $T$ lines contains two space-separated binary strings $M$ and $G$ ($|M| \ge 1$, $|G| \ge 2$). The first character of $G$ is always `1`; $M$ may have leading `0`s or be all `0`s. Both $\sum |M|$ and $\sum |G|$ across all test cases do not exceed $5000$.

```typst
#import "/problems/crc-check/figures.typ": fields
#fields("input", [First sample case:], ("101001", $M$), ("1101", $G$))
```

## Output

For each test case, print a single line containing $Q$ and $R$ separated by a space.

Strip leading `0`s from $Q$ (output a single `0` if $Q$ is all zeros); output $R$ with **exact** length $r$, preserving any leading `0`s.

```typst
#import "/problems/crc-check/figures.typ": fields
#fields("output", [Corresponding output:], ("110101", $Q$), ("001", $R$))
```

## Constraints

For all test cases, $1 \le T \le 1000$, $\sum |M| \le 5000$, and $\sum |G| \le 5000$.

| Tests | $\sum \lvert M \rvert, \sum \lvert G \rvert \le$ | Special Property |
| :---: | :---: | :---: |
| $1 \sim 4$ | $50$ | $T = 1$ |
| $5 \sim 7$ | $1000$ | $M$ has no leading `0`s |
| $8 \sim 10$ | $5000$ | None |
"##,
    ),
    (
        "problems/crc-check/samples/01.in",
        "2\n101001 1101\n0011 101\n",
    ),
    ("problems/crc-check/samples/01.ans", "110101 001\n11 10\n"),
    (
        "problems/crc-check/editorial.md",
        r##"Simulate modulo-2 division directly according to its definition.

```typst
#insight(title: "Edge Cases")[
  1. Strip leading `0`s from the quotient $Q$; if every bit is `0`, output a single `0`.
  2. Keep all $r = |G| - 1$ bits of the remainder $R$, including leading `0`s.
]
```

## Reference Implementation

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
    // Problem B: tree-beacon (Tree Beacons — CeTZ graph problem)
    // =========================================================================
    (
        "problems/tree-beacon/problem.toml",
        r#"title = "Tree Beacons"
rating = 1900
tags = ["trees", "dp", "heavy-light", "graph"]
group = "Publishing Showcase"

[limits]
time = "1s"
memory = "512MB"
"#,
    ),
    (
        "problems/tree-beacon/statement.en.md",
        r##"You are given a rooted tree with $n$ vertices numbered $1, 2, \ldots, n$, rooted at vertex $1$. Vertex $u$ holds an energy beacon with initial energy $w_u$.

You want to activate a subset $S$ of beacons satisfying the **non-adjacency constraint**: whenever $u \in S$, its parent $p_u \notin S$. You may also designate a downward path starting from root $1$ as the **heavy spine** (edges on this path are heavy edges; all other edges are light edges). No two endpoints of any tree edge may be activated simultaneously.

```typst
#import "@preview/cetz:0.5.3"

#align(center, block(breakable: false, inset: (y: 4pt), {
  set text(size: 9pt)
  cetz.canvas(length: 0.9cm, {
    import cetz.draw: *

    // Soft shaded region for subtree T_2
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
    content((0.0, 2.65), text(size: 8pt, fill: rgb("#0B6989"), weight: "bold")[Subtree $T_2$])

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

    // Light edges (dashed)
    for (u, v) in light {
      line(pos.at(u), pos.at(v), stroke: (paint: rgb("#94A3B8"), thickness: 0.9pt, dash: "densely-dashed"))
    }
    // Heavy edges (bold teal)
    for (u, v) in heavy {
      line(pos.at(u), pos.at(v), stroke: (paint: rgb("#0B6989"), thickness: 1.8pt))
    }

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

    // Compact legend at bottom-right
    let lx = 5.45
    let ly = -0.35
    line((lx, ly), (lx + 0.55, ly), stroke: 1.8pt + rgb("#0B6989"))
    content((lx + 0.72, ly), anchor: "west", text(size: 7.8pt, fill: rgb("#334155"))[Heavy path])
    line((lx, ly - 0.42), (lx + 0.55, ly - 0.42), stroke: (paint: rgb("#94A3B8"), thickness: 0.9pt, dash: "densely-dashed"))
    content((lx + 0.72, ly - 0.42), anchor: "west", text(size: 7.8pt, fill: rgb("#334155"))[Light edge])
    circle((lx + 0.28, ly - 0.84), radius: 0.14, fill: rgb("#0B6989"), stroke: none)
    content((lx + 0.72, ly - 0.84), anchor: "west", text(size: 7.8pt, fill: rgb("#334155"))[Lit beacon])
  })
}))
```

Find the maximum possible sum of energies of the activated beacons under the non-adjacency constraint.

## Input

The first line contains a positive integer $n$ ($1 \le n \le 2 \times 10^5$), the number of vertices.

The second line contains $n$ integers $w_1, w_2, \ldots, w_n$ ($0 \le w_u \le 10^9$), the energy of each beacon.

Each of the next $n - 1$ lines contains two integers $u, v$ ($1 \le u, v \le n$), representing an undirected edge of the tree.

## Output

Output a single integer — the maximum total energy of activated beacons.

## Note

Sample 1 corresponds to the figure above: activating $\{1, 4, 5, 6, 7\}$ yields total energy $4 + 3 + 7 + 2 + 8 = 24$, while $\{2, 6, 7, 8\}$ yields $6 + 2 + 8 + 4 = 20$. The maximum is $24$.

## Constraints

For all test cases, $1 \le n \le 2 \times 10^5$ and $0 \le w_u \le 10^9$.

| Tests | $n \le$ | Special Property |
| :---: | :---: | :---: |
| $1 \sim 3$ | $20$ | None |
| $4 \sim 5$ | $2 \times 10^5$ | Tree is a chain |
| $6 \sim 7$ | $2 \times 10^5$ | Tree is a star |
| $8 \sim 10$ | $2 \times 10^5$ | None |
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
        r##"This is the classic **Maximum Weight Independent Set on a Tree**. Root the tree at $1$ and maintain two DP states for each subtree $T_u$: $f_{u, 0}$ is the maximum energy in $T_u$ when $u$ is not activated, and $f_{u, 1}$ is the maximum energy when $u$ is activated.

```typst
#insight(title: "Subtree Independence")[
  If $u$ is activated, none of its direct children $v in "son"(u)$ may be activated.
  If $u$ is not activated, each child $v$ independently chooses the better of its two states.
]
```

## DP Transitions

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
    body-dx: -54pt, body-dy: -14pt, fill: rgb("#0B6989"))[
    #text(size: 8.5pt, fill: rgb("#0B6989"), weight: "bold")[Children decide freely]
  ]
  pinit-point-from((3, 4), pin-dy: 9pt, offset-dx: -14pt, offset-dy: 18pt,
    body-dx: -46pt, body-dy: 3pt, fill: rgb("#B45309"))[
    #text(size: 8.5pt, fill: rgb("#B45309"), weight: "bold")[Gain from lighting $u$]
  ]
  pinit-point-from((5, 6), pin-dy: 9pt, offset-dx: 16pt, offset-dy: 18pt,
    body-dx: -12pt, body-dy: 3pt, fill: rgb("#047857"))[
    #text(size: 8.5pt, fill: rgb("#047857"), weight: "bold")[Children must stay off]
  ]
})
```

## Reference Implementation

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
    // Problem C: typst-showcase (Typst Typesetting & Graphics Showcase)
    // =========================================================================
    (
        "problems/typst-showcase/problem.toml",
        r#"title = "Typst Typesetting & Graphics Showcase"
rating = 2400
tags = ["typst", "cetz", "pinit", "showcase"]
group = "Publishing Showcase"

[limits]
time = "1s"
memory = "512MB"
"#,
    ),
    (
        "problems/typst-showcase/statement.en.md",
        r##"This problem demonstrates the typesetting, package ecosystem, and scripting capabilities of **XCPC Workstation**'s embedded Typst engine: seamlessly mix LaTeX math (MiTeX), `@preview/cetz` vector graphics, `@preview/pinit` formula annotations, programmatic Typst scripting, and workspace macros (`templates/preamble.typ`) in a single Markdown statement.

### 1. Vector Graph & Geometry Diagrams with CeTZ

With `@preview/cetz:0.5.3`, draw crisp vector figures directly in source without external drawing tools (left: shortest path in a weighted directed graph; right: 2D convex hull):

```typst
#import "@preview/cetz:0.5.3"

#align(center, block(breakable: false, inset: (y: 4pt), {
  set text(size: 8.5pt)
  grid(
  columns: (auto, auto),
  column-gutter: 18pt,
  align: center + horizon,
  // Left: weighted digraph with shortest path highlighted
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
    content((2.8, -0.55), text(size: 8.5pt, fill: rgb("#52525B"))[Shortest path $S -> A -> C -> T$ (cost $5$)])
  }),
  // Right: 2D convex hull on a coordinate grid
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
    content((2.3, -0.7), text(size: 8.5pt, fill: rgb("#52525B"))[2D Convex Hull (Andrew's algorithm)])
  }),
)}))
```

### 2. Formula Highlights & Callouts with Pinit

Use `@preview/pinit:0.2.2` to anchor highlights and arrows directly inside equations — ideal for explaining DP recurrences and Convex Hull Trick structure:

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
    body-dx: -54pt, body-dy: -14pt, fill: rgb("#0B6989"))[
    #text(size: 8.5pt, fill: rgb("#0B6989"), weight: "bold")[Intercept $y(j)$ (depends only on $j$)]
  ]
  pinit-point-from((3, 4), pin-dy: 9pt, offset-dx: -20pt, offset-dy: 20pt,
    body-dx: -68pt, body-dy: 3pt, fill: rgb("#B45309"))[
    #text(size: 8.5pt, fill: rgb("#B45309"), weight: "bold")[Query slope $k(i)$ (monotone)]
  ]
  pinit-point-from((5, 6), pin-dy: 9pt, offset-dx: 28pt, offset-dy: 20pt,
    body-dx: 4pt, body-dy: 3pt, fill: rgb("#047857"))[
    #text(size: 8.5pt, fill: rgb("#047857"), weight: "bold")[Abscissa $x(j)$]
  ]
})
```

### 3. Programmable Typesetting

Typst ships a full scripting language: compute tables and figures at compile time instead of copying values by hand. Below, a short loop renders a **6-row Pascal's triangle (odd entries highlighted)** alongside an **asymptotic complexity quick-reference table**:

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
      text(size: 8.5pt, fill: rgb("#52525B"))[Generated $binom(n, k)$ (odd highlighted)],
    )
  },
  {
    let specs = (
      ("1", $n <= 20$, $cal(O)(2^n)$, "Bitmask DP"),
      ("2", $n <= 500$, $cal(O)(n^3)$, "Interval DP / Floyd"),
      ("3", $n <= 5000$, $cal(O)(n^2)$, "2D DP / Quadratic"),
      ("4", $n <= 2 times 10^5$, $cal(O)(n log n)$, "Data Structures / CHT"),
      ("5", $n <= 10^7$, $cal(O)(n)$, "Greedy / Monotone Queue"),
    )
    set text(size: 8.5pt)
    table(
      columns: (auto, auto, auto, auto),
      inset: (x: 6pt, y: 3pt),
      stroke: 0.45pt + rgb("#D4D4D8"),
      fill: (_, y) => if y == 0 { rgb("#F4F8FA") } else { none },
      align: (center, center, center, left),
      [*Tier*], [*Bound*], [*Target*], [*Typical Technique*],
      ..specs.flatten(),
    )
  },
)))
```

```typst
#insight(title: "Workspace Macros (templates/preamble.typ)")[
  This callout box is defined by `#insight` in `templates/preamble.typ` and is available across every statement and editorial in the workspace.
  Any `@preview/...` registry package is fetched and cached automatically on demand, alongside `@local/...` packages.
]
```

## Input

A single line containing two integers $a, b$ ($0 \le a, b \le 10^9$) to verify sample formatting.

```typst
#import "/problems/crc-check/figures.typ": fields
#fields("showcase-in", [Sample input:], ("2026", $a$), ("1122", $b$))
```

## Output

Output a single integer, the value of $a + b$.
"##,
    ),
    ("problems/typst-showcase/samples/01.in", "2026 1122\n"),
    ("problems/typst-showcase/samples/01.ans", "3148\n"),
    (
        "problems/typst-showcase/editorial.md",
        r##"This editorial illustrates the standard booklet layout: body text and math match the workspace typography, code blocks use bundled JetBrains Mono, and `templates/preamble.typ` provides reusable callout and complexity macros.

```typst
#insight(title: "Editorial Tip")[
  You can also `#import "@preview/cetz:0.5.3"` for state diagrams or `#import "@preview/pinit:0.2.2": *` to annotate key recurrences directly inside editorials.
]
```

## Reference Implementation

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
        r#"title = "Range Sum Queries"
rating = 900
tags = ["prefix-sum", "implementation"]
group = "Local Polygon"
author = "XCPC Problem Committee"

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
        "problems/interval-sum/statement.en.md",
        r##"Given an integer array $a_1, a_2, \dots, a_n$ and $q$ queries, each specifying an interval $[l, r]$, compute $\sum_{i=l}^{r} a_i$.

## Input

The first line contains two integers $n$ and $q$ ($1 \le n, q \le 10^5$).

The second line contains $n$ integers $a_1, a_2, \dots, a_n$ ($-10^9 \le a_i \le 10^9$).

Each of the next $q$ lines contains two integers $l$ and $r$ ($1 \le l \le r \le n$).

## Output

Output $q$ lines, each containing the sum over the corresponding interval.
"##,
    ),
    (
        "problems/interval-sum/editorial.md",
        r##"Precompute 64-bit prefix sums $S_i = \sum_{j=1}^{i} a_j$ and answer each query in $O(1)$ as $S_r - S_{l-1}$. Beware 32-bit integer overflow.
"##,
    ),
    (
        "problems/hidden-peak/problem.toml",
        r#"title = "Hidden Mountain Peak"
rating = 1500
tags = ["interactive", "binary-search", "subtasks"]
group = "Local Polygon"
author = "XCPC Problem Committee"

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
        "problems/hidden-peak/statement.en.md",
        r##"This is an **interactive problem**.

The judge hides a strictly unimodal array $a_1, a_2, \dots, a_n$ ($3 \le n \le 10^5$) with a unique peak index $p$ ($2 \le p \le n - 1$) such that $a_1 < \dots < a_p > \dots > a_n$. Find $p$ within $Q$ queries.

## Interaction

Read two integers $n$ and $Q$ from standard input. Then issue:

- `? i` ($1 \le i \le n$): query $a_i$; the interactor replies with a single integer.
- `! p` ($1 \le p \le n$): report the peak index and terminate.

Flush output after every line (e.g., `std::endl`), otherwise your program may receive `ILE`.

## Subtasks

- **Subtask 1 (30 points)**: $3 \le n \le 20$, $Q = 20$.
- **Subtask 2 (70 points)**: $3 \le n \le 10^5$, $Q = 40$, depends on Subtask 1.
"##,
    ),
    (
        "problems/hidden-peak/editorial.md",
        r##"Binary search on $[l, r]$ by comparing $a_{\text{mid}}$ and $a_{\text{mid}+1}$: if $a_{\text{mid}} < a_{\text{mid}+1}$ set $l = \text{mid} + 1$, else set $r = \text{mid}$. Uses at most $2\lceil \log_2 n \rceil \le 34$ queries.
"##,
    ),
    (
        "problems/secret-relay/problem.toml",
        r#"title = "Two-Pass Secret Relay"
rating = 1800
tags = ["interactive", "communication", "bitmasks"]
group = "Local Polygon"
author = "XCPC Problem Committee"

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
        "problems/secret-relay/statement.en.md",
        r##"This is a **two-pass communication problem** (`runs = 2`). Your program is executed twice independently:

1. **Pass 1 (Encoder)**: given a binary string $s$ of length $n$ ($1 \le n \le 64$), output an encoded binary string $c$ of length at most $n + 8$.
2. The relay channel flips at most **1 bit** of $c$, producing $c'$.
3. **Pass 2 (Decoder)**: started fresh with no memory from Pass 1, receive $n$ and $c'$ and recover $s$.

## Interaction

Each pass starts with a phase tag (`encode` or `decode`) on the first line and `n` plus the binary string on the second line. Output a single binary string, flush, and exit. Total CPU time is summed across both passes.
"##,
    ),
    (
        "problems/secret-relay/editorial.md",
        r##"Use a standard Hamming code: place parity bits at 1-based powers of two and fill remaining slots with $s$. In Pass 2, XORing each parity group yields the 1-based flipped index (or 0 if intact).
"##,
    ),
    (
        "problems/weighted-walk/problem.toml",
        r#"title = "Weighted Staircase Walk"
rating = 1700
tags = ["math", "subtasks", "partial-score"]
group = "Local Polygon"
author = "XCPC Problem Committee"

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
        "problems/weighted-walk/statement.en.md",
        r##"Given a positive integer $n$ ($1 \le n \le 10^5$), compute $\sum_{i=1}^{n} i$. This problem includes a custom `checker.cpp` that awards $50\%$ partial credit via `quitp(0.5)` if your answer differs from the optimal value by exactly $1$.

## Input

A single positive integer $n$.

## Output

A single integer.

## Subtasks

- **Subtask 1 (30 points)**: $1 \le n \le 10$.
- **Subtask 2 (70 points)**: $1 \le n \le 10^5$, depends on Subtask 1.
"##,
    ),
    (
        "problems/weighted-walk/editorial.md",
        r##"Evaluate $\frac{n(n+1)}{2}$ in 64-bit arithmetic. Solution `approx_half` intentionally returns $\text{ans} - 1$ on Subtask 2 to demonstrate `quitp(0.5)` scoring $30 + 70 \times 0.5 = 65$.
"##,
    ),
    // =========================================================================
    // Library Showcase Problems (5 lightweight bank problems across 2 groups)
    // =========================================================================
    (
        "problems/bipartite-coloring/problem.toml",
        r#"title = "Bipartite Coloring"
rating = 1300
tags = ["graphs", "bfs", "bipartite"]
group = "Regional Candidates"

[limits]
time = "1s"
memory = "256MB"

[[source]]
platform = "codeforces"
id = "1144F"
"#,
    ),
    (
        "problems/bipartite-coloring/statement.en.md",
        r##"Given a simple undirected graph with $n$ vertices and $m$ edges, determine whether it is bipartite; if so, output any valid $1/2$ vertex coloring.

## Input

The first line contains $n$ and $m$ ($1 \le n, m \le 2 \times 10^5$), followed by $m$ edges $u, v$.

## Output

Print `NO` if not bipartite; otherwise print `YES` and $n$ colors in $\{1, 2\}$.
"##,
    ),
    (
        "problems/bipartite-coloring/editorial.md",
        r##"Run BFS/DFS 2-coloring on each connected component in $O(n + m)$ time.
"##,
    ),
    (
        "problems/flow-network-cut/problem.toml",
        r#"title = "Minimum Network Cut"
rating = 2200
tags = ["flows", "graphs", "min-cut"]
group = "Regional Candidates"

[limits]
time = "2s"
memory = "512MB"

[[source]]
platform = "atcoder"
id = "abc326_g"
"#,
    ),
    (
        "problems/flow-network-cut/statement.en.md",
        r##"Given a directed capacitated network with $n$ vertices (source $1$, sink $n$) and $m$ edges, find the minimum $1$–$n$ cut capacity.

## Input

The first line contains $n$ and $m$ ($2 \le n \le 200$, $1 \le m \le 5000$), followed by $m$ triples $u, v, c$.

## Output

Output a single integer, the minimum cut capacity.
"##,
    ),
    (
        "problems/flow-network-cut/editorial.md",
        r##"By the max-flow min-cut theorem, compute maximum flow using Dinic's algorithm in $O(n^2 m)$.
"##,
    ),
    (
        "problems/suffix-automaton-lcp/problem.toml",
        r#"title = "Suffix Automaton LCP"
rating = 2500
tags = ["strings", "sam", "data-structures"]
group = "Regional Candidates"

[limits]
time = "2s"
memory = "512MB"

[[source]]
platform = "qoj"
id = "8412"
"#,
    ),
    (
        "problems/suffix-automaton-lcp/statement.en.md",
        r##"Given a lowercase string $S$ ($|S| \le 2 \times 10^5$) and $q$ queries $(i, j)$, compute the longest common prefix length of suffixes $S[i\dots]$ and $S[j\dots]$.

## Input

First line $S$, second line $q$, followed by $q$ pairs $(i, j)$.

## Output

Output $q$ lines with the LCP lengths.
"##,
    ),
    (
        "problems/suffix-automaton-lcp/editorial.md",
        r##"Build a Suffix Automaton on the reversed string; the LCP of two suffixes equals the `len` of their LCA in the parent tree.
"##,
    ),
    (
        "problems/bracket-balance/problem.toml",
        r#"title = "Bracket Balance Repair"
rating = 1100
tags = ["greedy", "stacks"]
group = "Inbox"

[limits]
time = "1s"
memory = "256MB"

[[source]]
platform = "codeforces"
id = "1214C"
"#,
    ),
    (
        "problems/bracket-balance/statement.en.md",
        r##"Given an even-length bracket string of length $n$, compute the minimum number of single-character flips needed to make it a regular bracket sequence.

## Input

First line an even integer $n$ ($2 \le n \le 2 \times 10^5$), second line the bracket string.

## Output

A single integer, the minimum number of flips.
"##,
    ),
    (
        "problems/sliding-window-median/problem.toml",
        r#"title = "Sliding Window Median"
tags = ["data-structures", "two-pointers"]
group = "Inbox"

[limits]
time = "1s"
memory = "256MB"
"#,
    ),
    (
        "problems/sliding-window-median/statement.en.md",
        r##"Given an array $a$ of length $n$ and an odd window size $k$, output the median of every contiguous subarray of length $k$.

## Input

First line $n$ and $k$ ($1 \le k \le n \le 2 \times 10^5$), second line $n$ integers.

## Output

Output $n - k + 1$ space-separated medians.
"##,
    ),
    // =========================================================================
    // Contests: XCPC (`olymp-classic`) & Minimal (`minimal`, all 4 packaged)
    // =========================================================================
    (
        "contests/welcome-round/contest.toml",
        r#"name = "XCPC Workstation Sample Round"
location = "XCPC Workstation"
date = "November 2026"
language = "english"
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
        "contests/minimal-round/contest.toml",
        r#"name = "Local Polygon Exportable Round"
location = "Minimal Template · Ready for DOMjudge / Hydro Export"
date = "November 2026"
language = "english"
template = "minimal"
cover = true

[[problem]]
label = "A"
id = "interval-sum"

[[problem]]
label = "B"
id = "weighted-walk"

[[problem]]
label = "C"
id = "hidden-peak"

[[problem]]
label = "D"
id = "secret-relay"
"#,
    ),
];
