// Hand-written equivalent of contest.md + problems/*.md, standing in for the
// Markdown → Typst conversion until Phase 1. Plain TeX delimiters are escaped (`\(`)
// so Typst does not scale them; only \left…\right becomes `lr()`.
#import "/xcpc/olymp-classic.typ": contest, problem, section, samples

#show: contest.with(
  name: "第一届 XCPC 工作站测试赛",
  location: "上海",
  date: "2026 年 11 月 20 日",
  author: "出题人",
  language: "chinese",
)

#problem(label: "A", title: "A + B Problem", input: "标准输入", output: "标准输出", time-limit: "1 秒", memory-limit: "256 MB")[
  给定两个整数 $a$ 和 $b$，计算它们的和。这是一道签到题，请注意 $a$ 和 $b$ 可能为负数。

  #section("input")

  一行两个整数 $a,b$，满足 $-10^9 <= a,b <= 10^9$。

  #section("output")

  输出一个整数，表示 $a+b$。

  #samples(("1 2", "3"), ("-100 -93", "-193"))

  #section("note")

  在第一组样例中，$1+2=3$。
]

#problem(label: "B", title: "猜数字", input: "标准输入", output: "标准输出", time-limit: "2 秒", memory-limit: "512 MB")[
  这是一道交互题。交互库在 $\[1,n\]$ 中选定了一个整数 $x$，你需要通过不超过 $#sym.ceil.l log_2 n #sym.ceil.r + 1$ 次询问找出它。

  每次询问你可以给出一个整数 $y$，交互库会告诉你：

  - 若 $x<y$，返回 `<`；
  - 若 $x>y$，返回 `>`；
  - 若 $x=y$，返回 `=`，此时你需要立即结束程序。

  询问次数的上界满足
  $ q <= #sym.ceil.l log_2 n #sym.ceil.r + 1 . $

  #section("interaction")

  首先读入一个整数 $n$（$1 <= n <= 10^18$）。

  之后每次输出一行 `? y` 进行询问，并读入一个字符作为回答。每次输出后请刷新缓冲区。

  #section("output")

  当回答为 `=` 时结束程序，不需要额外输出。

  #samples(("10\n>\n<\n=", "? 5\n? 8\n? 7"))
]

#let c-input = ("24",) + range(1, 25).map(i => str(i) + " " + str(calc.rem(i * 37, 101)))
#let c-output = range(1, 25).map(i => str(calc.rem(i * 37, 101) + i))

#problem(label: "C", title: "很长的样例", input: "标准输入", output: "标准输出", time-limit: "1 秒", memory-limit: "256 MB")[
  给定 $n$ 组整数对 $\(a_i,b_i\)$，对每一组输出 $a_i+b_i$。本题的样例较长，用于检验样例表在页面上的排布。

  #section("input")

  第一行一个整数 $n$（$1 <= n <= 10^5$）。

  接下来 $n$ 行，每行两个整数 $a_i,b_i$（$0 <= a_i,b_i <= 10^9$）。

  #section("output")

  输出 $n$ 行，第 $i$ 行一个整数表示 $a_i+b_i$。

  #samples((c-input.join("\n"), c-output.join("\n")))

  #section("note")

  注意答案可能超过 $2^31-1$，请使用 64 位整数。
]
