//! Built-in example workspace embedded in the binary for `xcpc init --example` and
//! `Preview.initExampleWorkspace` / `Preview.pickAndInitExampleWorkspace`.
//!
//! Contains two curated problems (`lantern-walk`, `star-grid`), a workspace-level
//! `.xcpc/preamble.typ` macro file, and two contests (`welcome-round` in `olymp-classic`
//! and `oi-mock` in `cnoi-classic`) live-referencing the same problem bank so new users
//! immediately see live preview, pure-Typst vector diagrams, preamble macros, and
//! multi-template publishing.

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
  text(size: 9.5pt)[
    *复杂度总结*：时间复杂度 #time，空间复杂度 #space。
  ],
)
"##,
    ),
    (
        "problems/lantern-walk/problem.toml",
        r#"title = "灯笼小径"
rating = 1500
tags = ["greedy", "prefix-sum", "constructive"]

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
// 纯内置 Typst 矢量绘制：展示操作区间 [l, r] = [2, 4] 与亮度增量 +k
#align(center, block(inset: (y: 4pt), {
  let step = 46pt
  let r = 13pt
  let vals = ("-5", "-1", "4", "-2", "-5")
  let after = ("-5", "+2", "+7", "+1", "-5")
  box(width: step * 4 + r * 2 + 24pt, height: 76pt, {
    // 顶部操作区间标注括号 [2, 4] + k
    let x1 = 12pt + r + step * 1
    let x2 = 12pt + r + step * 3
    place(top + left, dx: x1, dy: 14pt, line(length: x2 - x1, stroke: 0.9pt + rgb("#0B6989")))
    place(top + left, dx: x1, dy: 14pt, line(angle: 90deg, length: 5pt, stroke: 0.9pt + rgb("#0B6989")))
    place(top + left, dx: x2, dy: 14pt, line(angle: 90deg, length: 5pt, stroke: 0.9pt + rgb("#0B6989")))
    place(top + left, dx: (x1 + x2) / 2 - 34pt, dy: 0pt,
      box(width: 68pt, align(center, text(size: 8.5pt, fill: rgb("#0B6989"), weight: "bold")[操作区间 $+3$]))
    )
    // 连线
    for i in range(4) {
      let cx = 12pt + r + step * i
      place(top + left, dx: cx + r, dy: 24pt + r,
        line(length: step - r * 2, stroke: 0.6pt + rgb("#A1A1AA")))
    }
    // 灯笼节点与编号
    for i in range(5) {
      let lit = i >= 1 and i <= 3
      let cx = 12pt + step * i
      place(top + left, dx: cx, dy: 24pt, box(
        width: r * 2, height: r * 2, radius: r,
        stroke: if lit { 1.2pt + rgb("#0B6989") } else { 0.7pt + rgb("#A1A1AA") },
        fill: if lit { rgb("#EBF5F8") } else { white },
        align(center + horizon, text(size: 9.5pt, weight: if lit { "bold" } else { "regular" },
          fill: if lit { rgb("#09546D") } else { rgb("#27272A") }, vals.at(i))),
      ))
      place(top + left, dx: cx - 4pt, dy: 24pt + r * 2 + 4pt,
        box(width: r * 2 + 8pt, align(center, text(size: 8pt,
          fill: if lit { rgb("#0B6989") } else { rgb("#71717A") },
          if lit { after.at(i) } else { str(i + 1) }))))
    }
  })
}))
```

请你求出操作之后，整条小径上**最大连续子段和**的最大可能值。允许选取空子段，空子段的和视为 $0$。

## 输入格式

第一行包含两个整数 $n, k$（$1 \le n \le 2 \times 10^5$，$-10^9 \le k \le 10^9$），分别表示灯笼数量与单次调整的增量。

第二行包含 $n$ 个整数 $a_1, a_2, \ldots, a_n$（$-10^9 \le a_i \le 10^9$），表示每盏灯笼的初始亮度。

## 输出格式

输出一行一个整数，表示至多一次操作后能获得的最大连续子段和。

## 提示

在第一个样例中，选择区间 $[2, 4]$ 并将亮度同时加上 $k = 3$，亮度序列变为 $[-5, 2, 7, 1, -5]$，最大连续子段和为 $2 + 7 + 1 = 10$。

<!-- 提示：直接修改此处任意内容即可实时预览；在预览纸面上按住 Ctrl 点击文字或公式可跳回源码；上方示意图由纯 Typst 矢量原语绘制。 -->
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
        r#"## 思路分析

不妨先思考什么时候操作一定不会让答案变差。

- 若 $k \le 0$，给任意区间加上一个非正数都不会使最大连续子段和增大，因此最优策略是不做操作，直接求原数组的最大连续子段和（与 $0$ 取最大值）。
- 若 $k > 0$，最优方案一定可以把操作区间 $[l, r]$ 设为最终选出的最大子段本身：因为操作区间若超出所选子段，超出部分对答案没有贡献；若真包含于所选子段，把操作区间扩大到整个所选子段只会让和继续增加。

```typst
// 调用 .xcpc/preamble.typ 中定义的工作区自定义宏
#insight(title: "等价转化")[
  令 $b_i = a_i + max(0, k)$，原问题直接等价于求序列 $b$ 的最大连续子段和（允许空子段）。
]
```

## 参考实现

```cpp
#include <algorithm>
#include <iostream>

