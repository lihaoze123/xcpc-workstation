//! Language-neutral C++ sources, generators, validators, interactors, checkers,
//! self-tests, and sample I/O shared between the Chinese (`example.rs`) and
//! English (`example_en.rs`) built-in example workspaces.

pub(crate) const SHARED_PACKAGE_FILES: &[(&str, &str)] = &[
    // =========================================================================
    // 1. interval-sum (Standard Traditional Polygon Package)
    // =========================================================================
    (
        "problems/interval-sum/samples/01.in",
        "5 3\n1 2 3 4 5\n1 3\n2 5\n1 5\n",
    ),
    ("problems/interval-sum/samples/01.ans", "6\n14\n15\n"),
    (
        "problems/interval-sum/tests/validator/valid/01.in",
        "3 2\n-5 10 0\n1 2\n2 3\n",
    ),
    (
        "problems/interval-sum/tests/validator/invalid/01.in",
        "3 1\n1 2 3\n3 2\n",
    ),
    (
        "problems/interval-sum/validator.cpp",
        r##"#include "testlib.h"

int main(int argc, char* argv[]) {
    registerValidation(argc, argv);
    int n = inf.readInt(1, 100000, "n");
    inf.readSpace();
    int q = inf.readInt(1, 100000, "q");
    inf.readEoln();
    inf.readInts(n, -1000000000, 1000000000, "a");
    inf.readEoln();
    for (int i = 0; i < q; ++i) {
        int l = inf.readInt(1, n, "l");
        inf.readSpace();
        inf.readInt(l, n, "r");
        inf.readEoln();
    }
    inf.readEof();
    return 0;
}
"##,
    ),
    (
        "problems/interval-sum/generators/gen_random.cpp",
        r##"#include "testlib.h"
#include <iostream>

int main(int argc, char* argv[]) {
    registerGen(argc, argv, 1);
    (void)opt<int>("seed", 1);
    int n = opt<int>("n", 10);
    int q = opt<int>("q", 10);
    int max_v = opt<int>("max_v", 1000000000);

    std::cout << n << ' ' << q << '\n';
    for (int i = 1; i <= n; ++i) {
        if (i > 1) std::cout << ' ';
        std::cout << rnd.next(-max_v, max_v);
    }
    std::cout << '\n';
    for (int i = 0; i < q; ++i) {
        int l = rnd.next(1, n);
        int r = rnd.next(1, n);
        if (l > r) std::swap(l, r);
        std::cout << l << ' ' << r << '\n';
    }
    return 0;
}
"##,
    ),
    (
        "problems/interval-sum/solutions/std.cpp",
        r##"#include <iostream>
#include <vector>
using namespace std;

int main() {
    ios::sync_with_stdio(false);
    cin.tie(nullptr);
    int n, q;
    if (!(cin >> n >> q)) return 0;
    vector<long long> pref(n + 1, 0);
    for (int i = 1; i <= n; ++i) {
        long long x;
        cin >> x;
        pref[i] = pref[i - 1] + x;
    }
    while (q--) {
        int l, r;
        cin >> l >> r;
        cout << pref[r] - pref[l - 1] << '\n';
    }
    return 0;
}
"##,
    ),
    (
        "problems/interval-sum/solutions/alt_fenwick.cpp",
        r##"#include <iostream>
#include <vector>
using namespace std;

int main() {
    ios::sync_with_stdio(false);
    cin.tie(nullptr);
    int n, q;
    if (!(cin >> n >> q)) return 0;
    vector<long long> bit(n + 1, 0);
    auto add = [&](int i, long long v) {
        for (; i <= n; i += i & -i) bit[i] += v;
    };
    auto sum = [&](int i) {
        long long res = 0;
        for (; i > 0; i -= i & -i) res += bit[i];
        return res;
    };
    for (int i = 1; i <= n; ++i) {
        long long x;
        cin >> x;
        add(i, x);
    }
    while (q--) {
        int l, r;
        cin >> l >> r;
        cout << sum(r) - sum(l - 1) << '\n';
    }
    return 0;
}
"##,
    ),
    (
        "problems/interval-sum/solutions/brute.cpp",
        r##"#include <iostream>
#include <vector>
using namespace std;

int main() {
    ios::sync_with_stdio(false);
    cin.tie(nullptr);
    int n, q;
    if (!(cin >> n >> q)) return 0;
    vector<long long> a(n + 1);
    for (int i = 1; i <= n; ++i) cin >> a[i];
    while (q--) {
        int l, r;
        cin >> l >> r;
        long long s = 0;
        for (int i = l; i <= r; ++i) s += a[i];
        cout << s << '\n';
    }
    return 0;
}
"##,
    ),
    (
        "problems/interval-sum/solutions/wa_int32.cpp",
        r##"#include <iostream>
#include <vector>
using namespace std;

int main() {
    ios::sync_with_stdio(false);
    cin.tie(nullptr);
    int n, q;
    if (!(cin >> n >> q)) return 0;
    vector<int> pref(n + 1, 0);
    for (int i = 1; i <= n; ++i) {
        int x;
        cin >> x;
        pref[i] = pref[i - 1] + x; // intentional 32-bit overflow
    }
    while (q--) {
        int l, r;
        cin >> l >> r;
        cout << pref[r] - pref[l - 1] << '\n';
    }
    return 0;
}
"##,
    ),
    // =========================================================================
    // 2. hidden-peak (Interactive + OI Subtasks)
    // =========================================================================
    (
        "problems/hidden-peak/tests/validator/valid/1/01.in",
        "5 20\n1 4 9 6 2\n",
    ),
    (
        "problems/hidden-peak/tests/validator/invalid/1/01.in",
        "25 20\n1 2 3 4 5 6 7 8 9 10 11 12 13 12 11 10 9 8 7 6 5 4 3 2 1\n",
    ),
    (
        "problems/hidden-peak/tests/validator/valid/2/01.in",
        "5 40\n-10 0 100 50 10\n",
    ),
    (
        "problems/hidden-peak/tests/validator/invalid/2/01.in",
        "5 40\n1 2 3 4 5\n",
    ),
    (
        "problems/hidden-peak/validator.cpp",
        r##"#include "testlib.h"
#include <vector>

int main(int argc, char* argv[]) {
    registerValidation(argc, argv);
    int max_n = (validator.group() == "1") ? 20 : 100000;
    int n = inf.readInt(3, max_n, "n");
    inf.readSpace();
    int q = inf.readInt(1, 1000, "Q");
    inf.readEoln();
    std::vector<int> a = inf.readInts(n, -1000000000, 1000000000, "a");
    inf.readEoln();
    inf.readEof();

    int peak = -1;
    for (int i = 1; i + 1 < n; ++i) {
        if (a[i - 1] < a[i] && a[i] > a[i + 1]) {
            ensuref(peak == -1, "multiple peaks found");
            peak = i;
        }
    }
    ensuref(peak != -1, "no valid interior peak");
    for (int i = 0; i < peak; ++i) ensuref(a[i] < a[i + 1], "not strictly increasing");
    for (int i = peak; i + 1 < n; ++i) ensuref(a[i] > a[i + 1], "not strictly decreasing");
    ensuref(q >= 20, "Q too small");
    return 0;
}
"##,
    ),
    (
        "problems/hidden-peak/interactor.cpp",
        r##"#include "testlib.h"
#include <iostream>
#include <string>
#include <vector>

int main(int argc, char* argv[]) {
    registerInteraction(argc, argv);
    int n = inf.readInt();
    int max_q = inf.readInt();
    std::vector<int> a(n + 1);
    int peak = 1;
    for (int i = 1; i <= n; ++i) {
        a[i] = inf.readInt();
        if (a[i] > a[peak]) peak = i;
    }

    std::cout << n << " " << max_q << std::endl;

    int used = 0;
    while (true) {
        std::string op = ouf.readToken("?|!");
        if (op == "?") {
            int idx = ouf.readInt(1, n, "i");
            ++used;
            if (used > max_q) {
                std::cout << -1 << std::endl;
                quitf(_wa, "query limit exceeded: used %d > %d", used, max_q);
            }
            std::cout << a[idx] << std::endl;
        } else {
            int ans = ouf.readInt(1, n, "p");
            if (ans != peak) {
                quitf(_wa, "wrong peak: expected %d, got %d (queries=%d)", peak, ans, used);
            }
            quitf(_ok, "found peak %d in %d/%d queries", peak, used, max_q);
        }
    }
}
"##,
    ),
    (
        "problems/hidden-peak/generators/gen_peak.cpp",
        r##"#include "testlib.h"
#include <iostream>
#include <vector>

int main(int argc, char* argv[]) {
    registerGen(argc, argv, 1);
    (void)opt<int>("seed", 1);
    int n = opt<int>("n", 10);
    int peak = has_opt("peak") ? opt<int>("peak") : rnd.next(2, n - 1);
    int q = (n <= 20) ? 20 : 40;

    std::vector<int> a(n + 1, 0);
    int cur = rnd.next(-1000, 1000);
    for (int i = 1; i <= peak; ++i) {
        cur += rnd.next(1, 50);
        a[i] = cur;
    }
    for (int i = peak + 1; i <= n; ++i) {
        cur -= rnd.next(1, 50);
        a[i] = cur;
    }

    std::cout << n << " " << q << "\n";
    for (int i = 1; i <= n; ++i) {
        if (i > 1) std::cout << ' ';
        std::cout << a[i];
    }
    std::cout << "\n";
    return 0;
}
"##,
    ),
    (
        "problems/hidden-peak/solutions/std.cpp",
        r##"#include <iostream>
using namespace std;

int ask(int i) {
    cout << "? " << i << endl;
    int v;
    if (!(cin >> v)) exit(0);
    return v;
}

int main() {
    int n, q;
    if (!(cin >> n >> q)) return 0;
    int l = 1, r = n;
    while (l < r) {
        int mid = (l + r) / 2;
        if (ask(mid) < ask(mid + 1)) l = mid + 1;
        else r = mid;
    }
    cout << "! " << l << endl;
    return 0;
}
"##,
    ),
    (
        "problems/hidden-peak/solutions/alt_ternary.cpp",
        r##"#include <iostream>
#include <unordered_map>
using namespace std;

unordered_map<int, int> memo;
int ask(int i) {
    auto it = memo.find(i);
    if (it != memo.end()) return it->second;
    cout << "? " << i << endl;
    int v;
    if (!(cin >> v)) exit(0);
    return memo[i] = v;
}

int main() {
    int n, q;
    if (!(cin >> n >> q)) return 0;
    int l = 2, r = n - 1;
    while (l < r) {
        int mid = (l + r) / 2;
        if (ask(mid) < ask(mid + 1)) l = mid + 1;
        else r = mid;
    }
    cout << "! " << l << endl;
    return 0;
}
"##,
    ),
    (
        "problems/hidden-peak/solutions/brute_linear.cpp",
        r##"#include <iostream>
using namespace std;

int main() {
    int n, q;
    if (!(cin >> n >> q)) return 0;
    int best_i = 1, best_v = -2000000000;
    for (int i = 1; i <= n; ++i) {
        cout << "? " << i << endl;
        int v;
        if (!(cin >> v)) return 0;
        if (v > best_v) {
            best_v = v;
            best_i = i;
        }
    }
    cout << "! " << best_i << endl;
    return 0;
}
"##,
    ),
    (
        "problems/hidden-peak/solutions/deadlock_no_flush.cpp",
        r##"#include <iostream>
using namespace std;

int main() {
    ios::sync_with_stdio(false);
    cin.tie(nullptr);
    int n, q;
    if (!(cin >> n >> q)) return 0;
    // Intentional deadlock: cin.tie(nullptr) + '\n' never flushes -> ILE
    cout << "? 1\n";
    int v;
    cin >> v;
    cout << "! 2" << endl;
    return 0;
}
"##,
    ),
    // =========================================================================
    // 3. secret-relay (stdio 2-Run Communication, runs = 2)
    // =========================================================================
    (
        "problems/secret-relay/tests/validator/valid/01.in",
        "8 3\n10110010\n",
    ),
    (
        "problems/secret-relay/tests/validator/invalid/01.in",
        "4 0\n1021\n",
    ),
    (
        "problems/secret-relay/validator.cpp",
        r##"#include "testlib.h"
#include <string>

int main(int argc, char* argv[]) {
    registerValidation(argc, argv);
    int n = inf.readInt(1, 64, "n");
    inf.readSpace();
    int flip_pos = inf.readInt(0, 72, "flip_pos");
    inf.readEoln();
    std::string s = inf.readToken("[01]{1,64}", "s");
    inf.readEoln();
    inf.readEof();
    ensuref((int)s.size() == n, "string length mismatch");
    ensuref(flip_pos >= 0, "flip_pos negative");
    return 0;
}
"##,
    ),
    (
        "problems/secret-relay/interactor.cpp",
        r##"#include "testlib.h"
#include <iostream>
#include <string>

int main(int argc, char* argv[]) {
    registerInteraction(argc, argv);
    std::string first = inf.readToken();
    if (first != "PHASE2") {
        int n = std::stoi(first);
        int flip_pos = inf.readInt(0, 72);
        std::string s = inf.readToken("[01]+");

        std::cout << "encode\n" << n << " " << s << std::endl;

        std::string code = ouf.readToken("[01]+", "encoded");
        if ((int)code.size() < n || (int)code.size() > n + 8) {
            quitf(_wa, "pass 1: invalid code length %d (allowed [%d, %d])",
                  (int)code.size(), n, n + 8);
        }

        std::string corrupted = code;
        if (flip_pos > 0) {
            int idx = (flip_pos - 1) % (int)corrupted.size();
            corrupted[idx] = (corrupted[idx] == '0') ? '1' : '0';
        }

        tout << "PHASE2 " << n << " " << s << " " << corrupted << "\n";
        quitf(_ok, "pass 1 encoded len=%d, flipped=%d", (int)code.size(), flip_pos);
    } else {
        int n = inf.readInt(1, 64);
        std::string s = inf.readToken("[01]+");
        std::string corrupted = inf.readToken("[01]+");

        std::cout << "decode\n" << n << " " << corrupted << std::endl;

        std::string decoded = ouf.readToken("[01]+", "decoded");
        if (decoded != s) {
            quitf(_wa, "pass 2: decoded mismatch (expected %s, got %s)",
                  s.c_str(), decoded.c_str());
        }
        quitf(_ok, "pass 2 decoded %d bits accurately", n);
    }
}
"##,
    ),
    (
        "problems/secret-relay/generators/gen_msg.cpp",
        r##"#include "testlib.h"
#include <iostream>
#include <string>

int main(int argc, char* argv[]) {
    registerGen(argc, argv, 1);
    (void)opt<int>("seed", 1);
    int n = opt<int>("n", 16);
    int flip_pos = has_opt("flip") ? opt<int>("flip") : rnd.next(1, n + 7);
    std::string s;
    s.reserve(n);
    for (int i = 0; i < n; ++i) s.push_back(rnd.next(0, 1) ? '1' : '0');
    std::cout << n << " " << flip_pos << "\n" << s << "\n";
    return 0;
}
"##,
    ),
    (
        "problems/secret-relay/solutions/std.cpp",
        r##"#include <iostream>
#include <string>
#include <vector>
using namespace std;

static bool is_pow2(int x) { return x > 0 && (x & (x - 1)) == 0; }

int main() {
    string mode;
    if (!(cin >> mode)) return 0;
    if (mode == "encode") {
        int n;
        string s;
        cin >> n >> s;
        int m = 0, placed = 0;
        while (placed < n) {
            ++m;
            if (!is_pow2(m)) ++placed;
        }
        vector<int> bits(m + 1, 0);
        int idx = 0;
        for (int pos = 1; pos <= m; ++pos) {
            if (!is_pow2(pos)) bits[pos] = s[idx++] - '0';
        }
        for (int p = 1; p <= m; p <<= 1) {
            int xr = 0;
            for (int pos = 1; pos <= m; ++pos) {
                if (pos != p && (pos & p)) xr ^= bits[pos];
            }
            bits[p] = xr;
        }
        string out;
        out.reserve(m);
        for (int pos = 1; pos <= m; ++pos) out.push_back(bits[pos] ? '1' : '0');
        cout << out << endl;
    } else {
        int n;
        string c;
        cin >> n >> c;
        int m = (int)c.size();
        vector<int> bits(m + 1, 0);
        for (int pos = 1; pos <= m; ++pos) bits[pos] = c[pos - 1] - '0';
        int syndrome = 0;
        for (int p = 1; p <= m; p <<= 1) {
            int xr = 0;
            for (int pos = 1; pos <= m; ++pos) {
                if (pos & p) xr ^= bits[pos];
            }
            if (xr) syndrome |= p;
        }
        if (syndrome >= 1 && syndrome <= m) bits[syndrome] ^= 1;
        string s;
        s.reserve(n);
        for (int pos = 1; pos <= m && (int)s.size() < n; ++pos) {
            if (!is_pow2(pos)) s.push_back(bits[pos] ? '1' : '0');
        }
        cout << s << endl;
    }
    return 0;
}
"##,
    ),
    (
        "problems/secret-relay/solutions/alt_hamming.cpp",
        r##"#include <iostream>
#include <string>
#include <vector>
using namespace std;

static bool is_pow2(int x) { return (x & (x - 1)) == 0; }

int main() {
    string mode;
    if (!(cin >> mode)) return 0;
    if (mode == "encode") {
        int n;
        string s;
        cin >> n >> s;
        int m = 0, placed = 0;
        while (placed < n) {
            ++m;
            if (!is_pow2(m)) ++placed;
        }
        vector<int> a(m + 1, 0);
        int j = n - 1;
        for (int i = 1; i <= m; ++i) {
            if (!is_pow2(i)) a[i] = s[j--] - '0';
        }
        for (int p = 1; p <= m; p <<= 1) {
            int x = 0;
            for (int i = 1; i <= m; ++i) if (i != p && (i & p)) x ^= a[i];
            a[p] = x;
        }
        for (int i = 1; i <= m; ++i) cout << a[i];
        cout << endl;
    } else {
        int n;
        string c;
        cin >> n >> c;
        int m = (int)c.size();
        vector<int> a(m + 1, 0);
        for (int i = 1; i <= m; ++i) a[i] = c[i - 1] - '0';
        int err = 0;
        for (int p = 1; p <= m; p <<= 1) {
            int x = 0;
            for (int i = 1; i <= m; ++i) if (i & p) x ^= a[i];
            if (x) err |= p;
        }
        if (err >= 1 && err <= m) a[err] ^= 1;
        string rev;
        for (int i = 1; i <= m && (int)rev.size() < n; ++i) {
            if (!is_pow2(i)) rev.push_back(a[i] ? '1' : '0');
        }
        string s(rev.rbegin(), rev.rend());
        cout << s << endl;
    }
    return 0;
}
"##,
    ),
    (
        "problems/secret-relay/solutions/wa_raw.cpp",
        r##"#include <iostream>
#include <string>
using namespace std;

int main() {
    string mode;
    int n;
    string s;
    if (!(cin >> mode >> n >> s)) return 0;
    cout << s << endl;
    return 0;
}
"##,
    ),
    (
        "problems/secret-relay/solutions/ile_hang.cpp",
        r##"#include <iostream>
#include <string>
using namespace std;

int main() {
    string mode;
    int n;
    string s;
    if (!(cin >> mode >> n >> s)) return 0;
    string never;
    cin >> never;
    cout << s << endl;
    return 0;
}
"##,
    ),
    // =========================================================================
    // 4. weighted-walk (OI Subtasks + Custom Checker with quitp Partial Score)
    // =========================================================================
    ("problems/weighted-walk/samples/01.in", "5\n"),
    ("problems/weighted-walk/samples/01.ans", "15\n"),
    (
        "problems/weighted-walk/tests/validator/valid/1/01.in",
        "8\n",
    ),
    (
        "problems/weighted-walk/tests/validator/invalid/1/01.in",
        "50\n",
    ),
    (
        "problems/weighted-walk/tests/validator/valid/2/01.in",
        "1000\n",
    ),
    (
        "problems/weighted-walk/tests/validator/invalid/2/01.in",
        "0\n",
    ),
    ("problems/weighted-walk/tests/checker/ok-1/in", "5\n"),
    ("problems/weighted-walk/tests/checker/ok-1/out", "15\n"),
    ("problems/weighted-walk/tests/checker/ok-1/ans", "15\n"),
    ("problems/weighted-walk/tests/checker/wa-1/in", "5\n"),
    ("problems/weighted-walk/tests/checker/wa-1/out", "9\n"),
    ("problems/weighted-walk/tests/checker/wa-1/ans", "15\n"),
    (
        "problems/weighted-walk/validator.cpp",
        r##"#include "testlib.h"

int main(int argc, char* argv[]) {
    registerValidation(argc, argv);
    int max_n = (validator.group() == "1") ? 10 : 100000;
    inf.readInt(1, max_n, "n");
    inf.readEoln();
    inf.readEof();
    return 0;
}
"##,
    ),
    (
        "problems/weighted-walk/checker.cpp",
        r##"#include "testlib.h"
#include <cstdlib>

int main(int argc, char* argv[]) {
    registerTestlibCmd(argc, argv);
    long long ja = ans.readLong();
    long long pa = ouf.readLong();
    if (pa == ja) quitf(_ok, "exact optimal value %lld", ja);
    if (std::llabs(pa - ja) == 1) {
        quitp(0.5, "within 1 of optimal (%lld vs %lld) -> 50%% partial", pa, ja);
    }
    quitf(_wa, "expected %lld, found %lld", ja, pa);
}
"##,
    ),
    (
        "problems/weighted-walk/generators/gen_n.cpp",
        r##"#include "testlib.h"
#include <iostream>

int main(int argc, char* argv[]) {
    registerGen(argc, argv, 1);
    (void)opt<int>("seed", 1);
    int lo = opt<int>("lo", 1);
    int hi = opt<int>("hi", 10);
    std::cout << rnd.next(lo, hi) << "\n";
    return 0;
}
"##,
    ),
    (
        "problems/weighted-walk/solutions/std.cpp",
        r##"#include <iostream>
using namespace std;

int main() {
    long long n;
    if (cin >> n) cout << n * (n + 1) / 2 << '\n';
    return 0;
}
"##,
    ),
    (
        "problems/weighted-walk/solutions/alt_loop.cpp",
        r##"#include <iostream>
using namespace std;

int main() {
    long long n;
    if (!(cin >> n)) return 0;
    long long s = 0;
    for (long long i = 1; i <= n; ++i) s += i;
    cout << s << '\n';
    return 0;
}
"##,
    ),
    (
        "problems/weighted-walk/solutions/brute.cpp",
        r##"#include <iostream>
using namespace std;

int main() {
    long long n;
    if (!(cin >> n)) return 0;
    if (n <= 10) cout << n * (n + 1) / 2 << '\n';
    else cout << 0 << '\n';
    return 0;
}
"##,
    ),
    (
        "problems/weighted-walk/solutions/approx_half.cpp",
        r##"#include <iostream>
using namespace std;

int main() {
    long long n;
    if (!(cin >> n)) return 0;
    if (n <= 10) cout << n * (n + 1) / 2 << '\n';
    else cout << n * (n + 1) / 2 - 1 << '\n'; // triggers quitp(0.5) on subtask 2
    return 0;
}
"##,
    ),
    // =========================================================================
    // 5. Shared samples for lightweight bank showcase problems
    // =========================================================================
    (
        "problems/bipartite-coloring/samples/01.in",
        "4 4\n1 2\n2 3\n3 4\n4 1\n",
    ),
    (
        "problems/bipartite-coloring/samples/01.ans",
        "YES\n1 2 1 2\n",
    ),
    (
        "problems/flow-network-cut/samples/01.in",
        "4 5\n1 2 3\n1 3 2\n2 3 1\n2 4 2\n3 4 4\n",
    ),
    ("problems/flow-network-cut/samples/01.ans", "5\n"),
    (
        "problems/suffix-automaton-lcp/samples/01.in",
        "ababa\n2\n1 3\n2 4\n",
    ),
    ("problems/suffix-automaton-lcp/samples/01.ans", "3\n2\n"),
    ("problems/bracket-balance/samples/01.in", "4\n))((\n"),
    ("problems/bracket-balance/samples/01.ans", "2\n"),
    (
        "problems/sliding-window-median/samples/01.in",
        "5 3\n1 3 2 5 4\n",
    ),
    ("problems/sliding-window-median/samples/01.ans", "2 3 4\n"),
];
