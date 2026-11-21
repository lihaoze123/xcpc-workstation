#!/usr/bin/env python3
"""Generate XCPC Workstation Icon v3 (minimalist) deliverables in assets/brand/.

Outputs:
- icon.svg                     Master 1024x1024 static SVG (SVG 1.1 filters only)
- icon-layered.svg             Master 1024x1024 with explicit independent layer groups
- layers/plate.svg             Superellipse matte pearl-to-silver plate layer
- layers/paper-back.svg        Back draft paper (unrotated; animate around 565,680 -> -20deg)
- layers/paper-mid.svg         Middle draft paper (unrotated; animate around 565,680 -> -10deg)
- layers/paper-top.svg         Top pure-white draft paper
- layers/pencil.svg            Geometric yellow pencil at final resting pose
- icon-32.svg                  Pixel-aligned simplified SVG for 32px / 24px (plate + 2 papers + pencil)
- icon-16.svg                  Pixel-aligned simplified SVG for 16px (plate + 1 paper + pencil silhouette)
- xcpc-workstation-symbolic.svg Linux GNOME symbolic monochrome 16x16 icon
- icon-{512,256,128,64,48,32,24,16}.png Rasterized PNGs via Inkscape
- icon.ico                     Windows multi-size ICO (16..256 PNG-in-ICO)
"""

from __future__ import annotations

import os
import struct
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent
LAYERS_DIR = ROOT / "layers"
DESKTOP_BRAND_DIR = ROOT.parents[1] / "apps" / "desktop" / "src" / "ui" / "brand"

SQUIRCLE_PATH = (
    "M 330,102 L 694,102 "
    "C 858,102 922,166 922,330 "
    "L 922,694 "
    "C 922,858 858,922 694,922 "
    "L 330,922 "
    "C 166,922 102,858 102,694 "
    "L 102,330 "
    "C 102,166 166,102 330,102 Z"
)

PLATE_DEFS = f"""    <linearGradient id="plate-bg" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#F9F9F6"/>
      <stop offset="28%" stop-color="#F0F1EE"/>
      <stop offset="70%" stop-color="#D5DAE1"/>
      <stop offset="100%" stop-color="#B5BCC6"/>
    </linearGradient>
    <radialGradient id="plate-sheen" cx="50%" cy="10%" r="60%">
      <stop offset="0%" stop-color="#FFFFFF" stop-opacity="0.42"/>
      <stop offset="100%" stop-color="#FFFFFF" stop-opacity="0"/>
    </radialGradient>
    <linearGradient id="plate-top-rim" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#FFFFFF" stop-opacity="0.96"/>
      <stop offset="28%" stop-color="#FFFFFF" stop-opacity="0.28"/>
      <stop offset="56%" stop-color="#FFFFFF" stop-opacity="0"/>
    </linearGradient>
    <linearGradient id="plate-bottom-rim" x1="0" y1="0" x2="0" y2="1">
      <stop offset="74%" stop-color="#758090" stop-opacity="0"/>
      <stop offset="100%" stop-color="#758090" stop-opacity="0.38"/>
    </linearGradient>
    <filter id="shadow-plate" x="-15%" y="-15%" width="130%" height="135%">
      <feGaussianBlur in="SourceAlpha" stdDeviation="18" result="blur"/>
      <feOffset in="blur" dx="0" dy="16" result="offset"/>
      <feFlood flood-color="#000000" flood-opacity="0.30" result="color"/>
      <feComposite in="color" in2="offset" operator="in" result="shadow"/>
      <feMerge>
        <feMergeNode in="shadow"/>
        <feMergeNode in="SourceGraphic"/>
      </feMerge>
    </filter>
    <path id="squircle" d="{SQUIRCLE_PATH}"/>
    <clipPath id="plate-clip">
      <use href="#squircle"/>
    </clipPath>"""

PAPER_FILTER = """    <filter id="shadow-paper" x="-18%" y="-18%" width="138%" height="138%">
      <feGaussianBlur in="SourceAlpha" stdDeviation="11" result="blur"/>
      <feOffset in="blur" dx="0" dy="10" result="offset"/>
      <feFlood flood-color="#222936" flood-opacity="0.21" result="color"/>
      <feComposite in="color" in2="offset" operator="in" result="shadow"/>
      <feMerge>
        <feMergeNode in="shadow"/>
        <feMergeNode in="SourceGraphic"/>
      </feMerge>
    </filter>"""

