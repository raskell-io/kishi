"""Generates Kishi's brand marks from code, so they stay consistent:

    python3 scripts/brand.py

- web/icon.svg            the icon: a knight's move, an L, on a dark board
- web/icon-{180,192,512}.png and docs/brand/kishi-168.png (needs ImageMagick)
- docs/brand/kishi-wordmark-{light,dark}.svg   "kishi" in Geist Mono Bold, as paths,
  with the accent cursor; light is for dark backgrounds, dark for light ones
- docs/brand/kishi-lockup-{light,dark}.svg     icon and wordmark side by side

The wordmark needs fontTools (pip install fonttools brotli).
"""
import io
import pathlib
import subprocess

from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen
from fontTools.ttLib import TTFont
from fontTools.varLib import instancer

ROOT = pathlib.Path(__file__).resolve().parent.parent
WEB = ROOT / "web"
BRAND = ROOT / "docs" / "brand"

INK = "#0a0c10"
SQUARE = "#ffffff"
BLUE = "#79c0ff"
GREEN = "#7ee787"
LIGHT_TEXT = "#f0f6fc"
DARK_TEXT = "#0a0c10"


def icon(size=512, rounded=True):
    """A dark board, barely there, and a knight's move across it: two up, one right.

    Six squares across, shifted half a square down, so the move sits square-centred
    in the middle of the icon.
    """
    cell = size / 6
    squares = "".join(
        f'<rect x="{c * cell:g}" y="{(r - 0.5) * cell:g}" width="{cell:g}" height="{cell:g}"/>'
        for r in range(7)
        for c in range(6)
        if (r + c) % 2 == 0
    )
    x0, x1 = 2.5 * cell, 3.5 * cell
    y0, y1 = 4 * cell, 2 * cell
    stroke = cell * 0.3
    mark = cell * 0.62
    ring = cell * 0.24
    clip = f'<clipPath id="k"><rect width="{size}" height="{size}" rx="{size * 0.22:g}"/></clipPath>' if rounded else ""
    group = 'clip-path="url(#k)"' if rounded else ""
    return f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {size} {size}" width="{size}" height="{size}">
  <title>Kishi</title>
  <defs>
    <linearGradient id="g" x1="0" y1="{y0:g}" x2="0" y2="{y1:g}" gradientUnits="userSpaceOnUse">
      <stop offset="0" stop-color="{BLUE}"/><stop offset="1" stop-color="{GREEN}"/>
    </linearGradient>
    {clip}
  </defs>
  <g {group}>
    <rect width="{size}" height="{size}" fill="{INK}"/>
    <g fill="{SQUARE}" fill-opacity="0.05">{squares}</g>
    <path d="M{x0:g} {y0 - ring - stroke * 0.5:g} V{y1:g} H{x1 - mark / 2:g}" fill="none" stroke="url(#g)" stroke-width="{stroke:g}" stroke-linecap="round" stroke-linejoin="round"/>
    <circle cx="{x0:g}" cy="{y0:g}" r="{ring:g}" fill="none" stroke="{BLUE}" stroke-width="{stroke * 0.8:g}"/>
    <rect x="{x1 - mark / 2:g}" y="{y1 - mark / 2:g}" width="{mark:g}" height="{mark:g}" rx="{mark * 0.2:g}" fill="{GREEN}"/>
  </g>
</svg>
"""


def wordmark_paths():
    """'kishi' in Geist Mono Bold as one SVG path, plus the advance width and cap metrics."""
    font = TTFont(WEB / "fonts" / "GeistMono-Variable.woff2")
    font = instancer.instantiateVariableFont(font, {"wght": 700})
    # Round-trip through bytes so the instance has plain glyph tables.
    buf = io.BytesIO()
    font.flavor = None
    font.save(buf)
    font = TTFont(io.BytesIO(buf.getvalue()))
    glyphs = font.getGlyphSet()
    cmap = font.getBestCmap()
    upm = font["head"].unitsPerEm
    ascent = font["OS/2"].sTypoAscender
    x_height = font["OS/2"].sxHeight
    pen = SVGPathPen(glyphs)
    x = 0
    for ch in "kishi":
        name = cmap[ord(ch)]
        # Fonts draw y up; SVG draws y down.
        glyphs[name].draw(TransformPen(pen, (1, 0, 0, -1, x, 0)))
        x += glyphs[name].width
    return pen.getCommands(), x, upm, ascent, x_height


def wordmark(color, with_icon=False):
    d, advance, upm, ascent, x_height = wordmark_paths()
    # The cursor: a block the size of an x-height cell, after the last letter.
    gap = upm * 0.1
    cursor_w = upm * 0.26
    width = advance + gap + cursor_w
    top = -ascent
    height = ascent + upm * 0.12
    cursor = f'<rect x="{advance + gap:g}" y="{-x_height:g}" width="{cursor_w:g}" height="{x_height:g}" rx="{upm * 0.06:g}" fill="{GREEN}"/>'
    text = f'<path fill="{color}" d="{d}"/>{cursor}'
    if not with_icon:
        return (
            f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 {top:g} {width:g} {height:g}" role="img" aria-label="Kishi">'
            f"{text}</svg>\n"
        )
    # Lockup: the icon at cap height plus a little, then the wordmark.
    s = height
    pad = s * 0.35
    inner = icon(512).split("\n", 1)[1].rsplit("</svg>", 1)[0]
    scale = s / 512
    total = s + pad + width
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 {top:g} {total:g} {height:g}" role="img" aria-label="Kishi">'
        f'<g transform="translate(0 {top:g}) scale({scale:g})">{inner}</g>'
        f'<g transform="translate({s + pad:g} 0)">{text}</g></svg>\n'
    )


def png(svg_path, out, size):
    subprocess.run(
        ["magick", "-background", "none", "-density", "300", str(svg_path), "-resize", f"{size}x{size}", str(out)],
        check=True,
    )


def main():
    BRAND.mkdir(parents=True, exist_ok=True)
    (WEB / "icon.svg").write_text(icon())
    # Home-screen icons are square: iOS and Android round the corners themselves.
    square = BRAND / "icon-square.svg"
    square.write_text(icon(rounded=False))
    for size in (180, 192, 512):
        png(square, WEB / f"icon-{size}.png", size)
    png(WEB / "icon.svg", BRAND / "kishi-168.png", 168)
    (BRAND / "kishi-wordmark-light.svg").write_text(wordmark(LIGHT_TEXT))
    (BRAND / "kishi-wordmark-dark.svg").write_text(wordmark(DARK_TEXT))
    (BRAND / "kishi-lockup-light.svg").write_text(wordmark(LIGHT_TEXT, with_icon=True))
    (BRAND / "kishi-lockup-dark.svg").write_text(wordmark(DARK_TEXT, with_icon=True))
    print("wrote", ", ".join(p.name for p in sorted(BRAND.iterdir())))


if __name__ == "__main__":
    main()
