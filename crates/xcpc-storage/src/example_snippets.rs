//! Curated Phase 5 algorithm snippet seeds (`modint`, `ntt`, `dsu`, `number-theory-facts`)
//! and sample TRD (`trds/icpc-regional/trd.toml`) for `xcpc init --example`.

/// Shared C++ code and verification files (language-independent).
pub(crate) const SHARED_SNIPPET_CODE_FILES: &[(&str, &str)] = &[
    // ---- modint ----
    (
        "snippets/modint/code.hpp",
        r#"#pragma once
#include <cstdint>
#include <iostream>

template <uint32_t MOD = 998244353>
struct ModInt {
    uint32_t v;
    ModInt(int64_t x = 0) : v((x % int64_t(MOD) + MOD) % MOD) {}
    ModInt operator-() const { return ModInt(v ? MOD - v : 0); }
    ModInt& operator+=(ModInt o) {
        if ((v += o.v) >= MOD) v -= MOD;
        return *this;
    }
    ModInt& operator-=(ModInt o) {
        if ((v += MOD - o.v) >= MOD) v -= MOD;
        return *this;
    }
    ModInt& operator*=(ModInt o) {
        v = uint32_t(uint64_t(v) * o.v % MOD);
        return *this;
    }
    ModInt pow(uint64_t e) const {
        ModInt r = 1, b = *this;
        for (; e; e >>= 1, b *= b)
            if (e & 1) r *= b;
        return r;
    }
    ModInt inv() const { return pow(MOD - 2); }
    ModInt& operator/=(ModInt o) { return *this *= o.inv(); }
    friend ModInt operator+(ModInt a, ModInt b) { return a += b; }
    friend ModInt operator-(ModInt a, ModInt b) { return a -= b; }
    friend ModInt operator*(ModInt a, ModInt b) { return a *= b; }
    friend ModInt operator/(ModInt a, ModInt b) { return a /= b; }
    friend bool operator==(ModInt a, ModInt b) { return a.v == b.v; }
    friend std::ostream& operator<<(std::ostream& os, ModInt a) { return os << a.v; }
};
using mint = ModInt<998244353>;
"#,
    ),
    (
        "snippets/modint/test.cpp",
        r#"#include "modint/code.hpp"
#include <cassert>

int main() {
    mint a = 123456789, b = 987654321;
    assert((a + b) - b == a);
    assert((a * b) / b == a);
    assert(mint(-1).v == 998244352);
    assert(mint(3).pow(10).v == 59049);
    assert((mint(7) * mint(7).inv()).v == 1);
    return 0;
}
"#,
    ),
    // ---- ntt (depends on modint) ----
    (
        "snippets/ntt/code.hpp",
        r#"#pragma once
#include "modint/code.hpp"
#include <algorithm>
#include <vector>

inline void ntt(std::vector<mint>& a, bool inv) {
    int n = int(a.size());
    for (int i = 1, j = 0; i < n; ++i) {
        int bit = n >> 1;
        for (; j & bit; bit >>= 1) j ^= bit;
        j ^= bit;
        if (i < j) std::swap(a[i], a[j]);
    }
    for (int len = 2; len <= n; len <<= 1) {
        mint wlen = mint(3).pow((998244353 - 1) / len);
        if (inv) wlen = wlen.inv();
        for (int i = 0; i < n; i += len) {
            mint w = 1;
            for (int j = 0; j < len / 2; ++j) {
                mint u = a[i + j], v = a[i + j + len / 2] * w;
                a[i + j] = u + v;
                a[i + j + len / 2] = u - v;
                w *= wlen;
            }
        }
    }
    if (inv) {
        mint inv_n = mint(n).inv();
        for (mint& x : a) x *= inv_n;
    }
}

inline std::vector<mint> convolution(std::vector<mint> a, std::vector<mint> b) {
    if (a.empty() || b.empty()) return {};
    int sz = int(a.size() + b.size()) - 1;
    int n = 1;
    while (n < sz) n <<= 1;
    a.resize(n);
    b.resize(n);
    ntt(a, false);
    ntt(b, false);
    for (int i = 0; i < n; ++i) a[i] *= b[i];
    ntt(a, true);
    a.resize(sz);
    return a;
}
"#,
    ),
    (
        "snippets/ntt/test.cpp",
        r#"#include "ntt/code.hpp"
#include <cassert>

int main() {
    std::vector<mint> a = {1, 2, 3, 4};
    std::vector<mint> b = {5, 6, 7};
    auto c = convolution(a, b);
    assert(c.size() == 6);
    std::vector<uint32_t> expected = {5, 16, 34, 52, 45, 28};
    for (size_t i = 0; i < expected.size(); ++i) {
        assert(c[i].v == expected[i]);
    }
    return 0;
}
"#,
    ),
    (
        "snippets/ntt/verify_conv.cpp",
        r#"#include "ntt/code.hpp"
#include <iostream>
#include <vector>

int main() {
    std::ios::sync_with_stdio(false);
    std::cin.tie(nullptr);
    int n, m;
    if (!(std::cin >> n >> m)) return 0;
    std::vector<mint> a(n), b(m);
    for (int i = 0; i < n; ++i) {
        int64_t x;
        std::cin >> x;
        a[i] = x;
    }
    for (int i = 0; i < m; ++i) {
        int64_t x;
        std::cin >> x;
        b[i] = x;
    }
    auto c = convolution(a, b);
    for (size_t i = 0; i < c.size(); ++i) {
        if (i) std::cout << ' ';
        std::cout << c[i].v;
    }
    std::cout << '\n';
    return 0;
}
"#,
    ),
    // ---- dsu ----
    (
        "snippets/dsu/code.hpp",
        r#"#pragma once
#include <numeric>
#include <utility>
#include <vector>

struct DSU {
    std::vector<int> p, sz;
    explicit DSU(int n = 0) : p(n), sz(n, 1) {
        std::iota(p.begin(), p.end(), 0);
    }
    int find(int x) {
        while (p[x] != x) x = p[x] = p[p[x]];
        return x;
    }
    bool unite(int a, int b) {
        a = find(a);
        b = find(b);
        if (a == b) return false;
        if (sz[a] < sz[b]) std::swap(a, b);
        p[b] = a;
        sz[a] += sz[b];
        return true;
    }
    bool same(int a, int b) { return find(a) == find(b); }
    int size(int x) { return sz[find(x)]; }
};
"#,
    ),
    (
        "snippets/dsu/test.cpp",
        r#"#include "dsu/code.hpp"
#include <cassert>

int main() {
    DSU d(5);
    assert(!d.same(0, 1));
    assert(d.unite(0, 1));
    assert(!d.unite(0, 1));
    assert(d.unite(1, 2));
    assert(d.same(0, 2));
    assert(d.size(2) == 3);
    assert(d.size(4) == 1);
    return 0;
}
"#,
    ),
];

