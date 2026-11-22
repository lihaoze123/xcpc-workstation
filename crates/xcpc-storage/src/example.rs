//! Built-in example workspace embedded in the binary for `xcpc init --example` and
//! `Preview.initExampleWorkspace` / `Preview.pickAndInitExampleWorkspace`.
//!
//! Contains a dedicated Typst capability showcase (`typst-showcase` featuring CeTZ
//! vector graphics, Pinit formula annotations, programmatic Typst scripting, and
//! `.xcpc/preamble.typ` macros), a polished real contest problem (`lantern-walk`),
//! and two contests (`welcome-round` in `olymp-classic`, `oi-mock` in `cnoi-classic`)
//! demonstrating live-reference multi-template publishing.

use std::fs;
use std::path::Path;

use crate::{Result, Workspace, init_workspace_with_template};

/// Static files seeded into an example workspace (relative to workspace root).
const EXAMPLE_FILES: &[(&str, &str)] = &[
    (
        ".xcpc/preamble.typ",
        r##"// 工作区级 Typst 自定义宏（自动注入所有题面与题解编译作用域）
// 在题面或题解中使用 ```typst 代码块即可直接调用这里定义的宏。

#let insight(title: "关键观察", body) = pad(
  left: 1pt,
  block(
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
      *复杂度总结*：时间复杂度 #time，空间复杂度 #space。
    ]
  },
)
"##,
    ),
    (
        "problems/typst-showcase/problem.toml",
        r#"title = "Typst 排版与绘图能力展示"
rating = 2400
tags = ["typst", "cetz", "pinit", "showcase"]

[limits]
time = "1s"
memory = "512MB"
"#,
    ),
    (
        "problems/typst-showcase/statement.md",
        r##"本篇用于集中展示 **XCPC Workstation** 内置 Typst 引擎的排版、外部库与编程能力：在同一份 Markdown 题面中自由混排 LaTeX 公式（MiTeX）、`@preview/cetz` 矢量绘图、`@preview/pinit` 公式标注、Typst 脚本自动生成以及工作区级自定义宏（`.xcpc/preamble.typ`）。

### 1. CeTZ 矢量图论与几何绘图

借助 `@preview/cetz:0.5.3`，无需外部绘图软件或位图导出，直接在题面源码中用坐标与语义声明绘制高清矢量图（左：最短路高亮有向图；右：二维平面凸包与坐标系）：

```typst
#import "@preview/cetz:0.5.3"

#align(center, block(inset: (y: 4pt), grid(
  columns: (auto, auto),
  column-gutter: 18pt,
  align: center + horizon,
  // 左图：带权有向图与最短路高亮
  cetz.canvas(length: 0.92cm, {
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

    // 普通边
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

    // 最短路高亮边 S -> A -> C -> T
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

    // 节点圆圈（盖在边端点上方）
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
    // 淡灰网格
    grid((0, 0), (4.5, 3.2), step: 1, stroke: 0.35pt + rgb("#E4E4E7"))
    // 坐标轴
    line((-0.3, 0), (4.8, 0), stroke: 0.8pt + rgb("#52525B"), mark: (end: ">", fill: rgb("#52525B"), scale: 0.5))
    line((0, -0.3), (0, 3.5), stroke: 0.8pt + rgb("#52525B"), mark: (end: ">", fill: rgb("#52525B"), scale: 0.5))
    content((4.9, -0.25), text(size: 8pt, fill: rgb("#52525B"))[$x$])
    content((-0.28, 3.55), text(size: 8pt, fill: rgb("#52525B"))[$y$])

    // 凸包填充多边形
    let hull = ((0.6, 0.8), (2.4, 0.4), (4.1, 1.5), (3.4, 2.9), (1.3, 2.7))
    line(..hull, close: true, fill: rgb("#0B6989").transparentize(86%), stroke: 1.2pt + rgb("#0B6989"))

    // 内部散点
    for pt in ((1.8, 1.6), (2.7, 1.4), (2.2, 2.1)) {
      circle(pt, radius: 0.08, fill: rgb("#A1A1AA"), stroke: none)
    }
    // 凸包极点
    let labels = ($P_1$, $P_2$, $P_3$, $P_4$, $P_5$)
    let offsets = ((-0.32, -0.22), (0.05, -0.32), (0.35, -0.05), (0.28, 0.24), (-0.28, 0.24))
    for (idx, pt) in hull.enumerate() {
      circle(pt, radius: 0.1, fill: rgb("#0B6989"), stroke: white + 0.7pt)
      let off = offsets.at(idx)
      content((pt.at(0) + off.at(0), pt.at(1) + off.at(1)),
        text(size: 8pt, fill: rgb("#09546D"), labels.at(idx)))
    }
    content((2.3, -0.7), text(size: 8.5pt, fill: rgb("#52525B"))[平面点集凸包（Andrew 算法）])
  }),
)))
```

### 2. Pinit 公式精准高亮与箭头标注

借助 `@preview/pinit:0.2.2`，可以在任意复杂数学公式中放置锚点，用柔和高亮块与指向箭头拆解递推项、斜率优化结构或复杂度贡献（尤为适合题解与讲评）：

```typst
#import "@preview/pinit:0.2.2": *

#block(width: 100%, inset: (top: 24pt, bottom: 32pt), {
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
    // 6 层帕斯卡三角（奇数着青蓝呈谢尔宾斯基分形）
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
  // 右侧：数据驱动生成复杂度参考表
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
#insight(title: "自定义工作区宏（.xcpc/preamble.typ）")[
  本提示框由 `.xcpc/preamble.typ` 中的 `#insight` 宏生成，全工作区所有题面与题解均可直接调用；
  同时支持任意 `@preview/...` 官方生态包按需自动拉取缓存，以及 `@local/...` 本地自定义包。
]
```

## 输入格式

输入一行两个整数 $a, b$（$0 \le a, b \le 10^9$），用于验证样例排版与字段标注效果：

```typst
#import "@preview/pinit:0.2.2": absolute-place, pin, pinit

