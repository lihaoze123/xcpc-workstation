---
title: Three-Source Automatic Verification
---

# Three-Source Automatic Verification

To ensure every algorithm implementation in your Team Reference Document is backed by real test runs rather than memory, XCPC Workstation provides a unified **three-source verification engine** supporting self-contained unit tests, local workspace problem tests, and direct integration with [Library Checker](https://judge.yosupo.jp/).

## Verification Sources

Declare one or more `[[verify]]` entries in `snippets/<slug>/snippet.toml`. The engine executes all configured sources sequentially and aggregates the results:

=== "`self` Self-Contained Test"

    Suitable for lightweight unit tests or self-contained random stress tests:

    ```toml
    [[verify]]
    kind = "self"
    file = "test.cpp"
    ```

    - `file` is a C++ path relative to `snippets/<slug>/` and can directly `#include "<slug>/code.hpp"`.
    - Compiled automatically with `-I <workspace>/snippets`. Exit code `0` passes (`AC`); on `assert()` failure, cached path prefixes are stripped to display a clean assertion location.

=== "`problem` Workspace Problem"

    Verifies the snippet directly against a real problem's test data and checker under `problems/<problem>/` in the same workspace:

    ```toml
    [[verify]]
    kind = "problem"
    problem = "my-poly-mul"
    adapter = "verify_local.cpp"
    ```

    - `adapter` is a thin wrapper under `snippets/<slug>/` that reads the problem input, invokes the snippet, and prints the output.
    - The engine prefers already-built test data in `build/problems/<problem>/tests/`. If the problem is a package whose tests are not built yet, it triggers a Debug build automatically; for statement-only problems, it falls back to `samples/`.
    - Automatically evaluates using the problem's configured checker (built-in or custom `checker.cpp`) and time/memory limits.

=== "`library_checker` Library Checker"

    Verifies the snippet against strong test data from `yosupo06/library-checker-problems`:

    ```toml
    [[verify]]
    kind = "library_checker"
    problem = "convolution_mod"      # Accepts problem name or "category/problem"
    adapter = "verify_conv.cpp"
    ```

    - **On-demand fetch and global cache**: On first run, only `info.toml`, `checker.cpp`, `sol/correct.cpp`, `gen/`, and `common/` are fetched and cached in `~/.cache/xcpc/library-checker/<problem>/` (overridable via `XCPC_CACHE_DIR`). Test data is generated locally with the native compiler and reused, making subsequent runs completely offline with zero network traffic.
    - **Standard limits**: Uses `timelimit` from `info.toml` and the standard Library Checker memory limit of `1024 MiB`.

## Library Checker Mirrors, Local Clones, and Offline Fallback

To work reliably in restricted networks or offline pre-contest environments, Library Checker problem resolution follows a strict priority order:

1. **Local cache hit**: If `info.toml`, `checker.cpp`, and `sol/correct.cpp` already exist in `~/.cache/xcpc/library-checker/<problem>/`, they are reused offline with no network request.
2. **Local Git clone**: If a local `library-checker-problems` clone is configured (under **Settings → Toolchain → Library Checker Source → Local Clone Directory**, or via the `XCPC_LC_REPO` environment variable), files are copied directly from the local repository into the cache.
3. **Remote HTTP fetch (with optional mirror)**: Fetches from GitHub Raw; configure **Mirror Prefix** in Settings or set `XCPC_LC_MIRROR` (e.g., `https://ghproxy.net/https://raw.githubusercontent.com`).

!!! note "Safe Offline Degradation (Never Corrupts Verified Status)"
    If a verification run involving `library_checker` is triggered while offline or experiencing network errors and the problem is not yet cached locally, that verification case returns `UNAVAILABLE` rather than `failed`. The engine **never** downgrades an already-`passed` snippet to `failed` due to network unavailability: if the content hash is unchanged, the existing `passed` status and disk cache remain intact; if the code changed, the status stays `stale`.

## Verification Cache and Status Lifecycle

After each verification run, results are written atomically as JSON to `.xcpc/snippet-verify/<slug>.json`.

### Content Hash Coverage

The engine computes a deterministic SHA-256 verification key (`content_hash`) covering all of the following inputs:

- The snippet's own `snippet.toml` and `code.hpp`
- `code.hpp` of **every transitive dependency**
- Every test/adapter source file referenced in `[[verify]]`
- For `kind = "problem"` entries, the linked problem's package hash (or sample contents)

### Four Derived Statuses

Reading snippet status requires zero compilation or execution—the engine statically compares the current `content_hash` against `.xcpc/snippet-verify/<slug>.json`:

| Status | Meaning |
| :--- | :--- |
| `passed` | All `[[verify]]` cases passed and the current content hash matches the cache |
| `stale` | Previously verified, but the snippet, any upstream dependency, a verification script, or linked problem data has changed |
| `failed` | The most recent run for the current content hash had at least one failing case (`WA` / `TLE` / `MLE` / `RE` / `CE` / `JE`) |
| `unverified` | Never verified, or no `[[verify]]` entries declared |

!!! tip "Automatic Staleness Propagation from Upstream Edits"
    Suppose `ntt` depends on `modint`. As soon as you edit `snippets/modint/code.hpp`, `ntt`'s `content_hash` changes immediately even without touching `ntt`, turning its status to `stale` so you know to re-run regression tests.

## CLI Commands and Cascading Regression

```bash
# Verify a single snippet (returns immediately from cache if content_hash is unchanged)
xcpc snippet verify modint

# Cascading regression: verify modint and all direct/transitive downstream dependents (e.g. ntt) in topological order
xcpc snippet verify modint --cascade

# Full library verification in upstream-first topological order
xcpc snippet verify --all

# Ignore cached results and force recompilation and re-execution
xcpc snippet verify ntt --force

# Control parallel jobs (forwarded to the underlying build engine)
xcpc snippet verify --all -j 4
```

| Flag | Description |
| :--- | :--- |
| `<slug>` | Target snippet slug (mutually exclusive with `--all`) |
| `--cascade` | Also verify all downstream snippets depending on `<slug>` in topological order |
| `--all` | Verify all snippets in the workspace in topological order |
| `--force` | Bypass cached results in `.xcpc/snippet-verify/<slug>.json` and re-run |
| `-j, --jobs <N>` | Parallel job count for underlying problem builds |

In the desktop **Snippets** space, click **Verify** in the snippet detail sidebar, or click **Verify & Downstream** in the editor's **Verify** tab to inspect per-source verdicts, passed case counts, and error details. To verify the entire library, run **Verify All Snippets** from the Command Palette.