PENCIL_FILTER = """    <filter id="shadow-pencil" x="-25%" y="-25%" width="150%" height="150%">
      <feGaussianBlur in="SourceAlpha" stdDeviation="9" result="blur"/>
      <feOffset in="blur" dx="0" dy="11" result="offset"/>
      <feFlood flood-color="#1E2430" flood-opacity="0.24" result="color"/>
      <feComposite in="color" in2="offset" operator="in" result="shadow"/>
      <feMerge>
        <feMergeNode in="shadow"/>
        <feMergeNode in="SourceGraphic"/>
      </feMerge>
    </filter>"""

PLATE_GROUP = """  <g id="plate" filter="url(#shadow-plate)">
    <use href="#squircle" fill="url(#plate-bg)"/>
    <g clip-path="url(#plate-clip)">
      <use href="#squircle" fill="url(#plate-sheen)"/>
      <use href="#squircle" fill="none" stroke="url(#plate-top-rim)" stroke-width="3.5" transform="translate(0, 1)"/>
      <use href="#squircle" fill="none" stroke="url(#plate-bottom-rim)" stroke-width="2"/>
    </g>
  </g>"""

PENCIL_SHAPES = """      <polygon points="0,0 -19,-60 19,-60" fill="#F2E3CA"/>
      <polygon points="0,0 5,-60 19,-60" fill="#E0C8A2"/>
      <polygon points="0,0 -6.5,-20.5 6.5,-20.5" fill="#2B3038"/>
      <path d="M -19,-58 Q -13,-64 -7,-58 L -7,-308 L -19,-308 Z" fill="#F5BC52"/>
      <path d="M -7,-58 Q 0,-64 7,-58 L 7,-308 L -7,-308 Z" fill="#E59A2E"/>
      <path d="M 7,-58 Q 13,-64 19,-58 L 19,-308 L 7,-308 Z" fill="#C57818"/>
      <rect x="-19" y="-340" width="38" height="32" fill="#CDD3DC"/>
      <rect x="7" y="-340" width="12" height="32" fill="#B1B9C5"/>
      <rect x="-19" y="-331" width="38" height="4.5" fill="#969FAD"/>
      <rect x="-19" y="-319" width="38" height="4.5" fill="#969FAD"/>
      <path d="M -19,-340 L -19,-366 A 8,8 0 0 1 -11,-374 L 11,-374 A 8,8 0 0 1 19,-366 L 19,-340 Z" fill="#DC8D92"/>
      <path d="M 7,-340 L 7,-374 L 11,-374 A 8,8 0 0 1 19,-366 L 19,-340 Z" fill="#C4757B"/>"""


def build_master_svg() -> str:
    return f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" width="1024" height="1024">
  <defs>
{PLATE_DEFS}
{PAPER_FILTER}
{PENCIL_FILTER}
  </defs>
{PLATE_GROUP}
  <g id="paper-back" filter="url(#shadow-paper)">
    <rect x="342" y="200" width="446" height="608" rx="6" fill="#C8D0DC" stroke="#98A4B4" stroke-width="2" stroke-opacity="0.78" transform="rotate(-20 565 680)"/>
  </g>
  <g id="paper-mid" filter="url(#shadow-paper)">
    <rect x="342" y="200" width="446" height="608" rx="6" fill="#E5EAF1" stroke="#B4BFCB" stroke-width="2" stroke-opacity="0.80" transform="rotate(-10 565 680)"/>
  </g>
  <g id="paper-top" filter="url(#shadow-paper)">
    <rect x="342" y="200" width="446" height="608" rx="6" fill="#FFFFFF" stroke="#C6CFD9" stroke-width="2" stroke-opacity="0.88"/>
  </g>
  <g id="pencil" filter="url(#shadow-pencil)">
    <g transform="translate(578, 635) rotate(30) scale(1.12)">
{PENCIL_SHAPES}
    </g>
  </g>
</svg>
"""


def build_layered_svgs() -> dict[str, str]:
    plate = f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" width="1024" height="1024">
  <defs>
{PLATE_DEFS}
  </defs>
{PLATE_GROUP}
</svg>
"""
    paper_back = f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" width="1024" height="1024">
  <defs>
{PAPER_FILTER}
  </defs>
  <g id="paper-back" filter="url(#shadow-paper)">
    <rect x="342" y="200" width="446" height="608" rx="6" fill="#C8D0DC" stroke="#98A4B4" stroke-width="2" stroke-opacity="0.78"/>
  </g>
