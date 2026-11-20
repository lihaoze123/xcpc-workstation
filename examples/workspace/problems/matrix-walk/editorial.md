## Meet in the middle

Every walk crosses the anti-diagonal $i + j = \lfloor (n + m) / 2 \rfloor + 1$ exactly once. Enumerate the first halves from $(1, 1)$ and store, for each cell on the diagonal, a map from XOR value to count; then enumerate the second halves from $(n, m)$ backwards and look up $k \oplus x$.

Each half has at most $2^{19}$ paths, so the whole runs in $O(2^{(n+m)/2} \log)$.