#let accent = rgb("#0B6989")
#let fields(id, prefix, ..parts) = {
  let names = parts.pos().enumerate().map(((i, _)) => id + "-" + str(i))
  let text-line = parts.pos().enumerate().map(((i, part)) => [
    #pin(names.at(i) + "-a")#raw(part.at(0))#pin(names.at(i) + "-b")
  ]).join(h(1.2em))
  let marks = parts.pos().enumerate().map(((i, part)) => pinit(
    names.at(i) + "-a", names.at(i) + "-b",
    callback: (a, b) => {
      absolute-place(dx: a.x, dy: a.y + 3.5pt, line(length: b.x - a.x, stroke: 1pt + accent))
      absolute-place(
        dx: (a.x + b.x) / 2 - 2cm, dy: a.y + 5.5pt,
        box(width: 4cm, align(center, text(size: 8.5pt, fill: accent, part.at(1)))),
      )
    },
  )).join()
  block(width: 100%, inset: (top: 4pt, bottom: 16pt), {
    set par(first-line-indent: 0pt)
    align(center)[#text(size: 9pt, fill: rgb("#71717A"))[#prefix]#h(1em)#text-line]
    marks
  })
}

#fields("in", "样例输入：", ("2026", [第一个整数 $a$]), ("1122", [第二个整数 $b$]))
```

## 输出格式

输出一行一个整数，表示 $a + b$ 的值。
"##,
    ),
    ("problems/typst-showcase/samples/01.in", "2026 1122\n"),
    ("problems/typst-showcase/samples/01.ans", "3148\n"),
    (
        "problems/typst-showcase/editorial.md",
        r##"本篇演示题解册的典型排版结构：正文与公式沿用工作区统一字号，代码块使用内嵌的 JetBrains Mono 字体，并通过 `.xcpc/preamble.typ` 提供的自定义宏统一呈现关键观察与复杂度结论。

```typst
#insight(title: "题解写作建议")[
  在题解中同样可以直接 `#import "@preview/cetz:0.5.3"` 绘制状态转移图或用 `#import "@preview/pinit:0.2.2": *` 对关键递推式做逐步批注。
]
```

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
    (
        "problems/lantern-walk/problem.toml",
        r#"title = "灯笼小径"
rating = 1500
tags = ["greedy", "dp", "prefix-sum"]

[limits]
time = "1s"
memory = "256MB"
"#,
    ),
    (
        "problems/lantern-walk/statement.md",
        r##"夜幕降临时，校园里一条笔直的小径上排着 $n$ 盏灯笼，自左向右编号为 $1, 2, \ldots, n$。第 $i$ 盏灯笼初始有一个亮度值 $a_i$。

你可以进行**至多一次**操作：选择一个连续区间 $[l, r]$（$1 \le l \le r \le n$），将该区间内所有灯笼的亮度同时加上 $k$。

```typst
#import "@preview/cetz:0.5.3"

