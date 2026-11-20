Polycarp keeps $n$ cups of yogurt in a row; the $i$-th cup has flavour $a_i$. He wants to split the row into the minimum number of contiguous groups so that no group contains two cups of the same flavour, because mixing identical flavours is boring.

Formally, you have to find the smallest $k$ such that the sequence can be cut into $k$ segments, and for every segment the following quantity is equal to its length:

$$
\left|\{a_l, a_{l+1}, \ldots, a_r\}\right| = r - l + 1.
$$

Note that a single cup always forms a valid group. Some useful facts:

- a greedy cut is optimal;
- each flavour is remembered with its last position;
- the answer never exceeds $n$.

## Input

The first line contains an integer $n$ ($1 \le n \le 2 \cdot 10^5$). The second line contains $n$ integers $a_1, a_2, \ldots, a_n$ ($1 \le a_i \le 10^9$).

## Output

Print one integer: the minimum number of groups.

## Explanation

In the first example the groups are $[1, 2]$ and $[1, 3, 2]$.
