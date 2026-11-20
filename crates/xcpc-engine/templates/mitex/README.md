The Typst half of [MiTeX](https://github.com/mitex-rs/mitex) 0.2.4 (`packages/mitex/specs`,
Apache-2.0, see `LICENSE`): the scope that the Rust crate `mitex`'s output is evaluated in.
Bundled by `xcpc-engine` as `/xcpc/mitex/…`. Changes, marked `xcpc:` in `latex/standard.typ`:
a local `xarrow` instead of importing the `@preview/xarrow` package, and `\KaTeX` without
the symbol `kai`, which Typst 0.15 no longer has.