int main() {
    std::ios::sync_with_stdio(false);
    std::cin.tie(nullptr);

    int n;
    long long k;
    std::cin >> n >> k;
    long long add = std::max(0LL, k);
    long long cur = 0, ans = 0;
    for (int i = 0; i < n; ++i) {
        long long a;
        std::cin >> a;
        cur = std::max(0LL, cur + a + add);
        ans = std::max(ans, cur);
    }
    std::cout << ans << '\n';
    return 0;
}
```

```typst
#complexity($cal(O)(n)$, $cal(O)(1)$)
```
"#,
    ),
    (
        "problems/star-grid/problem.toml",
        r#"title = "星图折叠"
rating = 1900
tags = ["math", "dp", "grid"]

[limits]
time = "2s"
memory = "512MB"
"#,
    ),
    (
        "problems/star-grid/statement.md",
        r##"天文学家绘制了一张 $n \times m$ 的星图，第 $i$ 行第 $j$ 列的格点亮度为 $w_{i, j}$。

现在你需要从左上角 $(1, 1)$ 出发，每一步只能向右移动到 $(i, j+1)$ 或向下移动到 $(i+1, j)$，最终到达右下角 $(n, m)$。一条路径的权值定义为路径上经过的所有格点亮度之和：

$$S(P) = \sum_{(i, j) \in P} w_{i, j}$$

```typst
// 纯内置 Typst 绘制网格 + 最优路径高亮与方向指示
#align(center, block(inset: (y: 4pt), {
  let cw = 36pt
  let ch = 28pt
  let grid_vals = ((3, 1, 2), (4, 5, 1))
  let on_path = ((0, 0), (0, 1), (1, 1), (2, 1))
  box(width: cw * 3 + 22pt, height: ch * 2 + 18pt, {
    for c in range(3) {
      place(top + left, dx: 18pt + c * cw, dy: 0pt,
        box(width: cw, align(center, text(size: 8pt, fill: rgb("#71717A"), str(c + 1)))))
    }
    for r in range(2) {
      place(top + left, dx: 0pt, dy: 14pt + r * ch,
        box(width: 14pt, height: ch, align(right + horizon, text(size: 8pt, fill: rgb("#71717A"), str(r + 1)))))
      for c in range(3) {
        let hit = on_path.contains((c, r))
        place(top + left, dx: 18pt + c * cw, dy: 14pt + r * ch, box(
          width: cw, height: ch,
          stroke: if hit { 1pt + rgb("#0B6989") } else { 0.5pt + rgb("#A1A1AA") },
          fill: if hit { rgb("#EBF5F8") } else { white },
          align(center + horizon, text(size: 10pt, weight: if hit { "bold" } else { "regular" },
            fill: if hit { rgb("#09546D") } else { rgb("#27272A") }, str(grid_vals.at(r).at(c)))),
        ))
      }
    }
    // 路径方向指示箭头
    place(top + left, dx: 18pt + cw / 2 - 4pt, dy: 14pt + ch - 6pt,
      text(size: 9pt, fill: rgb("#0B6989"), weight: "bold")[↓])
    place(top + left, dx: 18pt + cw - 5pt, dy: 14pt + ch + ch / 2 - 5pt,
      text(size: 9pt, fill: rgb("#0B6989"), weight: "bold")[→])
    place(top + left, dx: 18pt + cw * 2 - 5pt, dy: 14pt + ch + ch / 2 - 5pt,
      text(size: 9pt, fill: rgb("#0B6989"), weight: "bold")[→])
  })
}))
```

请你计算从 $(1, 1)$ 到 $(n, m)$ 的所有合法路径中，路径权值的最大值。

## 输入格式

第一行包含两个正整数 $n, m$（$1 \le n, m \le 1000$）。

接下来 $n$ 行，每行包含 $m$ 个整数 $w_{i, 1}, w_{i, 2}, \ldots, w_{i, m}$（$-10^6 \le w_{i, j} \le 10^6$）。

## 输出格式

输出一行一个整数，表示最大路径权值。

## 提示

样例中的最优路径为 $(1, 1) \to (2, 1) \to (2, 2) \to (2, 3)$，路径权值为 $3 + 4 + 5 + 1 = 13$。

| 子任务 | 分值 | 约束 |
| :---: | :---: | :--- |
| 1 | 30 | $n, m \le 10$ |
| 2 | 30 | $w_{i, j} \ge 0$ |
| 3 | 40 | 无特殊约束 |
"##,
    ),
    ("problems/star-grid/samples/01.in", "2 3\n3 1 2\n4 5 1\n"),
    ("problems/star-grid/samples/01.ans", "13\n"),
    (
        "problems/star-grid/editorial.md",
        r#"## 思路分析

由于每一步只能向右或向下走，状态转移天然构成有向无环图。设 $f[i][j]$ 表示到达 $(i, j)$ 的最大路径权值，则有：

$$f[i][j] = w_{i, j} + \max(f[i - 1][j], f[i][j - 1])$$

边界处仅从合法前驱转移即可，使用一维滚动数组即可将空间压到 $\mathcal{O}(m)$。

## 参考实现

```cpp
#include <algorithm>
#include <iostream>
#include <limits>
#include <vector>

