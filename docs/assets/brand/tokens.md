# Open Memory Lab design tokens

Direction: Atlas, cosmic. A stellar system on an engraved atlas plate. The foundation is the star at the centre;
the lab is the system forming around it.

## What each colour means

- **Warm (star)** is the foundation and only the foundation: the star, its glow, the companion star that gives the
  second look, the line of sight and the bracket around a body waiting to join. Never use warm for lab features.
- **Ice (dark) / steel (light)** is the lab: orbits, planets (shared vocabularies, whole datasets), the fenced
  experiment and its dashed fence, and the second line of the tagline.
- **The dashed outer arc is the forming disk: growing.** It stands for lab work that is designed but not built yet.
  Draw it dashed, never solid.
- **Night / paper** are grounds; **text** and **muted** are for words; **line** and **faint** are for leaders,
  construction lines and plate ticks only.

## Dark (primary)

Ground `#0A0D22`, with a warm radial glow from the star (star colour at 30% at the centre, fading to 0) and a soft vignette.

| Token | Hex | Role | Contrast on ground | Use |
|---|---|---|---|---|
| text | `#EFE9DA` | Headlines, wordmark, body | 15.9:1 | AAA |
| muted | `#AEB6D2` | Proposal and secondary copy | 9.5:1 | AAA |
| star | `#F6A83E` | Foundation; first tagline line; foundation labels | 9.7:1 | AAA |
| ice | `#9FC6E6` | Lab; second tagline line; lab labels | 10.7:1 | AAA |
| line | `#5F7FA3` | Leader lines, plate ticks | 4.6:1 | decorative lines only, never text |
| faint | `#2E3664` | Construction axis | n/a | graphics only |

## Light

Ground `#F1EBDC` (star-chart paper), with a fainter warm glow and no vignette.

| Token | Hex | Role | Contrast on ground | Use |
|---|---|---|---|---|
| text | `#10153A` | Headlines, wordmark, body | 14.8:1 | AAA |
| muted | `#414872` | Proposal and secondary copy | 7.4:1 | AAA |
| star (on paper) | `#C4700D` | Foundation; first tagline line; foundation labels | 3.1:1 | AA large (18px+ / 14px+ bold) |
| steel | `#2F6690` | Lab; second tagline line; lab labels | 5.2:1 | AA |
| line | `#7C97B4` | Leader lines, plate ticks | 2.5:1 | decorative lines only, never text |
| faint | `#C4BDAE` | Construction axis | n/a | graphics only |

Warm on paper passes only as large text (3:1), so set warm words on paper at 18px or above, or 14px bold and above. (Hardened in the kit: the v3 mockup value #C8730F was 2.98:1 and is nudged to #C4700D.)

## Type

| Family | Weight | Role |
|---|---|---|
| Newsreader Display (OFL), optical size 72 | SemiBold 600 | Wordmark, headlines, tagline |
| Inter (OFL), optical size 14 | Regular 400, Medium 500, SemiBold 600 | Body copy (Regular); plate labels in caps with +11% tracking (Medium, SemiBold for foundation and second look) |
| JetBrains Mono (OFL) | Medium 500 | Plate captions and small caps lines, +14 to 16% tracking |

All text in the kit is outlined to paths. Headline line one is star, line two is ice/steel.

## Mark

A full orbit ring that is also the O of Open (lab colour), the warm star off-centre inside it, and a companion
body sitting in a small gap in the ring. Clear space: at least 25% of the mark's
width on every side. Minimum size: 16px (use favicon.svg, which has thicker strokes, below 24px).

## Files

| File | Use |
|---|---|
| oml-mark.svg | Mark on dark grounds |
| oml-mark-light.svg | Mark on light grounds |
| oml-mark-mono.svg | One colour, follows `currentColor` |
| oml-icon.svg, oml-icon-512.png | App tile / avatar on the night ground; works in light and dark UIs |
| favicon.svg | Favicon tile, strokes tuned for 16px |
| oml-lockup-dark.svg / -light.svg | Mark, wordmark and tagline |
| oml-wordmark-dark.svg / -light.svg | Mark and wordmark, no tagline |
| readme-hero-dark.svg / -light.svg | 1280x380 README banner; pair them with a `<picture>` element and `prefers-color-scheme` so each theme gets its own |
| social-preview.svg / .png | 1280x640 link preview |
