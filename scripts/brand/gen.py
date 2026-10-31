# Generates the OML brand SVGs into docs/assets/brand/. Text is outlined to paths with
# fontTools from locally installed Inter Display / Inter / JetBrains Mono, so the SVGs render
# identically on GitHub, where web fonts are not loaded inside <img>.
# Usage: pip install fonttools && python3 scripts/brand/gen.py docs/assets/brand
#        node scripts/brand/render.js docs/assets/brand docs/assets/brand \
#          '[["social-preview.svg","social-preview.png",1280,640],["oml-icon.svg","oml-icon-512.png",512,512]]'
import random, os, sys
from fontTools.ttLib import TTFont
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen

OUT = sys.argv[1]
os.makedirs(OUT, exist_ok=True)
FD = "/usr/share/fonts/opentype/inter/"
FONTS = {
  "disp-bold": FD+"InterDisplay-Bold.otf",
  "disp-semi": FD+"InterDisplay-SemiBold.otf",
  "medium": FD+"Inter-Medium.otf",
  "regular": FD+"Inter-Regular.otf",
  "mono": "/usr/share/fonts/truetype/jetbrains-mono/JetBrainsMono-Medium.ttf",
}
_cache = {}
def font(k):
    if k not in _cache:
        f = TTFont(FONTS[k]); _cache[k] = (f, f.getGlyphSet(), f.getBestCmap(), f["head"].unitsPerEm, f["hmtx"])
    return _cache[k]

def kern_pairs(f):
    # simple pair kerning from GPOS PairPos format 1 only (best effort)
    return {}

def text_path(k, s, size, x, y, track=0.0, anchor="start"):
    f, gs, cmap, upm, hmtx = font(k)
    sc = size / upm
    w = 0; glyphs = []
    for ch in s:
        g = cmap.get(ord(ch))
        if g is None: g = cmap[ord(" ")]
        glyphs.append((g, w)); w += hmtx[g][0] * sc + track * size
    w -= track * size
    if anchor == "middle": x -= w/2
    elif anchor == "end": x -= w
    pen = SVGPathPen(gs)
    for g, off in glyphs:
        tp = TransformPen(pen, (sc, 0, 0, -sc, x + off, y))
        gs[g].draw(tp)
    return pen.getCommands(), w

def tp(k, s, size, x, y, fill, track=0.0, anchor="start", extra=""):
    d, w = text_path(k, s, size, x, y, track, anchor)
    return f'<path fill="{fill}" {extra} d="{d}"/>', w

# ---------- palette ----------
P = dict(void="#04060D", deep="#0A0F1D", panel="#0E1526", line="#1E2745",
         star="#E9EDF8", dust="#94A0C0", faint="#5C6584",
         gold="#E8B45A", teal="#4FD8C0", blue="#7AA2FF", violet="#B48AF2",
         green="#46D684", red="#F0716E",
         paper="#F7F8FC", paper2="#ECEFF8", ink="#0B1020", ink2="#4A5370", lline="#D5DAEA",
         gold_d="#A86E12", teal_d="#0B8A78", blue_d="#3157D6", violet_d="#7A4FCF")

def mark_group(cx, cy, s, left, right, core, sw_ratio=7/64):
    """The OML mark: two facing chevrons (writer, reviewer) closing on a claim at the core.
    Drawn in a 64-unit box centred on (cx, cy), scaled to s px."""
    k = s/64.0
    sw = 64*sw_ratio
    return (f'<g transform="translate({cx-32*k:.2f} {cy-32*k:.2f}) scale({k:.4f})" fill="none" '
            f'stroke-width="{sw:.2f}" stroke-linejoin="miter" stroke-linecap="butt">'
            f'<path stroke="{left}" d="M28.6 10.6 L7.2 32 L28.6 53.4"/>'
            f'<path stroke="{right}" d="M35.4 10.6 L56.8 32 L35.4 53.4"/>'
            f'<rect x="26.5" y="26.5" width="11" height="11" transform="rotate(45 32 32)" fill="{core}" stroke="none"/>'
            f'</g>')