</svg>
"""
    paper_mid = f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" width="1024" height="1024">
  <defs>
{PAPER_FILTER}
  </defs>
  <g id="paper-mid" filter="url(#shadow-paper)">
    <rect x="342" y="200" width="446" height="608" rx="6" fill="#E5EAF1" stroke="#B4BFCB" stroke-width="2" stroke-opacity="0.80"/>
  </g>
</svg>
"""
    paper_top = f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" width="1024" height="1024">
  <defs>
{PAPER_FILTER}
  </defs>
  <g id="paper-top" filter="url(#shadow-paper)">
    <rect x="342" y="200" width="446" height="608" rx="6" fill="#FFFFFF" stroke="#C6CFD9" stroke-width="2" stroke-opacity="0.88"/>
  </g>
</svg>
"""
    pencil = f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" width="1024" height="1024">
  <defs>
{PENCIL_FILTER}
  </defs>
  <g id="pencil" filter="url(#shadow-pencil)">
    <g transform="translate(578, 635) rotate(30) scale(1.12)">
{PENCIL_SHAPES}
    </g>
  </g>
</svg>
"""
    return {
        "plate.svg": plate,
        "paper-back.svg": paper_back,
        "paper-mid.svg": paper_mid,
        "paper-top.svg": paper_top,
        "pencil.svg": pencil,
    }


def build_icon_32_svg() -> str:
    """Pixel-aligned 32x32 SVG: plate + 2 papers + simplified geometric pencil."""
    return """<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32" width="32" height="32">
  <defs>
    <linearGradient id="plate-bg-32" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#F9F9F6"/>
      <stop offset="40%" stop-color="#E8EAEE"/>
      <stop offset="100%" stop-color="#B5BCC6"/>
    </linearGradient>
    <filter id="shadow-32" x="-15%" y="-15%" width="130%" height="135%">
      <feGaussianBlur in="SourceAlpha" stdDeviation="0.6" result="blur"/>
      <feOffset in="blur" dx="0" dy="0.6" result="offset"/>
      <feFlood flood-color="#000000" flood-opacity="0.28" result="color"/>
      <feComposite in="color" in2="offset" operator="in" result="shadow"/>
      <feMerge>
        <feMergeNode in="shadow"/>
        <feMergeNode in="SourceGraphic"/>
      </feMerge>
    </filter>
  </defs>
  <g filter="url(#shadow-32)">
    <rect x="2.5" y="2.5" width="27" height="27" rx="7" fill="url(#plate-bg-32)" stroke="#A7B0BC" stroke-width="0.6"/>
  </g>
  <rect x="10.5" y="6" width="14.5" height="19.5" rx="1" fill="#D5DCE6" stroke="#9AA6B6" stroke-width="0.7" transform="rotate(-15 17.5 21.5)"/>
  <rect x="10.5" y="6" width="14.5" height="19.5" rx="1" fill="#FFFFFF" stroke="#B8C2CF" stroke-width="0.7"/>
  <g transform="translate(18.2, 20.2) rotate(30)">
    <polygon points="0,0 -1.6,-3.8 1.6,-3.8" fill="#F0DFC2"/>
    <polygon points="0,0 -0.6,-1.4 0.6,-1.4" fill="#2B3038"/>
    <rect x="-1.6" y="-13.6" width="1.6" height="9.9" fill="#F4B94F"/>
    <rect x="0" y="-13.6" width="1.6" height="9.9" fill="#D48420"/>
    <rect x="-1.6" y="-15.2" width="3.2" height="1.6" fill="#C4CBD4"/>
    <path d="M -1.6,-15.2 L -1.6,-16.7 A 0.8,0.8 0 0 1 -0.8,-17.5 L 0.8,-17.5 A 0.8,0.8 0 0 1 1.6,-16.7 L 1.6,-15.2 Z" fill="#D6868B"/>
  </g>
</svg>
"""


def build_icon_16_svg() -> str:
    """Pixel-aligned 16x16 SVG: plate + 1 white paper + bold diagonal pencil silhouette."""
    return """<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" width="16" height="16">
  <defs>
    <linearGradient id="plate-bg-16" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#F8F8F5"/>
      <stop offset="100%" stop-color="#B8BFC9"/>
    </linearGradient>
  </defs>
  <rect x="1" y="1" width="14" height="14" rx="3.5" fill="url(#plate-bg-16)" stroke="#9DA6B4" stroke-width="0.6"/>
  <rect x="4" y="3" width="8" height="10" rx="0.8" fill="#FFFFFF" stroke="#AAB4C2" stroke-width="0.6"/>
  <g transform="translate(8.6, 10.6) rotate(30)">
    <polygon points="0,0 -1.2,-2.2 1.2,-2.2" fill="#2B3038"/>
    <rect x="-1.2" y="-7.8" width="2.4" height="5.8" rx="0.3" fill="#E59A2E"/>
    <rect x="-1.2" y="-9.2" width="2.4" height="1.4" rx="0.4" fill="#D6868B"/>
  </g>
</svg>
"""