#align(center, block(inset: (y: 4pt), cetz.canvas(length: 1.05cm, {
  import cetz.draw: *
  let vals = ("-5", "-1", "4", "-2", "-5")
  let after = ("-5", "+2", "+7", "+1", "-5")

  // 顶部操作区间括号 [2, 4] + 3
  line((1.5, 0.85), (4.5, 0.85), stroke: 1pt + rgb("#0B6989"))
  line((1.5, 0.85), (1.5, 0.65), stroke: 1pt + rgb("#0B6989"))
  line((4.5, 0.85), (4.5, 0.65), stroke: 1pt + rgb("#0B6989"))
  content((3.0, 1.12), text(size: 8.5pt, weight: "bold", fill: rgb("#0B6989"))[选择区间 $[2, 4]$，亮度 $+3$])

  // 相邻连线
  for i in range(4) {
    line((i * 1.5 + 0.42, 0), ((i + 1) * 1.5 - 0.42, 0), stroke: 0.7pt + rgb("#A1A1AA"))
  }

  // 灯笼节点
  for i in range(5) {
    let x = i * 1.5
    let lit = i >= 1 and i <= 3
    circle(
      (x, 0),
      radius: 0.42,
      fill: if lit { rgb("#EBF5F8") } else { white },
      stroke: if lit { 1.3pt + rgb("#0B6989") } else { 0.8pt + rgb("#A1A1AA") },
    )
    content(
      (x, 0),
      text(size: 9pt, weight: if lit { "bold" } else { "regular" },
        fill: if lit { rgb("#09546D") } else { rgb("#27272A") }, vals.at(i)),
    )
    content(
      (x, -0.72),
      text(size: 8pt, fill: if lit { rgb("#0B6989") } else { rgb("#71717A") },
        if lit { after.at(i) } else { "#" + str(i + 1) }),
    )
  }
})))
```

请你求出操作之后，整条小径上**最大连续子段和**的最大可能值。允许选取空子段，空子段的和视为 $0$。

## 输入格式

第一行包含两个整数 $n, k$（$1 \le n \le 2 \times 10^5$，$-10^9 \le k \le 10^9$），分别表示灯笼数量与单次调整的增量。

第二行包含 $n$ 个整数 $a_1, a_2, \ldots, a_n$（$-10^9 \le a_i \le 10^9$），表示每盏灯笼的初始亮度。

## 输出格式

输出一行一个整数，表示至多一次操作后能获得的最大连续子段和。

## 样例解释

样例 1 中，选择区间 $[2, 4]$ 并加上 $k = 3$ 后，亮度序列变为 $[-5, 2, 7, 1, -5]$，区间 $[2, 4]$ 的和为 $2 + 7 + 1 = 10$，可以证明这是最大可能值。

样例 2 中，最优策略是选择空子段，和为 $0$。

## 数据范围

对于所有测试数据，保证 $1 \le n \le 2 \times 10^5$，$-10^9 \le k, a_i \le 10^9$。

| 测试点 | $n \le$ | 特殊性质 |
| :---: | :---: | :---: |
| $1 \sim 3$ | $200$ | 无 |
| $4 \sim 6$ | $5\,000$ | $k > 0$ |
| $7 \sim 10$ | $2 \times 10^5$ | 无 |
"##,
    ),
    (
        "problems/lantern-walk/samples/01.in",
        "5 3\n-5 -1 4 -2 -5\n",
    ),
    ("problems/lantern-walk/samples/01.ans", "10\n"),
    ("problems/lantern-walk/samples/02.in", "4 -5\n-1 -2 -3 -4\n"),
    ("problems/lantern-walk/samples/02.ans", "0\n"),
    (
        "problems/lantern-walk/editorial.md",
        r##"注意到若最优子段为 $[L, R]$，操作区间 $[l, r]$ 只需取为 $[L, R]$ 的一个子区间即可：如果 $k \ge 0$，将整个 $[L, R]$ 都加上 $k$ 一定不劣；如果 $k < 0$，最优策略等价于不操作（或者选择空操作区间）。

```typst
#insight(title: "等价转化")[
  令每个位置在进入最优子段时可以处于三种阶段：尚未开启加 $k$（状态 $0$）、正在加 $k$（状态 $1$）、已结束加 $k$（状态 $2$）。
  因此只需维护以位置 $i$ 结尾的三态线性 DP 即可在单遍扫描中求出答案。
]
```

### 状态转移方程

```typst
#import "@preview/pinit:0.2.2": *