def svg(w, h, body, title, desc=""):
    return (f'<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}" role="img" aria-labelledby="t d">'
            f'<title id="t">{title}</title><desc id="d">{desc}</desc>{body}</svg>\n')

def write(name, content):
    open(os.path.join(OUT, name), "w").write(content); print("wrote", name)

# ---------- marks ----------
write("oml-mark.svg", svg(64, 64, mark_group(32, 32, 64, P["gold"], P["teal"], P["star"]),
      "Open Memory Lab", "Two facing chevrons, the writer and the reviewer, closing on a claim."))
write("oml-mark-light.svg", svg(64, 64, mark_group(32, 32, 64, P["gold_d"], P["teal_d"], P["ink"]),
      "Open Memory Lab", "OML mark for light backgrounds."))
write("oml-mark-mono.svg", svg(64, 64, mark_group(32, 32, 64, "currentColor", "currentColor", "currentColor"),
      "Open Memory Lab", "Single-colour OML mark; inherits currentColor."))
tile = (f'<rect width="64" height="64" rx="14" fill="{P["deep"]}"/>'
        f'<rect x="0.5" y="0.5" width="63" height="63" rx="13.5" fill="none" stroke="{P["line"]}"/>'
        + mark_group(32, 32, 46, P["gold"], P["teal"], P["star"]))
write("oml-icon.svg", svg(64, 64, tile, "Open Memory Lab", "App icon: OML mark on a deep-navy tile."))
write("favicon.svg", svg(64, 64, f'<rect width="64" height="64" rx="14" fill="{P["deep"]}"/>' +
      mark_group(32, 32, 52, P["gold"], P["teal"], P["star"], sw_ratio=9/64), "OML", ""))

# ---------- lockups ----------
def lockup(dark):
    fg = P["star"] if dark else P["ink"]; sub = P["dust"] if dark else P["ink2"]
    m = mark_group(44, 44, 72, P["gold"] if dark else P["gold_d"], P["teal"] if dark else P["teal_d"], fg)
    t1, w1 = tp("disp-bold", "Open Memory Lab", 40, 100, 50, fg, track=-0.01)
    t2, w2 = tp("mono", "every memory is a claim under test", 13.5, 102, 74, sub, track=0.02)
    W = int(100 + max(w1, w2) + 16)
    return svg(W, 88, m + t1 + t2, "Open Memory Lab", "Every memory is a claim under test.")
write("oml-lockup-dark.svg", lockup(True))
write("oml-lockup-light.svg", lockup(False))

# ---------- shared art: starfield + atlas orbit + journal chain ----------
def starfield(w, h, n, seed, col, x0=0):
    r = random.Random(seed); out = []
    for _ in range(n):
        x = r.uniform(x0, w); y = r.uniform(0, h); rad = r.choice([0.6, 0.8, 1, 1, 1.3, 1.7])
        op = r.uniform(0.25, 0.9)
        out.append(f'<circle cx="{x:.1f}" cy="{y:.1f}" r="{rad}" fill="{col}" opacity="{op:.2f}"/>')
    return "".join(out)

def diamond(x, y, s, fill, extra=""):
    return f'<rect x="{x-s/2:.1f}" y="{y-s/2:.1f}" width="{s}" height="{s}" transform="rotate(45 {x:.1f} {y:.1f})" fill="{fill}" {extra}/>'

