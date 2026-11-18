# open-island: Logo Guidelines (compact)

## 1. The logo

- **Idea**: a high-contrast serif **O** and **I** (the monogram of open-island). The I carries a **pill-shaped top serif**: the island hanging from the top edge of the screen. The same pill is the hyphen in the wordmark.
- **Versions**: horizontal (primary) · stacked · symbol (OI) · wordmark only.
- **Colour**: black and white only.

| File | Use |
|---|---|
| `open-island-symbol.svg` | master symbol, normal cut, 32 px high and up |
| `open-island-symbol-small.svg` | small-size cut (thicker hairlines), 16 to 31 px high |
| `open-island-symbol-reversed.svg` | white-on-dark cut (thinned to offset the glow of light on dark); `variants/symbol/open-island-symbol-reversed-white.svg` is the ready white file |
| `open-island-horizontal.svg`, `open-island-stacked.svg`, `open-island-wordmark.svg` | lockups; black and white files in `variants/<name>/` with 1200 px PNGs |
| `variants/symbol/` | black, square (avatar), favicon, app icon (white on black tile), 512 px PNGs |
| `variants/web/` | `favicon.ico`, `favicon.svg`, 16/32/48 PNGs, apple-touch icon, 192/512 PNGs, maskable icon, `site.webmanifest`, `head-snippet.html` (all built from the small-size cut) |
| `presentation.html` | presentation board with mockups (open in a browser) |

All masters are filled outlines on a 256-unit-high canvas: no live text, no strokes, no effects.

## 2. Clear space

Keep a clear zone of **1 × the width of the I's top serif** (about 0.4 × the height of the mark) around the logo on all sides. The zone scales with the logo; never use a fixed distance.

## 3. Minimum size

| Version | Screen | Print |
|---|---|---|
| Horizontal | 160 px wide | not tested |
| Symbol, normal cut | 32 px high | not tested |
| Symbol, small cut | 16 px high (favicon) | not tested |

The screen minimums come from looking at renders at 16, 24 and 32 px; the print minimums were not tested and are deliberately left empty.

## 4. Colour

| Name | HEX | RGB | CMYK | Pantone |
|---|---|---|---|---|
| Black | #000000 | 0, 0, 0 | not recorded | not recorded |
| White | #FFFFFF | 255, 255, 255 | not recorded | not recorded |

**Approved pairs**: black on white · white on black (use the reversed cut) · on a photo, the black or white version over a calm area. Never use grey for the logo.

## 5. Typography

- **Wordmark**: Noto Serif Display Medium, converted to outlines, with the hyphen replaced by a pill. Noto Serif Display is under the SIL Open Font License 1.1, which allows use in a logo. The wordmark is therefore font-based, not custom lettering.
- **Letter spacing**: +0.035 em; there is no pair kerning.
- **Never retype the wordmark**: use the files.
- **UI and documents**: the font is not decided; the island itself uses the font in `config.json`.

## 6. Don'ts

Don't stretch or squash · don't recolour (black and white only) · don't rotate · don't add shadows, outlines, gradients or effects · don't use the normal cut below 32 px or the small cut above 31 px · don't use the normal cut white on black (use the reversed one) · don't separate the O from the I with other characters · don't redraw the pill serif as a plain rectangle.

## 7. Handover notes: what was and was not checked

- **Checked**: looked at the renders at 16/24/32/64 px, on white and on black, in the favicon, app-icon and maskable files, and in the presentation board (headless Firefox screenshot). The SVG audit scores 100/100 for the three symbol cuts.
- **Not done**: no trademark or reverse-image search (do one before wide use); no print test and no CMYK/Pantone values; no check of the one-colour embroidery or engraving case.
- **By eye, not by system**: the gap between O and I, the thinning of the reversed cut, the lockup spacing and the weight match between the symbol and the wordmark. The O and I overshoot the cap height by 2 units on a 256 grid.
- **Open**: custom wordmark lettering to replace the font, a motion version (the pill could grow into the island), and the stacked lockup's proportions if it is used on small avatars.
- **Contact and master files**: contact not recorded; masters are in this folder.
