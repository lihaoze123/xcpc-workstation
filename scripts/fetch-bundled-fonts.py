#!/usr/bin/env python3
"""Download and unpack the 24 OTF fonts required by `crates/xcpc-engine/build.rs`.

Used by CI / Release workflows on non-Nix runners (Ubuntu and Windows) so
`XCPC_FONT_DIRS` points to the exact Latin Modern + JetBrains Mono OTF set.
"""

from __future__ import annotations

import io
import os
import sys
import urllib.request
import zipfile

REQUIRED_FONTS = {
    "lmroman10-regular.otf",
    "lmroman10-bold.otf",
    "lmroman10-italic.otf",
    "lmroman10-bolditalic.otf",
    "lmroman12-regular.otf",
    "lmroman12-bold.otf",
    "lmroman12-italic.otf",
    "lmroman17-regular.otf",
    "lmsans10-regular.otf",
    "lmsans10-bold.otf",
    "lmsans10-oblique.otf",
    "lmsans10-boldoblique.otf",
    "lmsans12-regular.otf",
    "lmsans12-oblique.otf",
    "lmsans17-regular.otf",
    "lmsans17-oblique.otf",
    "lmmono10-regular.otf",
    "lmmono10-italic.otf",
    "lmmono12-regular.otf",
    "latinmodern-math.otf",
    "JetBrainsMono-Regular.ttf",
    "JetBrainsMono-Italic.ttf",
    "JetBrainsMono-Bold.ttf",
    "JetBrainsMono-BoldItalic.ttf",
}

ARCHIVES = [
    (
        "https://mirrors.ctan.org/fonts/lm.zip",
        "https://ctan.math.illinois.edu/fonts/lm.zip",
    ),
    (
        "https://mirrors.ctan.org/fonts/lm-math.zip",
        "https://www.gust.org.pl/projects/e-foundry/lm-math/download/latinmodern-math-1959.zip",
    ),
    (
        "https://github.com/JetBrains/JetBrainsMono/releases/download/v2.304/JetBrainsMono-2.304.zip",
    ),
]


def download_first(urls: tuple[str, ...]) -> bytes:
    last_err: Exception | None = None
    for url in urls:
        print(f"Downloading {url} ...")
        req = urllib.request.Request(url, headers={"User-Agent": "xcpc-workstation-ci/1.0"})
        try:
            with urllib.request.urlopen(req, timeout=120) as resp:
                return resp.read()
        except Exception as exc:
            print(f"warning: failed to download {url}: {exc}", file=sys.stderr)
            last_err = exc
    raise RuntimeError(f"all mirrors failed for {urls}: {last_err}")


def missing_fonts(out_dir: str) -> set[str]:
    return {
        name for name in REQUIRED_FONTS if not os.path.isfile(os.path.join(out_dir, name))
    }


def main() -> int:
    if len(sys.argv) != 2:
        print(f"Usage: {sys.argv[0]} <out_dir>", file=sys.stderr)
        return 2

    out_dir = os.path.abspath(sys.argv[1])
    os.makedirs(out_dir, exist_ok=True)

    missing = missing_fonts(out_dir)
    if not missing:
        print(f"All {len(REQUIRED_FONTS)} bundled fonts already present in {out_dir}")
        return 0

    for urls in ARCHIVES:
        if not missing:
            break
        data = download_first(urls)
        with zipfile.ZipFile(io.BytesIO(data)) as zf:
            for member in zf.infolist():
                if member.is_dir():
                    continue
                base = os.path.basename(member.filename)
                if base in missing:
                    dst = os.path.join(out_dir, base)
                    with zf.open(member) as src, open(dst, "wb") as out:
                        out.write(src.read())
                    missing.remove(base)

    still_missing = missing_fonts(out_dir)
    if still_missing:
        print(
            f"error: missing required fonts after download: {sorted(still_missing)}",
            file=sys.stderr,
        )
        return 1

    print(f"Prepared {len(REQUIRED_FONTS)} bundled fonts in {out_dir}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