def atlas(cx, cy, R, dark, seed=7):
    """Workarea orbits with claims on them; one held claim mid-review (dashed link to reviewer)."""
    import math
    r = random.Random(seed)
    lc = P["line"] if dark else P["lline"]
    cols = ([P["teal"], P["teal"], P["green"], P["blue"], P["gold"], P["violet"]] if dark else
            [P["teal_d"], P["teal_d"], "#1F9D57", P["blue_d"], P["gold_d"], P["violet_d"]])
    out = [f'<g opacity="{1 if dark else 0.95}">']
    rings = [(R*0.38, 0.42), (R*0.68, 0.44), (R, 0.46)]
    for i, (rr, flat) in enumerate(rings):
        out.append(f'<ellipse cx="{cx}" cy="{cy}" rx="{rr:.1f}" ry="{rr*flat:.1f}" fill="none" stroke="{lc}" stroke-width="1.2" transform="rotate(-14 {cx} {cy})"/>')
    pts = []
    for i, (rr, flat) in enumerate(rings):
        for j in range(4 + 2*i):
            a = r.uniform(0, 2*math.pi)
            x = rr*math.cos(a); y = rr*flat*math.sin(a)
            th = math.radians(-14)
            X = cx + x*math.cos(th) - y*math.sin(th); Y = cy + x*math.sin(th) + y*math.cos(th)
            pts.append((X, Y, cols[(len(pts)*5) % len(cols)], r.choice([7, 8, 9, 10, 12])))
    # grant lines from a few claims to core
    for (X, Y, c, s) in pts[::4]:
        out.append(f'<line x1="{cx}" y1="{cy}" x2="{X:.1f}" y2="{Y:.1f}" stroke="{c}" stroke-width="1" opacity="0.35"/>')
    for (X, Y, c, s) in pts:
        out.append(diamond(X, Y, s, c, 'opacity="0.95"'))
    # universe core: the mark itself, small
    core = P["star"] if dark else P["ink"]
    out.append(f'<circle cx="{cx}" cy="{cy}" r="{R*0.16:.1f}" fill="{P["violet"] if dark else P["violet_d"]}" opacity="0.10"/>')
    out.append(mark_group(cx, cy, R*0.22, P["gold"] if dark else P["gold_d"], P["teal"] if dark else P["teal_d"], core))
    out.append('</g>')
    return "".join(out)

def chain(x, y, n, size, gap, dark):
    """Signed journal: hash-linked blocks."""
    lc = P["line"] if dark else P["lline"]; fill = P["panel"] if dark else "#FFFFFF"
    acc = P["violet"] if dark else P["violet_d"]
    out = []
    for i in range(n):
        bx = x + i*(size+gap)
        if i: out.append(f'<line x1="{bx-gap}" y1="{y+size/2}" x2="{bx}" y2="{y+size/2}" stroke="{acc}" stroke-width="1.5" opacity="0.7"/>')
        out.append(f'<rect x="{bx}" y="{y}" width="{size}" height="{size}" rx="4" fill="{fill}" stroke="{lc}" stroke-width="1.2"/>')
        out.append(f'<rect x="{bx+size*0.25}" y="{y+size*0.3}" width="{size*0.5}" height="2" rx="1" fill="{acc}" opacity="0.8"/>')
        out.append(f'<rect x="{bx+size*0.25}" y="{y+size*0.55}" width="{size*0.32}" height="2" rx="1" fill="{acc}" opacity="0.5"/>')
    return "".join(out)

def pipeline(x, y, size, dark):
    """held -> scanned -> reviewed by another identity -> promoted, as coloured dots + mono labels."""
    steps = [("held", P["gold"] if dark else P["gold_d"]),
             ("scanned", P["blue"] if dark else P["blue_d"]),
             ("reviewed by another identity", P["teal"] if dark else P["teal_d"]),
             ("promoted", P["green"] if dark else "#1F9D57")]
    lab = P["dust"] if dark else P["ink2"]; arrow = P["faint"] if dark else "#9AA3BD"
    out = []; cx = x
    for i, (s, c) in enumerate(steps):
        out.append(diamond(cx + size*0.35, y - size*0.33, size*0.5, c))
        p, w = tp("mono", s, size, cx + size*0.95, y, lab)
        out.append(p); cx += size*0.95 + w
        if i < len(steps)-1:
            p, w = tp("mono", "→", size, cx + size*0.5, y, arrow); out.append(p); cx += size*0.5 + w + size*0.6
    return "".join(out), cx - x