def build_symbolic_svg() -> str:
    """Linux GNOME symbolic monochrome 16x16 icon."""
    return """<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" width="16" height="16">
  <g fill="#2e3436">
    <path d="M 3.5,2 C 2.67,2 2,2.67 2,3.5 L 2,12.5 C 2,13.33 2.67,14 3.5,14 L 10.5,14 C 11.33,14 12,13.33 12,12.5 L 12,10.2 L 10.5,11.7 L 10.5,12.5 L 3.5,12.5 L 3.5,3.5 L 9.5,3.5 L 10.8,2.2 C 10.65,2.07 10.58,2 10.5,2 Z"/>
    <path d="M 13.7,2.3 C 13.3,1.9 12.67,1.9 12.28,2.3 L 7.2,7.38 L 6.5,9.5 L 8.62,8.8 L 13.7,3.72 C 14.1,3.33 14.1,2.7 13.7,2.3 Z"/>
  </g>
</svg>
"""


def render_png(svg_path: Path, png_path: Path, size: int) -> None:
    subprocess.run(
        [
            "inkscape",
            str(svg_path),
            "--export-type=png",
            f"--export-filename={png_path}",
            f"--export-width={size}",
            f"--export-height={size}",
        ],
        check=True,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )


def pack_ico(entries: list[tuple[int, Path]], out_path: Path) -> None:
    """Pack PNG files into a standard Windows multi-size .ico (PNG-in-ICO)."""
    images: list[tuple[int, bytes]] = []
    for size, path in entries:
        images.append((size, path.read_bytes()))

    count = len(images)
    header = struct.pack("<HHH", 0, 1, count)
    dir_entries = bytearray()
    offset = 6 + 16 * count
    payload = bytearray()

    for size, data in images:
        dim = 0 if size >= 256 else size
        dir_entries.extend(
            struct.pack(
                "<BBBBHHII",
                dim,
                dim,
                0,
                0,
                1,
                32,
                len(data),
                offset,
            )
        )
        payload.extend(data)
        offset += len(data)

    out_path.write_bytes(header + bytes(dir_entries) + bytes(payload))


def main() -> None:
    LAYERS_DIR.mkdir(parents=True, exist_ok=True)
    DESKTOP_BRAND_DIR.mkdir(parents=True, exist_ok=True)

    master_svg = build_master_svg()
    (ROOT / "icon.svg").write_text(master_svg, encoding="utf-8")
    (ROOT / "icon-layered.svg").write_text(master_svg, encoding="utf-8")

    for name, content in build_layered_svgs().items():
        (LAYERS_DIR / name).write_text(content, encoding="utf-8")
        (DESKTOP_BRAND_DIR / name).write_text(content, encoding="utf-8")

    (ROOT / "icon-32.svg").write_text(build_icon_32_svg(), encoding="utf-8")
    (ROOT / "icon-16.svg").write_text(build_icon_16_svg(), encoding="utf-8")
    (ROOT / "xcpc-workstation-symbolic.svg").write_text(build_symbolic_svg(), encoding="utf-8")

    size_sources: list[tuple[int, Path]] = [
        (512, ROOT / "icon.svg"),
        (256, ROOT / "icon.svg"),
        (128, ROOT / "icon.svg"),
        (64, ROOT / "icon.svg"),
        (48, ROOT / "icon.svg"),
        (32, ROOT / "icon-32.svg"),
        (24, ROOT / "icon-32.svg"),
        (16, ROOT / "icon-16.svg"),
    ]

    png_map: dict[int, Path] = {}
    for size, src in size_sources:
        out_png = ROOT / f"icon-{size}.png"
        render_png(src, out_png, size)
        png_map[size] = out_png
        print(f"generated {out_png.relative_to(ROOT)} ({size}x{size})")

    ico_sizes = [16, 24, 32, 48, 64, 128, 256]
    pack_ico([(s, png_map[s]) for s in ico_sizes], ROOT / "icon.ico")
    print(f"generated icon.ico ({len(ico_sizes)} sizes)")


if __name__ == "__main__":
    os.environ.setdefault("GDK_BACKEND", "wayland,x11")
    main()