int main() {
    std::ios::sync_with_stdio(false);
    std::cin.tie(nullptr);

    int n, m;
    std::cin >> n >> m;
    const long long NEG = std::numeric_limits<long long>::min() / 4;
    std::vector<long long> dp(m + 1, NEG);
    for (int i = 1; i <= n; ++i) {
        for (int j = 1; j <= m; ++j) {
            long long w;
            std::cin >> w;
            if (i == 1 && j == 1) {
                dp[j] = w;
            } else {
                dp[j] = std::max(dp[j], dp[j - 1]) + w;
            }
        }
    }
    std::cout << dp[m] << '\n';
    return 0;
}
```

```typst
#complexity($cal(O)(n m)$, $cal(O)(m)$)
```
"#,
    ),
    (
        "contests/welcome-round/contest.toml",
        r#"name = "新生热身赛"
location = "XCPC Workstation"
date = "2026 年 11 月"
language = "chinese"
show-cover = true

[[problem]]
label = "A"
id = "lantern-walk"

[[problem]]
label = "B"
id = "star-grid"
"#,
    ),
    (
        "contests/oi-mock/contest.toml",
        r#"name = "秋季模拟赛（OI / CNOI 模板）"
location = "XCPC Workstation"
date = "2026 年 11 月"
language = "chinese"
template = "cnoi-classic"
show-cover = true

[[problem]]
label = "A"
id = "lantern-walk"

[[problem]]
label = "B"
id = "star-grid"
[problem.override.limits]
time = "1.5s"
"#,
    ),
];

/// Initializes an XCPC workspace at `dir` and populates it with the embedded example
/// contests (`welcome-round` in `olymp-classic`, `oi-mock` in `cnoi-classic`),
/// `.xcpc/preamble.typ`, and two problems (`lantern-walk`, `star-grid`).
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
