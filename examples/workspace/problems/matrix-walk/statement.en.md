You are given an $n \times m$ grid. Each cell $(i, j)$ contains a non-negative integer $a_{i,j}$. A *walk* starts at $(1, 1d ends at $(n, m)$, moving only **right** or **down**.

The *beauty* of a walk is the bitwise XOR of all numbers on it. Count the walks whose beauty equals $k$, modulo $998\,244\,353$:
$$
\#\left\{ P : \bigoplus_{(i,j) \in P} a_{i,j} = k \right\} \bmod 998\,244\,353.
$$

## Input

The first line contains three integers $n$, $m$ and $k$ ($1 \le n, m \le 20$, $0 \le k < 2^{30}$).

Each of the next $n$ lines contains $m$ integers $a_{i,j}$ ($0 \le a_{i,j} < 2^{30}$).

## Output

Print one integer: the number of walks, modulo $998\,244\,353$.

## Note

A walk visits exactly $n + m - 1$ cells, so there are $\binom{n + m - 2}{n - 1}$ walks in total, which is too many to enumerate when $n = m = 20$. Try *meet in the middle*.
