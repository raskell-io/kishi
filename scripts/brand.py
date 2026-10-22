"""Generates Kishi's brand marks from code, so they stay consistent:

    python3 scripts/brand.py

- web/icon.svg            the icon: a knight cut into a vermilion name seal (hanko), on ink
- web/icon-{180,192,512}.png and docs/brand/kishi-168.png (needs ImageMagick)
- docs/brand/kishi-seal.svg                    the seal alone, for small sizes and print
- docs/brand/kishi-wordmark-{light,dark}.svg   "kishi" in Geist, as paths;
  light is for dark backgrounds, dark for light ones
- docs/brand/kishi-lockup-{light,dark}.svg     seal and wordmark side by side

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

INK = "#0e0f0d"
PAPER = "#ece6d9"
VERMILION = "#c8553d"

# A knight's head and its base, drawn on a 512 grid; the bust stands on y = 404.
KNIGHT = (
    "M176 404 L176 330 C176 300 196 282 226 262 L170 270 C150 272 136 258 142 240 "
    "L196 168 L190 120 L232 146 C300 140 360 190 366 270 L372 404 Z"
)


def seal_group(fill_knight=INK):
    """The seal in a 512 box: a vermilion square, a fine inner border, the knight."""
    return f"""<rect x="0" y="0" width="512" height="512" rx="56" fill="{VERMILION}"/>
    <rect x="30" y="30" width="452" height="452" rx="34" fill="none" stroke="{fill_knight}" stroke-width="10" opacity="0.9"/>
    <g transform="translate(256 262) scale(1.06) translate(-254 -276)">
      <path d="{KNIGHT}" fill="{fill_knight}"/>
      <circle cx="236" cy="190" r="14" fill="{VERMILION}"/>
      <rect x="150" y="404" width="236" height="28" rx="14" fill="{fill_knight}"/>
    </g>"""


def icon(rounded=True):
    """The app icon: the seal filling most of the tile, on ink."""
    radius = f' rx="{512 * 0.22:g}"' if rounded else ""
    return f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512" width="512" height="512">
  <title>Kishi</title>
  <rect width="512" height="512"{radius} fill="{INK}"/>
  <g transform="translate(56 56) scale(0.78125)">{seal_group()}</g>
</svg>
"""


def seal():
    return f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512" width="512" height="512">
  <title>Kishi</title>
  {seal_group()}
</svg>
"""


def wordmark_paths():
    """'kishi' in Geist Medium as one SVG path, plus its advance width and metrics."""
    font = TTFont(WEB / "fonts" / "Geist-Variable.woff2")
    font = instancer.instantiateVariableFont(font, {"wght": 520})
    buf = io.BytesIO()
    font.flavor = None
    font.save(buf)
    font = TTFont(io.BytesIO(buf.getvalue()))
    glyphs = font.getGlyphSet()
    cmap = font.getBestCmap()
    upm = font["head"].unitsPerEm
    ascent = font["OS/2"].sTypoAscender
    pen = SVGPathPen(glyphs)
    x = 0
    tracking = upm * 0.01
    for ch in "kishi":
        name = cmap[ord(ch)]
        glyphs[name].draw(TransformPen(pen, (1, 0, 0, -1, x, 0)))
        x += glyphs[name].width + tracking
    return pen.getCommands(), x - tracking, upm, ascent


def wordmark(color, with_seal=False):
    d, width, upm, ascent = wordmark_paths()
    top = -ascent
    height = ascent + upm * 0.06
    text = f'<path fill="{color}" d="{d}"/>'
    if not with_seal:
        return (
            f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 {top:g} {width:g} {height:g}" role="img" aria-label="Kishi">'
            f"{text}</svg>\n"
        )
    s = height
    pad = s * 0.32
    total = s + pad + width
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 {top:g} {total:g} {height:g}" role="img" aria-label="Kishi">'
        f'<g transform="translate(0 {top:g}) scale({s / 512:g})">{seal_group()}</g>'
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
    (BRAND / "kishi-seal.svg").write_text(seal())
    # Home-screen icons are square: iOS and Android round the corners themselves.
    square = BRAND / "icon-square.svg"
    square.write_text(icon(rounded=False))
    for size in (180, 192, 512):
        png(square, WEB / f"icon-{size}.png", size)
    png(WEB / "icon.svg", BRAND / "kishi-168.png", 168)
    (BRAND / "kishi-wordmark-light.svg").write_text(wordmark(PAPER))
    (BRAND / "kishi-wordmark-dark.svg").write_text(wordmark(INK))
    (BRAND / "kishi-lockup-light.svg").write_text(wordmark(PAPER, with_seal=True))
    (BRAND / "kishi-lockup-dark.svg").write_text(wordmark(INK, with_seal=True))
    print("wrote", ", ".join(p.name for p in sorted(BRAND.iterdir())))


if __name__ == "__main__":
    main()
