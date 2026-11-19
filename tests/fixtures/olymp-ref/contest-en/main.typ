// Generated with contest.md + problems/*.md by scripts/gen-olymp-fixtures.py.
#import "/xcpc/olymp-classic.typ": contest, problem, section, samples

#show: contest.with(
  name: "XCPC Workstation Open Cup",
  location: "Shanghai",
  date: "November 20, 2026",
  author: "Problem Setters",
  language: "english",
)

#problem(label: "A", title: "Yogurt Grouping", input: "standard input", output: "standard output", time-limit: "2 seconds", memory-limit: "256 megabytes")[
  Polycarp keeps $n$ cups of yogurt in a row; the $i$-th cup has flavour $a_i$. He wants to split the row into the minimum number of contiguous groups so that no group contains two cups of the same flavour, because mixing identical flavours is boring.

  Formally, you have to find the smallest $k$ such that the sequence can be cut into $k$ segments, and for every segment the following quantity is equal to its length:

  $ lr(|\\{a_l, a_(l+1), dots, a_r\\}|) = r - l + 1. $

  Note that a single cup always forms a valid group. Some useful facts:

  - a greedy cut is optimal;
  - each flavour is remembered with its last position;
  - the answer never exceeds $n$.

  #section("input")

  The first line contains an integer $n$ ($1 <= n <= 2 dot 10^5$). The second line contains $n$ integers $a_1, a_2, dots, a_n$ ($1 <= a_i <= 10^9$).

  #section("output")

  Print one integer: the minimum number of groups.

  #samples(("5\n1 2 1 3 2", "2"), ("3\n7 7 7", "3"),)

  #section("explanation")

  In the first example the groups are $\[1, 2\]$ and $\[1, 3, 2\]$.

]

#problem(label: "B", title: "Quiet Problem", input: "standard input", output: "standard output", time-limit: "1 second", memory-limit: "512 megabytes", show-metadata: false)[
  Read a single word and print it back in reverse order. This problem hides its limits table.

  #section("input")

  One line with a word of at most $100$ lowercase letters.

  #section("output")

  The reversed word.

  #samples(("typography", "yhpargopyt"),)

  #section("note")

  Long lines in samples are kept as they are.

]