/// Chinese metadata, notes, and sample TRD for `init_example_workspace`.
pub(crate) const EXAMPLE_SNIPPET_FILES_ZH: &[(&str, &str)] = &[
    (
        "snippets/modint/snippet.toml",
        r#"title = "自动取模整数（ModInt）"
tags = ["math", "modint"]
time_complexity = "O(1) / O(log MOD)"
space_complexity = "O(1)"
description = "编译期固定质数模数的轻量取模整数，默认 998244353"

[[verify]]
kind = "self"
file = "test.cpp"
"#,
    ),
    (
        "snippets/modint/notes.md",
        r#"支持加、减、乘、快速幂与费马小定理求逆元（要求 `MOD` 为质数）。

- 负数构造自动规约到 $[0, \mathrm{MOD})$。
- 若需动态模数或合数模数，请改用 Barrett 约减与扩展欧几里得求逆。
"#,
    ),
    (
        "snippets/ntt/snippet.toml",
        r#"title = "快速数论变换（NTT）"
tags = ["poly", "math", "ntt"]
time_complexity = "O(n log n)"
space_complexity = "O(n)"
description = "998244353 模数下的迭代 Cooley-Tukey 多项式卷积"

[[verify]]
kind = "self"
file = "test.cpp"

[[verify]]
kind = "library_checker"
problem = "convolution_mod"
adapter = "verify_conv.cpp"
"#,
    ),
    (
        "snippets/ntt/notes.md",
        r#"模数 $998244353 = 119 \times 2^{23} + 1$，原根 $g = 3$，最大支持长度 $2^{23}$。

- 依赖 `modint/code.hpp`，展开提交时自动按拓扑序内联。
- 传入空向量时直接返回空向量。
"#,
    ),
    (
        "snippets/dsu/snippet.toml",
        r#"title = "并查集（DSU）"
tags = ["data-structures", "graph"]
time_complexity = "O(α(n))"
space_complexity = "O(n)"
description = "路径减半 + 按子树大小合并的简洁并查集"

[[verify]]
kind = "self"
file = "test.cpp"
"#,
    ),
    (
        "snippets/dsu/notes.md",
        r#"采用迭代路径减半（`x = p[x] = p[p[x]]`）与按大小启发式合并，无递归爆栈风险。
"#,
    ),
    (
        "snippets/number-theory-facts/snippet.toml",
        r#"title = "数论常用质数与原根速查"
tags = ["math", "cheatsheet"]
description = "纯公式/结论卡片（无代码），收录常用 NTT 模数、原根与容斥结论"
"#,
    ),
    (
        "snippets/number-theory-facts/notes.md",
        r#"### 常用 NTT 模数与原根

| 模数 $P$ | 分解 $c \cdot 2^k + 1$ | 最小原根 $g$ |
| :--- | :--- | :--- |
| $998244353$ | $119 \times 2^{23} + 1$ | $3$ |
| $167772161$ | $5 \times 2^{25} + 1$ | $3$ |
| $469762049$ | $7 \times 2^{26} + 1$ | $3$ |
| $1004535809$ | $479 \times 2^{21} + 1$ | $3$ |

### 欧拉定理与扩展欧拉定理

当 $b \ge \varphi(m)$ 时：

$$a^b \equiv a^{(b \bmod \varphi(m)) + \varphi(m)} \pmod m$$

### 莫比乌斯反演

$$[n = 1] = \sum_{d \mid n} \mu(d), \qquad \varphi(n) = \sum_{d \mid n} \mu(d)\frac{n}{d}$$
"#,
    ),
    (
        "trds/icpc-regional/trd.toml",
        r#"name = "2026 ICPC 区域赛模板册"
author = "XCPC Workstation 示例队"
date = "2026 年 11 月"
template = "trd-compact"
frozen = false

[template-options.trd-compact]
columns = 3
hash-interval = 10
strip-comments = false

[[chapter]]
title = "数学与多项式"
items = ["modint", "ntt", "number-theory-facts"]

[[chapter]]
title = "数据结构"
items = ["dsu"]
"#,
    ),
];

