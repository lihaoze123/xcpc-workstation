#!/usr/bin/env python3
"""Compare a rendered PDF against an olymp reference PDF, page by page.

    scripts/compare-pdf.py reference.pdf ours.pdf [--page N] [--words] [--png DIR]

For every page it reports:
  * word placement: words are matched in reading order by text; prints the median and
    worst offsets (in bp) and, with --words, every matched word;
  * horizontal rules: wide dark lines found in a 720 dpi raster (header/footer rules,
    sample tables);
  * a pixel score: the share of differing pixels in a 100 dpi grayscale raster.

Needs poppler-utils (pdftotext, pdftoppm, pdfinfo); run inside `nix develop`.
Python standard library only.
"""

import argparse
import difflib
import re
import statistics
import subprocess
import sys
import tempfile
import unicodedata
from pathlib import Path

WORD = re.compile(
    r'<word xMin="([\d.]+)" yMin="([\d.]+)" xMax="([\d.]+)" yMax="([\d.]+)">(.*?)</word>'
)


def pages(pdf: Path) -> int:
    out = subprocess.run(["pdfinfo", str(pdf)], capture_output=True, text=True, check=True).stdout
    return int(re.search(r"^Pages:\s+(\d+)", out, re.M).group(1))


def words(pdf: Path, page: int):
    out = subprocess.run(
        ["pdftotext", "-f", str(page), "-l", str(page), "-bbox", str(pdf), "-"],
        capture_output=True, text=True, check=True,
    ).stdout
    result = []
    for m in WORD.finditer(out):
        x0, y0, x1, y1, text = m.groups()
        text = text.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">")
        # Typst's math letters extract as Mathematical Alphanumerics (𝑎); TeX's as ASCII.
        text = unicodedata.normalize("NFKC", text)
        result.append((text, float(x0), float(y0), float(x1), float(y1)))
    return result


def raster(pdf: Path, page: int, dpi: int, tmp: Path):
    stem = tmp / f"{pdf.stem}-{page}-{dpi}"
    subprocess.run(
        ["pdftoppm", "-f", str(page), "-l", str(page), "-r", str(dpi), "-gray", "-singlefile", str(pdf), str(stem)],
        check=True,
    )
    data = (stem.parent / (stem.name + ".pgm")).read_bytes()
    # P5\n<w> <h>\n<max>\n<bytes>
    header, rest = data.split(b"\n", 1)
    dims, rest = rest.split(b"\n", 1)
    _maxval, pixels = rest.split(b"\n", 1)
    w, h = map(int, dims.split())
    return w, h, pixels


def rules(pdf: Path, page: int, tmp: Path, dpi: int = 720):
    w, h, px = raster(pdf, page, dpi, tmp)
    scale = 72 / dpi
    found = []
    run = None
    for y in range(h):
        row = px[y * w:(y + 1) * w]
        # A rule: a long run of dark pixels (at least 30% of the page width).
        best, start, cur = (0, 0), None, 0
        for x in range(w):
            if row[x] < 128:
                if start is None:
                    start = x
                cur = x - start + 1
                if cur > best[0]:
                    best = (cur, start)
            else:
                start = None
        if best[0] > 0.3 * w:
            if run and run[1] == y - 1 and abs(run[2] - best[1]) < 3:
                run[1] = y
            else:
                run = [y, y, best[1], best[1] + best[0]]
                found.append(run)
    return [(r[0] * scale, (r[1] + 1) * scale, r[2] * scale, r[3] * scale) for r in found]


def pixel_score(ref: Path, ours: Path, page: int, tmp: Path) -> float:
    w1, h1, a = raster(ref, page, 100, tmp)
    w2, h2, b = raster(ours, page, 100, tmp)
    if (w1, h1) != (w2, h2):
        return float("nan")
    diff = sum(1 for i in range(len(a)) if abs(a[i] - b[i]) > 64)
    ink = sum(1 for i in range(len(a)) if a[i] < 192) or 1
    return diff / ink


def compare_page(ref: Path, ours: Path, page: int, show_words: bool, tmp: Path):
    rw, ow = words(ref, page), words(ours, page)
    matcher = difflib.SequenceMatcher(a=[w[0] for w in rw], b=[w[0] for w in ow], autojunk=False)
    pairs = []
    for block in matcher.get_matching_blocks():
        for k in range(block.size):
            pairs.append((rw[block.a + k], ow[block.b + k]))
    print(f"== page {page}: {len(rw)} reference words, {len(ow)} ours, {len(pairs)} matched")
    if pairs:
        dx = [o[1] - r[1] for r, o in pairs]
        dy = [o[2] - r[2] for r, o in pairs]
        worst = max(pairs, key=lambda p: abs(p[1][1] - p[0][1]) + abs(p[1][2] - p[0][2]))
        off = sum(1 for r, o in pairs if abs(o[1] - r[1]) > 1 or abs(o[2] - r[2]) > 1)
        print(f"   words: {off}/{len(pairs)} off by >1bp; median dx={statistics.median(dx):+.2f} dy={statistics.median(dy):+.2f}; "
              f"worst '{worst[0][0]}' dx={worst[1][1]-worst[0][1]:+.2f} dy={worst[1][2]-worst[0][2]:+.2f}")
        if show_words:
            for r, o in pairs:
                print(f"   {r[0][:24]:<24} ref ({r[1]:7.2f},{r[2]:7.2f})  dx={o[1]-r[1]:+7.2f} dy={o[2]-r[2]:+7.2f}")
    missing = [w[0] for w in rw if w[0] not in {p[0][0] for p in pairs}]
    extra = [w[0] for w in ow if w[0] not in {p[1][0] for p in pairs}]
    if missing:
        print(f"   only in reference: {' '.join(missing[:20])}")
    if extra:
        print(f"   only in ours:      {' '.join(extra[:20])}")
    rr, orr = rules(ref, page, tmp), rules(ours, page, tmp)
    print(f"   rules: reference {len(rr)}, ours {len(orr)}")
    for i in range(max(len(rr), len(orr))):
        a = rr[i] if i < len(rr) else None
        b = orr[i] if i < len(orr) else None
        fa = f"y={a[0]:7.2f} x={a[2]:6.2f}..{a[3]:6.2f}" if a else "-"
        fb = f"y={b[0]:7.2f} x={b[2]:6.2f}..{b[3]:6.2f}" if b else "-"
        delta = f"dy={b[0]-a[0]:+.2f}" if a and b else ""
        print(f"     ref {fa:<32} ours {fb:<32} {delta}")
    print(f"   pixels differing / ink: {pixel_score(ref, ours, page, tmp):.3f}")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("reference", type=Path)
    ap.add_argument("ours", type=Path)
    ap.add_argument("--page", type=int)
    ap.add_argument("--words", action="store_true")
    args = ap.parse_args()
    n_ref, n_ours = pages(args.reference), pages(args.ours)
    if n_ref != n_ours:
        print(f"page count differs: reference {n_ref}, ours {n_ours}")
    with tempfile.TemporaryDirectory() as tmp:
        for page in ([args.page] if args.page else range(1, min(n_ref, n_ours) + 1)):
            compare_page(args.reference, args.ours, page, args.words, Path(tmp))


if __name__ == "__main__":
    sys.exit(main())
