// `xcpc build contest contest-zh --emit-typst` of tests/fixtures/olymp-ref/contest-zh/workspace.
#import "/templates/olymp-classic.typ": *
#import "/xcpc/markdown.typ": m, dm

#show: contest.with(name: "第一届 XCPC 工作站测试赛", location: "上海", date: "2026 年 11 月 20 日", author: "出题人", language: "chinese", show-cover: true, blank-pages: false)

#problem(label: "A", title: "A + B Problem", input: none, output: none, time-limit: "1 秒", memory-limit: "256 MB", show-metadata: true)[
给定两个整数 #m("a"); 和 #m("b");，计算它们的和。这是一道签到题，请注意 #m("a"); 和 #m("b"); 可能为负数。

#section("input")

一行两个整数 #m("a \\,b");，满足 #m("- 1 0 ^(9 )<=  a \\,b <=  1 0 ^(9 )");。

#section("output")

输出一个整数，表示 #m("a + b");。

#samples(("1 2\n", "3\n"), ("-100 -93\n", "-193\n"),)

#section("note")

在第一组样例中，#m("1 + 2 = 3");。
]

#problem(label: "B", title: "猜数字", input: none, output: none, time-limit: "2 秒", memory-limit: "512 MB", show-metadata: true)[
这是一道交互题。交互库在 #m("\\[1 \\,n \\]"); 中选定了一个整数 #m("x");，你需要通过不超过 #m("⌈  log _(2 ) n  ⌉  +  1"); 次询问找出它。

每次询问你可以给出一个整数 #m("y");，交互库会告诉你：

#list([若 #m("x < y");，返回 #raw("<");；], [若 #m("x > y");，返回 #raw(">");；], [若 #m("x = y");，返回 #raw("=");，此时你需要立即结束程序。],)

询问次数的上界满足 #dm("q  <=  lr(⌈  log _(2 ) n  ⌉ ) +  1  .");

#section("interaction")

首先读入一个整数 #m("n");（#m("1 <=  n <=  1 0 ^(1 8 )");）。

之后每次输出一行 #raw("? y"); 进行询问，并读入一个字符作为回答。每次输出后请刷新缓冲区。

#section("output")

当回答为 #raw("="); 时结束程序，不需要额外输出。

#samples(("10\n>\n<\n=\n", "? 5\n? 8\n? 7\n"),)
]

#problem(label: "C", title: "很长的样例", input: none, output: none, time-limit: "1 秒", memory-limit: "256 MB", show-metadata: true)[
给定 #m("n"); 组整数对 #m("\\(a _(i )\\,b _(i )\\)");，对每一组输出 #m("a _(i )+ b _(i )");。本题的样例较长，用于检验样例表在页面上的排布。

#section("input")

第一行一个整数 #m("n");（#m("1 <=  n <=  1 0 ^(5 )");）。

接下来 #m("n"); 行，每行两个整数 #m("a _(i )\\,b _(i )");（#m("0 <=  a _(i )\\,b _(i )<=  1 0 ^(9 )");）。

#section("output")

输出 #m("n"); 行，第 #m("i"); 行一个整数表示 #m("a _(i )+ b _(i )");。

#samples(("24\n1 37\n2 74\n3 10\n4 47\n5 84\n6 20\n7 57\n8 94\n9 30\n10 67\n11 3\n12 40\n13 77\n14 13\n15 50\n16 87\n17 23\n18 60\n19 97\n20 33\n21 70\n22 6\n23 43\n24 80\n", "38\n76\n13\n51\n89\n26\n64\n102\n39\n77\n14\n52\n90\n27\n65\n103\n40\n78\n116\n53\n91\n28\n66\n104\n"),)

#section("note")

注意答案可能超过 #m("2 ^(3 1 )- 1");，请使用 64 位整数。
]