#block(width: 100%, inset: (top: 24pt, bottom: 24pt), {
  set par(first-line-indent: 0pt)
  align(center, text(size: 11pt)[
    $ f_1(i) = max( #pin(1) f_0(i - 1) #pin(2), #pin(3) f_1(i - 1) #pin(4) ) + #pin(5) a_i + k #pin(6) $
  ])
  pinit-highlight(1, 2, fill: rgb("#0B6989").transparentize(85%), radius: 3pt)
  pinit-highlight(3, 4, fill: rgb("#D97706").transparentize(84%), radius: 3pt)
  pinit-highlight(5, 6, fill: rgb("#059669").transparentize(84%), radius: 3pt)

  pinit-point-from((1, 2), pin-dy: -11pt, offset-dx: -16pt, offset-dy: -22pt,
    body-dx: -42pt, body-dy: -14pt, fill: rgb("#0B6989"))[
    #text(size: 8.5pt, fill: rgb("#0B6989"), weight: "bold")[从位置 $i$ 开启区间加 $k$]
  ]
  pinit-point-from((3, 4), pin-dy: 9pt, offset-dx: -12pt, offset-dy: 18pt,
    body-dx: -42pt, body-dy: 3pt, fill: rgb("#B45309"))[
    #text(size: 8.5pt, fill: rgb("#B45309"), weight: "bold")[延续当前加 $k$ 区间]
  ]
  pinit-point-from((5, 6), pin-dy: 9pt, offset-dx: 18pt, offset-dy: 18pt,
    body-dx: -2pt, body-dy: 3pt, fill: rgb("#047857"))[
    #text(size: 8.5pt, fill: rgb("#047857"), weight: "bold")[本位贡献]
  ]
})
```

### 参考代码

```cpp
#include <algorithm>
#include <iostream>

int main() {
    std::ios::sync_with_stdio(false);
    std::cin.tie(nullptr);

    int n;
    long long k;
    std::cin >> n >> k;

    long long dp0 = 0, dp1 = 0, dp2 = 0, ans = 0;
    for (int i = 0; i < n; ++i) {
        long long a;
        std::cin >> a;
        long long n0 = std::max(0LL, dp0 + a);
        long long n1 = std::max(dp0, dp1) + a + k;
        long long n2 = std::max(dp1, dp2) + a;
        dp0 = n0;
        dp1 = std::max(0LL, n1);
        dp2 = std::max(0LL, n2);
        ans = std::max({ans, dp0, dp1, dp2});
    }
    std::cout << ans << '\n';
    return 0;
}
```

```typst
#complexity($cal(O)(n)$, $cal(O)(1)$)
```
"##,
    ),
    (
        "contests/welcome-round/contest.toml",
        r#"name = "XCPC Workstation 示例赛"
location = "XCPC Workstation"
date = "2026 年 11 月"
language = "chinese"
cover = true

[[problem]]
label = "A"
id = "typst-showcase"

[[problem]]
label = "B"
id = "lantern-walk"
"#,
    ),
    (
        "contests/oi-mock/contest.toml",
        r#"name = "2026 CCF 非专业级软件能力认证（示例）"
location = "CSP-S 模拟第二轮认证 提高级"
date = "2026 年 11 月 22 日 14:30 ~ 18:30"
language = "chinese"
template = "cnoi-classic"
cover = true

[[problem]]
label = "A"
id = "lantern-walk"
[problem.limits]
time = "1.0 秒"
memory = "512 MiB"
input = "lantern.in"
output = "lantern.out"

[[problem]]
label = "B"
id = "typst-showcase"
[problem.limits]
time = "2.0 秒"
memory = "512 MiB"
input = "showcase.in"
output = "showcase.out"
"#,
    ),
];

/// Initializes an XCPC workspace at `dir` and populates it with the embedded example
/// contests (`welcome-round` in `olymp-classic`, `oi-mock` in `cnoi-classic`),
/// `.xcpc/preamble.typ`, and two problems (`typst-showcase`, `lantern-walk`).
///
/// Existing files are left untouched so re-running is safe and idempotent.
pub fn init_example_workspace(dir: impl AsRef<Path>) -> Result<Workspace> {
    init_example_workspace_with_template(dir, None)
}

/// Initializes the built-in example workspace at `dir` with an optional default
/// `template` written to `workspace.toml` (e.g. `"olymp-classic"` or `"cnoi-classic"`).
pub fn init_example_workspace_with_template(
    dir: impl AsRef<Path>,
    template: Option<&str>,
) -> Result<Workspace> {
    let ws = init_workspace_with_template(dir, template)?;
    for &(rel, content) in EXAMPLE_FILES {
        let abs = ws.resolve(rel)?;
        if fs::symlink_metadata(&abs).is_err() {
            ws.write(rel, content.as_bytes())?;
        }
    }
    Ok(ws)
}