/// English metadata, notes, and sample TRD for `init_example_workspace_with_options(..., Language::English)`.
pub(crate) const EXAMPLE_SNIPPET_FILES_EN: &[(&str, &str)] = &[
    (
        "snippets/modint/snippet.toml",
        r#"title = "Modular Integer (ModInt)"
tags = ["math", "modint"]
time_complexity = "O(1) / O(log MOD)"
space_complexity = "O(1)"
description = "Lightweight compile-time prime modulus integer, defaulting to 998244353"

[[verify]]
kind = "self"
file = "test.cpp"
"#,
    ),
    (
        "snippets/modint/notes.md",
        r#"Supports addition, subtraction, multiplication, binary exponentiation, and Fermat inverse (requires prime `MOD`).

- Negative inputs are normalized into $[0, \mathrm{MOD})$.
- For runtime or composite moduli, switch to Barrett reduction and extended GCD.
"#,
    ),
    (
        "snippets/ntt/snippet.toml",
        r#"title = "Number Theoretic Transform (NTT)"
tags = ["poly", "math", "ntt"]
time_complexity = "O(n log n)"
space_complexity = "O(n)"
description = "Iterative Cooley-Tukey polynomial convolution modulo 998244353"

[[verify]]
kind = "self"
file = "test.cpp"

[[verify]]
kind = "library_checker"
problem = "convolution_mod"
adapter = "verify_conv.cpp"
"#,
    ),
    (
        "snippets/ntt/notes.md",
        r#"Modulus $998244353 = 119 \times 2^{23} + 1$ with primitive root $g = 3$, supporting lengths up to $2^{23}$.

- Depends on `modint/code.hpp`; automatically inlined in topological order when bundled.
"#,
    ),
    (
        "snippets/dsu/snippet.toml",
        r#"title = "Disjoint Set Union (DSU)"
tags = ["data-structures", "graph"]
time_complexity = "O(α(n))"
space_complexity = "O(n)"
description = "Compact DSU with path halving and union by size"

[[verify]]
kind = "self"
file = "test.cpp"
"#,
    ),
    (
        "snippets/dsu/notes.md",
        r#"Uses iterative path halving (`x = p[x] = p[p[x]]`) and union by size — zero recursion overhead.
"#,
    ),
    (
        "snippets/number-theory-facts/snippet.toml",
        r#"title = "Number Theory Primes & Primitive Roots"
tags = ["math", "cheatsheet"]
description = "Pure formula/reference card (no code) with NTT primes and identities"
"#,
    ),
    (
        "snippets/number-theory-facts/notes.md",
        r#"### Common NTT Primes & Primitive Roots

| Prime $P$ | Factorization $c \cdot 2^k + 1$ | Primitive Root $g$ |
| :--- | :--- | :--- |
| $998244353$ | $119 \times 2^{23} + 1$ | $3$ |
| $167772161$ | $5 \times 2^{25} + 1$ | $3$ |
| $469762049$ | $7 \times 2^{26} + 1$ | $3$ |
| $1004535809$ | $479 \times 2^{21} + 1$ | $3$ |

### Extended Euler's Theorem

For $b \ge \varphi(m)$:

$$a^b \equiv a^{(b \bmod \varphi(m)) + \varphi(m)} \pmod m$$

### Möbius Inversion

$$[n = 1] = \sum_{d \mid n} \mu(d), \qquad \varphi(n) = \sum_{d \mid n} \mu(d)\frac{n}{d}$$
"#,
    ),
    (
        "trds/icpc-regional/trd.toml",
        r#"name = "2026 ICPC Regional Team Reference Document"
author = "XCPC Workstation Sample Team"
date = "November 2026"
template = "trd-compact"
frozen = false

[template-options.trd-compact]
columns = 3
hash-interval = 10
strip-comments = false

[[chapter]]
title = "Math & Polynomials"
items = ["modint", "ntt", "number-theory-facts"]

[[chapter]]
title = "Data Structures"
items = ["dsu"]
"#,
    ),
];
