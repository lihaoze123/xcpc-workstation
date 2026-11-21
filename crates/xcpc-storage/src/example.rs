//! Built-in example workspace embedded in the binary for `xcpc init --example` and
//! `Preview.initExampleWorkspace` / `Preview.pickAndInitExampleWorkspace`.
//!
//! Contains two curated problems (`lantern-walk`, `star-grid`) and one contest
//! (`welcome-round`), written from a competitive programmer's perspective and naturally
//! showcasing live preview, `Ctrl+点击` reverse locate, and raw `typst` diagram blocks.

use std::fs;
use std::path::Path;

use crate::{Result, Workspace, init_workspace};

/// Static files seeded into an example workspace (relative to workspace root).
const EXAMPLE_FILES: &[(&str, &str)] = &[
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
#align(center, block(inset: (y: 4pt), {
  let node(label, val, lit: false) = stack(
    dir: ttb,
    spacing: 4pt,
    align(center, box(
      width: 28pt,
      height: 28pt,
      radius: 14pt,
      stroke: if lit { 1.2pt + rgb("#222222") } else { 0.6pt + rgb("#888888") },
      fill: if lit { rgb("#f4f4f4") } else { white },
      align(center + horizon, text(size: 10pt, weight: if lit { "bold" } else { "regular" }, val)),
    )),
    align(center, text(size: 8.5pt, fill: rgb("#666666"), label)),
  )
  grid(
    columns: 5,
    column-gutter: 18pt,
    node("1", "-5"),
    node("2", "-1", lit: true),
    node("3", "4", lit: true),
    node("4", "-2", lit: true),
    node("5", "-5"),
  )
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

<!-- 提示：直接修改此处任意内容即可实时预览排版；在预览纸面上按住 Ctrl 点击任意文字或公式可跳回源码；上方的示意图使用 typst 代码块绘制。 -->
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

于是问题等价于：将每个元素替换成 $a_i + k$ 后，求新数组的最大连续子段和，再与 $0$ 取较大值。

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

时间复杂度 $\mathcal{O}(n)$，空间复杂度 $\mathcal{O}(1)$。
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
#align(center, block(inset: (y: 4pt), {
  table(
    columns: (28pt, 28pt, 28pt),
    rows: (24pt, 24pt),
    align: center + horizon,
    stroke: 0.5pt + rgb("#666666"),
    fill: (x, y) => if (x == 0 and y == 0) or (x == 0 and y == 1) or (x == 1 and y == 1) or (x == 2 and y == 1) { rgb("#f0f0f0") } else { white },
    [3], [1], [2],
    [4], [5], [1],
  )
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
];

/// Initializes an XCPC workspace at `dir` and populates it with the embedded example
/// contest (`welcome-round`) and two problems (`lantern-walk`, `star-grid`).
///
/// Existing files are left untouched so re-running is safe and idempotent.
pub fn init_example_workspace(dir: impl AsRef<Path>) -> Result<Workspace> {
    let ws = init_workspace(dir)?;
    for &(rel, content) in EXAMPLE_FILES {
        let abs = ws.resolve(rel)?;
        if fs::symlink_metadata(&abs).is_err() {
            ws.write(rel, content.as_bytes())?;
        }
    }
    Ok(ws)
}
