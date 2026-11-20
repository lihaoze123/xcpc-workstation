## 二分答案

若最小距离能达到 $D$，则一定也能达到任何 $D' < D$，所以答案具有单调性，可以二分 $D$。二分的范围是 $[0, L]$，共 $O(\log L)$ 轮。

## 判定

给定 $D$，从左到右贪心：第一盏挂在 $x_1$，之后每次挂在第一个与上一盏距离不小于 $D$ 的挂钩上。若能挂满 $k$ 盏，则 $D$ 可行。

### 引理

设 $g_1 < g_2 < \cdots$ 是贪心选出的位置，$p_1 < p_2 < \cdots < p_k$ 是任意一个可行方案，则对每个 $i$ 都有 $g_i \le p_i$。

### 证明

对 $i$ 归纳。$i = 1$ 时 $g_1 = x_1 \le p_1$。若 $g_{i-1} \le p_{i-1}$，则 $p_i \ge p_{i-1} + D \ge g_{i-1} + D$，而 $g_i$ 是满足 $x \ge g_{i-1} + D$ 的最小挂钩，故 $g_i \le p_i$。

由引理，存在可行方案时贪心一定能挂满 $k$ 盏。总复杂度为
$$
O(n \log L).
$$

> 常见错误：二分的上界写成 $\max x_i$ 而不是 $x_n - x_1$，在 $x_1 > 0$ 时会多做一轮；不影响正确性，但值得留意。

## 参考实现

```cpp
#include <bits/stdc++.h>
using namespace std;

int main() {
    int n, k;
    long long L;
    cin >> n >> k >> L;
    vector<long long> x(n);
    for (auto &v : x) cin >> v;

    // 能否让相邻两盏的距离都不小于 d
    auto ok = [&](long long d) {
        int cnt = 1;
        long long last = x[0];
        for (int i = 1; i < n; i++)
            if (x[i] - last >= d) cnt++, last = x[i];
        return cnt >= k;
    };

    long long lo = 0, hi = L;
    while (lo < hi) {
        long long mid = (lo + hi + 1) / 2;
        if (ok(mid)) lo = mid; else hi = mid - 1;
    }
    cout << lo << '\n';
}
```