# ---------- README hero ----------
def hero(dark):
    W, H = 1280, 380
    bg = P["void"] if dark else P["paper"]; fg = P["star"] if dark else P["ink"]; sub = P["dust"] if dark else P["ink2"]
    defs = (f'<defs><radialGradient id="glow" cx="0.80" cy="0.45" r="0.55">'
            f'<stop offset="0" stop-color="{P["violet"] if dark else P["violet_d"]}" stop-opacity="{0.22 if dark else 0.10}"/>'
            f'<stop offset="1" stop-color="{bg}" stop-opacity="0"/></radialGradient>'
            f'<linearGradient id="fade" x1="0" x2="1"><stop offset="0" stop-color="{bg}"/><stop offset="0.55" stop-color="{bg}" stop-opacity="0"/></linearGradient></defs>')
    b = [defs, f'<rect width="{W}" height="{H}" rx="18" fill="{bg}"/>',
         f'<rect width="{W}" height="{H}" rx="18" fill="url(#glow)"/>']
    if not dark: b.append(f'<rect x="0.5" y="0.5" width="{W-1}" height="{H-1}" rx="17.5" fill="none" stroke="{P["lline"]}"/>')
    b.append(starfield(W, H, 110, 3, P["star"] if dark else "#8C96B8", x0=560))
    b.append(atlas(1010, 185, 230, dark))
    b.append(f'<rect width="{W*0.62}" height="{H}" rx="18" fill="url(#fade)"/>')
    b.append(mark_group(118, 150, 120, P["gold"] if dark else P["gold_d"], P["teal"] if dark else P["teal_d"], fg))
    t, _ = tp("disp-bold", "Open Memory Lab", 64, 206, 152, fg, track=-0.015); b.append(t)
    t, _ = tp("medium", "Every memory is a claim under test.", 27, 208, 196, sub); b.append(t)
    p, _ = pipeline(64, 286, 18, dark); b.append(p)
    t, _ = tp("mono", "external memory for AI agents and crews", 15, 64, 330, P["faint"] if dark else "#8C96B8", track=0.04); b.append(t)
    return svg(W, H, "".join(b), "Open Memory Lab — every memory is a claim under test",
               "External memory for AI agents and crews: held, scanned, reviewed by another identity, promoted.")
write("readme-hero-dark.svg", hero(True))
write("readme-hero-light.svg", hero(False))

# ---------- social preview 1280x640 ----------
def social():
    W, H = 1280, 640; dark = True
    bg = P["void"]; fg = P["star"]; sub = P["dust"]
    defs = (f'<defs><radialGradient id="glow" cx="0.78" cy="0.48" r="0.6">'
            f'<stop offset="0" stop-color="{P["violet"]}" stop-opacity="0.24"/><stop offset="1" stop-color="{bg}" stop-opacity="0"/></radialGradient>'
            f'<linearGradient id="fade" x1="0" x2="1"><stop offset="0" stop-color="{bg}"/><stop offset="0.6" stop-color="{bg}" stop-opacity="0"/></linearGradient></defs>')
    b = [defs, f'<rect width="{W}" height="{H}" fill="{bg}"/>', f'<rect width="{W}" height="{H}" fill="url(#glow)"/>',
         starfield(W, H, 170, 11, P["star"], x0=520), atlas(985, 300, 300, True, seed=21),
         f'<rect width="{W*0.66}" height="{H}" fill="url(#fade)"/>']
    b.append(mark_group(150, 170, 128, P["gold"], P["teal"], fg))
    t, _ = tp("disp-bold", "Open Memory Lab", 82, 88, 335, fg, track=-0.02); b.append(t)
    t, _ = tp("medium", "Every memory is a claim under test.", 34, 90, 392, sub); b.append(t)
    # three pills
    x = 90
    for label, c in [("held on arrival", P["gold"]), ("non-author review", P["teal"]), ("signed journal", P["violet"])]:
        d, w = text_path("mono", label, 19, 0, 0)
        pw = w + 62
        b.append(f'<rect x="{x}" y="440" width="{pw:.0f}" height="46" rx="23" fill="{P["deep"]}" stroke="{P["line"]}" stroke-width="1.5"/>')
        b.append(diamond(x + 26, 463, 10, c))
        p, _ = tp("mono", label, 19, x + 44, 470, P["star"]); b.append(p)
        x += pw + 14
    b.append(chain(90, 540, 7, 30, 16, True))
    t, _ = tp("mono", "github.com/e8kor/open-memory-lab", 18, 420, 561, P["faint"], track=0.02); b.append(t)
    return svg(W, H, "".join(b), "Open Memory Lab", "Social preview")
write("social-preview.svg", social())
