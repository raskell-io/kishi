# Brand

Kishi (騎士) means knight. The mark is a knight cut into a vermilion name seal,
a hanko, the stamp an artist presses onto a finished scroll.

| File | Use |
|---|---|
| `../../web/icon.svg` | the app icon: the seal on ink, rounded |
| `icon-square.svg` | the same, square, for home-screen icons (the OS rounds them) |
| `kishi-seal.svg` | the seal alone |
| `kishi-168.png` | the icon at 168 px, for project cards |
| `kishi-wordmark-light.svg`, `kishi-wordmark-dark.svg` | "kishi" in Geist, as paths: light for dark backgrounds, dark for light ones |
| `kishi-lockup-light.svg`, `kishi-lockup-dark.svg` | the seal and the wordmark together |
| `../../web/og.png` | the social card, 1200 × 630 |

## Colour

| Token | Hex | Role |
|---|---|---|
| Ink | `#0e0f0d` | the ground of the app and icon |
| Paper | `#ece6d9` | text on ink, the wordmark |
| Vermilion | `#c8553d` (seal), `#d8644a` (accent) | the seal; the app's only accent: the last move, your turn, the current move |
| Stone | `#e2ddd1` / `#a8a293` | the default board |
| Ivory | `#faf7f0` → `#e9e3d6` | shogi pieces, white Go stones |
| Wood edge | `#cdbf9f` | the depth under pieces and stones |

## Type

Geist for text, Geist Mono for moves, coordinates and figures (both SIL OFL,
by Vercel), and Noto Sans JP Bold for the shogi kanji (SIL OFL), all bundled
in `web/fonts/`.

## Regenerating

```sh
python3 scripts/brand.py     # icons, seal, wordmarks, lockups (needs fontTools and ImageMagick)
```

The marks are drawn in code, so changing a colour or the knight's outline is
a change to `scripts/brand.py`. The social card is an HTML page,
`scripts/og.html`; render it at 1200 × 630 with any headless browser, for
example `chrome --headless --screenshot=web/og.png --window-size=1200,630
--allow-file-access-from-files scripts/og.html`.
