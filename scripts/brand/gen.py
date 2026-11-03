#!/usr/bin/env python3
"""
Open Memory Lab brand kit generator (direction: Atlas, cosmic).

Usage:   python3 gen.py OUT_DIR
Needs:   Python 3.9+, fonttools (pip install fonttools); uharfbuzz is optional (better kerning).
Fonts:   OFL fonts are fetched from raw.githubusercontent.com/google/fonts into a cache directory
         ($OML_FONT_CACHE, default ~/.cache/oml-brand-fonts) on first run, then instanced to the
         static weights used here. Every piece of text is outlined to paths; no <text> is emitted.
Writes:  oml-mark.svg, oml-mark-light.svg, oml-mark-mono.svg, oml-icon.svg, favicon.svg,
         oml-lockup-dark.svg, oml-lockup-light.svg, oml-wordmark-dark.svg, oml-wordmark-light.svg,
         readme-hero-dark.svg, readme-hero-light.svg, social-preview.svg, tokens.md.
         PNGs (social-preview.png, oml-icon-512.png) come from render.js.
"""
import math, os, sys, random, urllib.request
from fontTools.ttLib import TTFont
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen
from fontTools.varLib import instancer
try:
    import uharfbuzz as hb
except ImportError:
    hb = None

if len(sys.argv) < 2:
    sys.exit("usage: python3 gen.py OUT_DIR")
OUT = sys.argv[1]
os.makedirs(OUT, exist_ok=True)
CACHE = os.environ.get("OML_FONT_CACHE", os.path.join(os.path.expanduser("~"), ".cache", "oml-brand-fonts"))
os.makedirs(CACHE, exist_ok=True)

RAW = "https://raw.githubusercontent.com/google/fonts/main/ofl/"
SOURCES = {  # variable OFL sources
    "newsreader": ("newsreader/Newsreader%5Bopsz,wght%5D.ttf", "Newsreader-VF.ttf"),
    "inter": ("inter/Inter%5Bopsz,wght%5D.ttf", "Inter-VF.ttf"),
    "jbmono": ("jetbrainsmono/JetBrainsMono%5Bwght%5D.ttf", "JetBrainsMono-VF.ttf"),
}
STATIC = {  # key: (source, axis location)
    "disp":  ("newsreader", {"opsz": 72, "wght": 600}),   # Newsreader Display SemiBold
    "sans":  ("inter", {"opsz": 14, "wght": 400}),
    "sansM": ("inter", {"opsz": 14, "wght": 500}),
    "sansS": ("inter", {"opsz": 14, "wght": 600}),
    "mono":  ("jbmono", {"wght": 500}),
}

def _source(name):
    rel, fn = SOURCES[name]
    p = os.path.join(CACHE, fn)
    if not os.path.exists(p):
        print("downloading", fn, file=sys.stderr)
        with urllib.request.urlopen(RAW + rel, timeout=60) as r:
            data = r.read()
        with open(p + ".part", "wb") as f:
            f.write(data)
        os.replace(p + ".part", p)
    return p

def _static(key):
    src, loc = STATIC[key]
    tag = "-".join(f"{k}{v}" for k, v in sorted(loc.items()))
    p = os.path.join(CACHE, f"{src}-{tag}.ttf")
    if not os.path.exists(p):
        vf = TTFont(_source(src))
        axes = {a.axisTag for a in vf["fvar"].axes}
        inst = instancer.instantiateVariableFont(vf, {k: v for k, v in loc.items() if k in axes})
        inst.save(p + ".part")
        os.replace(p + ".part", p)
    return p

F = {k: None for k in STATIC}
_cache = {}

def _font(key):
    if key not in _cache:
        p = F[key] or _static(key)
        tt = TTFont(p)
        hbfont = None
        if hb:
            face = hb.Face(hb.Blob.from_file_path(p))
            hbfont = hb.Font(face)
        _cache[key] = (tt, tt.getGlyphSet(), tt.getGlyphOrder(), hbfont, tt["head"].unitsPerEm)
    return _cache[key]

def _shape(key, text):
    tt, gs, order, font, upem = _font(key)
    if font is not None:
        buf = hb.Buffer(); buf.add_str(text); buf.guess_segment_properties()
        hb.shape(font, buf, {"kern": True, "liga": True})
        return [(order[i.codepoint], p.x_advance, p.x_offset, p.y_offset) for i, p in zip(buf.glyph_infos, buf.glyph_positions)]
    cmap = tt.getBestCmap(); hmtx = tt["hmtx"]
    out = []
    for ch in text:
        n = cmap.get(ord(ch), ".notdef")
        out.append((n, hmtx[n][0], 0, 0))
    return out

def measure(key, text, size, tracking=0.0):
    return text_d(key, text, size, 0, 0, tracking=tracking)[1]

def text_d(key, text, size, x, y, anchor="start", tracking=0.0):
    tt, gs, order, font, upem = _font(key)
    glyphs = _shape(key, text)
    s = size / upem
    width = sum(g[1] for g in glyphs) * s + tracking * max(len(glyphs) - 1, 0)
    if anchor == "middle": x -= width / 2
    elif anchor == "end": x -= width
    pen = SVGPathPen(gs)
    cx = x
    for name, adv, xo, yo in glyphs:
        gs[name].draw(TransformPen(pen, (s, 0, 0, -s, cx + xo * s, y - yo * s)))
        cx += adv * s + tracking
    return pen.getCommands(), width

def T(key, text, size, x, y, fill, anchor="start", tracking=0.0, opacity=None):
    d, _ = text_d(key, text, size, x, y, anchor, tracking)
    op = f' fill-opacity="{opacity}"' if opacity is not None else ""
    return f'<path d="{d}" fill="{fill}"{op}/>'

def esc(t):
    return t.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")

def write(name, svg, title, desc):
    """Every SVG gets role, aria-label, <title> and <desc> right after the root tag."""
    i = svg.index(">") + 1
    root = svg[:i - 1] + f' role="img" aria-label="{esc(title)}">'
    svg = root + f"<title>{esc(title)}</title><desc>{esc(desc)}</desc>" + svg[i:]
    assert "<text" not in svg
    with open(os.path.join(OUT, name), "w", encoding="utf-8") as f:
        f.write(svg)

# ---------------------------------------------------------------- palette and drawing
THEMES = {
    "dark": dict(bg="#0A0D22", fg="#EFE9DA", muted="#AEB6D2", star="#F6A83E", core="#FFE2B0",
                 lab="#9FC6E6", labDim="#5F7FA3", faint="#2E3664", glow=0.30, vig=0.55, starfield=True,
                 halo="#0A0D22"),
    "light": dict(bg="#F1EBDC", fg="#10153A", muted="#414872", star="#C4700D", core="#F2AA4C",
                  lab="#2F6690", labDim="#7C97B4", faint="#C4BDAE", glow=0.20, vig=0.0, starfield=False,
                  halo="#F1EBDC"),
}
PAL = {"night": "#0A0D22", "star": "#F6A83E", "ice": "#9FC6E6", "paper": "#EFE9DA",
       "starPaper": "#C4700D", "steel": "#2F6690", "paperBg": "#F1EBDC", "inkText": "#10153A"}


def Th(th, key, text, size, x, y, fill, anchor="start", tracking=0.0, halo=5):
    """Outlined text with a halo in the ground colour, so it keeps clear of lines."""
    d, _ = text_d(key, text, size, x, y, anchor, tracking)
    h = (f'<path d="{d}" fill="none" stroke="{th["halo"]}" stroke-width="{halo}" stroke-linejoin="round" '
         f'stroke-opacity="0.92"/>') if halo else ""
    return h + f'<path d="{d}" fill="{fill}"/>'

def cap(th, key, text, size, x, y, fill, anchor="start", halo=5):
    return Th(th, key, text.upper(), size, x, y, fill, anchor, size * 0.11, halo)

# ---------------------------------------------------------------- mark
def mark_group(x, y, size, ring, star, body, sw=6.4, comp_r=5.6, star_r=11):
    """A full orbit that is also the O of Open: star off-centre inside, companion sitting in a gap."""
    k = size / 64.0
    R, gap_c, gap = 24, -40, 26  # gap centre angle and half-width (deg)
    a0, a1 = math.radians(gap_c + gap), math.radians(gap_c - gap + 360)
    p0 = (32 + R * math.cos(a0), 32 + R * math.sin(a0))
    p1 = (32 + R * math.cos(a1), 32 + R * math.sin(a1))
    ringp = (f'<path d="M{p0[0]:.2f} {p0[1]:.2f} A{R} {R} 0 1 1 {p1[0]:.2f} {p1[1]:.2f}" fill="none" '
             f'stroke="{ring}" stroke-width="{sw}" stroke-linecap="round"/>')
    bc = (32 + R * math.cos(math.radians(gap_c)), 32 + R * math.sin(math.radians(gap_c)))
    comp = f'<circle cx="{bc[0]:.2f}" cy="{bc[1]:.2f}" r="{comp_r}" fill="{body}"/>'
    st = f'<circle cx="28.4" cy="35.4" r="{star_r}" fill="{star}"/>'
    return f'<g transform="translate({x} {y}) scale({k:.5f})">{ringp}{st}{comp}</g>'

def wordmark(th, x, y, size):
    ms = size * 1.18
    g = [mark_group(x, y - ms * 0.80, ms, th["lab"], th["star"], th["fg"])]
    g.append(T("disp", "Open Memory Lab", size, x + ms + size * 0.26, y, th["fg"], tracking=-size * 0.005))
    return "".join(g)

# ---------------------------------------------------------------- system
TILT = math.radians(-10)
O1, O2, O3 = (150, 62), (252, 104), (360, 150)

def P(cx, cy, rx, ry, deg, s=1.0):
    a = math.radians(deg)
    x, y = rx * math.cos(a) * s, ry * math.sin(a) * s
    return cx + x * math.cos(TILT) - y * math.sin(TILT), cy + x * math.sin(TILT) + y * math.cos(TILT)

def ell(cx, cy, rx, ry, s, stroke, w, extra=""):
    return (f'<ellipse cx="{cx:.1f}" cy="{cy:.1f}" rx="{rx*s:.2f}" ry="{ry*s:.2f}" fill="none" stroke="{stroke}" '
            f'stroke-width="{w}" transform="rotate({math.degrees(TILT):.2f} {cx:.1f} {cy:.1f})" {extra}/>')

def arc_pts(cx, cy, rx, ry, s, a0, a1, n=60):
    pts = [P(cx, cy, rx, ry, a0 + (a1 - a0) * i / n, s) for i in range(n + 1)]
    return "M" + " L".join(f"{x:.2f} {y:.2f}" for x, y in pts)

def atmosphere(th, W, H, cx, cy, s, uid, avoid, n=26, seed=11):
    g = []
    gr = 430 * s
    g.append(f'<radialGradient id="{uid}glow" cx="{cx}" cy="{cy}" r="{gr:.0f}" gradientUnits="userSpaceOnUse">'
             f'<stop offset="0" stop-color="{th["star"]}" stop-opacity="{th["glow"]}"/>'
             f'<stop offset="0.28" stop-color="{th["star"]}" stop-opacity="{th["glow"]*0.38:.3f}"/>'
             f'<stop offset="1" stop-color="{th["star"]}" stop-opacity="0"/></radialGradient>')
    g.append(f'<rect width="{W}" height="{H}" fill="url(#{uid}glow)"/>')
    if th["starfield"]:
        rnd = random.Random(seed)
        pts = []
        tries = 0
        while len(pts) < n and tries < 2000:
            tries += 1
            x, y = rnd.uniform(8, W - 8), rnd.uniform(8, H - 8)
            if any(a[0] <= x <= a[2] and a[1] <= y <= a[3] for a in avoid):
                continue
            if any((x - p[0]) ** 2 + (y - p[1]) ** 2 < 70 ** 2 for p in pts):
                continue
            pts.append((x, y))
        for x, y in pts:
            r = rnd.uniform(0.6, 1.25); op = rnd.uniform(0.22, 0.5)
            g.append(f'<circle cx="{x:.1f}" cy="{y:.1f}" r="{r:.2f}" fill="{th["fg"]}" opacity="{op:.2f}"/>')
    return "".join(g)

def vignette(th, W, H, uid):
    if not th["vig"]:
        return ""
    return (f'<radialGradient id="{uid}vig" cx="0.55" cy="0.5" r="0.75">'
            f'<stop offset="0.55" stop-color="#000" stop-opacity="0"/>'
            f'<stop offset="1" stop-color="#000" stop-opacity="{th["vig"]}"/></radialGradient>'
            f'<rect width="{W}" height="{H}" fill="url(#{uid}vig)"/>')

def system(th, cx, cy, s, uid, full=True):
    """Returns (svg, anchors) for the star system."""
    g = []
    ST, LAB, FG, BG = th["star"], th["lab"], th["fg"], th["bg"]
    w = max(2.4 * s, 1.6)
    # construction axis (faint)
    ax = 400 * s
    g.append(f'<path d="M{cx-ax*math.cos(TILT):.1f} {cy-ax*math.sin(TILT):.1f} L{cx+ax*math.cos(TILT):.1f} {cy+ax*math.sin(TILT):.1f}" '
             f'stroke="{th["faint"]}" stroke-width="1" stroke-dasharray="14 5 2 5"/>')
    # forming disk beyond O3: growing
    g.append(f'<path d="{arc_pts(cx, cy, O3[0]+30, O3[1]+13, s, -95, 70)}" fill="none" stroke="{LAB}" '
             f'stroke-width="{w*0.85:.2f}" stroke-dasharray="{14*max(s,.7):.1f} {10*max(s,.7):.1f}" stroke-linecap="round" opacity="0.7"/>')
    # orbits (lab, cool)
    g.append(ell(cx, cy, *O3, s, LAB, w * 0.85, 'opacity="0.85"'))
    g.append(ell(cx, cy, *O2, s, LAB, w))
    g.append(ell(cx, cy, *O1, s, LAB, w))
    # star (foundation, warm)
    rs = 46 * s
    g.append(f'<radialGradient id="{uid}core" cx="0.42" cy="0.40" r="0.65"><stop offset="0" stop-color="{th["core"]}"/>'
             f'<stop offset="1" stop-color="{ST}"/></radialGradient>')
    g.append(f'<circle cx="{cx:.1f}" cy="{cy:.1f}" r="{rs:.2f}" fill="url(#{uid}core)"/>')
    an = {"star": (cx, cy, rs)}
    # vocab planet on O1, dataset planet on O2
    A = P(cx, cy, *O1, 196, s)
    B = P(cx, cy, *O2, 18, s)
    ra = 11 * s
    g.append(f'<circle cx="{A[0]:.1f}" cy="{A[1]:.1f}" r="{ra+3*s:.1f}" fill="{BG}"/>')
    g.append(f'<circle cx="{A[0]:.1f}" cy="{A[1]:.1f}" r="{ra:.1f}" fill="{LAB}"/>')
    rb = 18 * s
    g.append(f'<circle cx="{B[0]:.1f}" cy="{B[1]:.1f}" r="{rb+3*s:.1f}" fill="{BG}"/>')
    g.append(f'<circle cx="{B[0]:.1f}" cy="{B[1]:.1f}" r="{rb:.1f}" fill="{LAB}"/>')
    an["A"], an["B"], an["rb"], an["ra"] = A, B, rb, ra
    # fenced experiment, its own closed boundary in the gap between O2 and O3
    Fc = P(cx, cy, (O2[0] + O3[0]) / 2, (O2[1] + O3[1]) / 2, 146, s)
    fr = 27 * s
    g.append(f'<circle cx="{Fc[0]:.1f}" cy="{Fc[1]:.1f}" r="{fr+6*s:.1f}" fill="{BG}"/>')
    g.append(f'<circle cx="{Fc[0]:.1f}" cy="{Fc[1]:.1f}" r="{fr:.1f}" fill="none" stroke="{LAB}" stroke-width="{2.2*max(s,.8):.2f}" stroke-dasharray="{6*max(s,.7):.1f} {4.5*max(s,.7):.1f}"/>')
    g.append(f'<circle cx="{Fc[0]:.1f}" cy="{Fc[1]:.1f}" r="{6*s:.1f}" fill="{FG}"/>')
    ma = math.radians(-40)
    g.append(f'<circle cx="{Fc[0]+15*s*math.cos(ma):.1f}" cy="{Fc[1]+15*s*math.sin(ma):.1f}" r="{3.6*s:.1f}" fill="{LAB}"/>')
    an["F"], an["fr"] = Fc, fr
    # second look: newcomer waits at the entry point on O3; companion sights it.
    E = P(cx, cy, *O3, -64, s)
    re = 10 * s
    g.append(f'<circle cx="{E[0]:.1f}" cy="{E[1]:.1f}" r="{re+4*s:.1f}" fill="{BG}"/>')
    g.append(f'<circle cx="{E[0]:.1f}" cy="{E[1]:.1f}" r="{re:.1f}" fill="{BG}" stroke="{FG}" stroke-width="{2.4*max(s,.8):.2f}"/>')
    # reticle brackets around the newcomer
    h, l = 22 * s, 8 * s
    br = []
    for sx in (-1, 1):
        for sy in (-1, 1):
            x0, y0 = E[0] + sx * h, E[1] + sy * h
            br.append(f"M{x0:.1f} {y0 - sy*l:.1f} L{x0:.1f} {y0:.1f} L{x0 - sx*l:.1f} {y0:.1f}")
    g.append(f'<path d="{" ".join(br)}" fill="none" stroke="{ST}" stroke-width="{2.2*max(s,.8):.2f}" stroke-linecap="square"/>')
    # companion star (binary companion to the foundation) and its single line of sight
    C = (cx - 120 * s, cy - 222 * s)
    rc = 9 * s
    dx, dy = E[0] - C[0], E[1] - C[1]
    dl = math.hypot(dx, dy); ux, uy = dx / dl, dy / dl
    s0 = (C[0] + ux * (rc + 10 * s), C[1] + uy * (rc + 10 * s))
    s1 = (E[0] - ux * (h + 6 * s), E[1] - uy * (h + 6 * s))
    g.append(f'<path d="M{s0[0]:.1f} {s0[1]:.1f} L{s1[0]:.1f} {s1[1]:.1f}" stroke="{ST}" stroke-width="{1.8*max(s,.8):.2f}"/>')
    # arrow tip
    px_, py_ = -uy, ux
    tip = s1; bl = 9 * s
    g.append(f'<path d="M{tip[0]:.1f} {tip[1]:.1f} L{tip[0]-ux*bl+px_*bl*0.45:.1f} {tip[1]-uy*bl+py_*bl*0.45:.1f} '
             f'L{tip[0]-ux*bl-px_*bl*0.45:.1f} {tip[1]-uy*bl-py_*bl*0.45:.1f} Z" fill="{ST}"/>')
    g.append(f'<circle cx="{C[0]:.1f}" cy="{C[1]:.1f}" r="{rc+6*s:.1f}" fill="none" stroke="{ST}" stroke-width="{1.3*max(s,.8):.2f}"/>')
    g.append(f'<circle cx="{C[0]:.1f}" cy="{C[1]:.1f}" r="{rc:.1f}" fill="{ST}"/>')
    an["E"], an["C"], an["h"], an["rc"] = E, C, h, rc
    return "".join(g), an

def atlas_marks(th, x0, y0, x1, y1, caption, lab=12, step=18, cap_top=False):
    """Corner ticks, a graduated bottom edge and a plate caption: no full box."""
    c, L = th["muted"], th["labDim"]
    g = []
    k = 26
    for (x, y, sx, sy) in [(x0, y0, 1, 1), (x0, y1, 1, -1)]:
        g.append(f'<path d="M{x} {y+sy*k} L{x} {y} L{x+sx*k} {y}" fill="none" stroke="{L}" stroke-width="1.4"/>')
    t = []
    i = 0; xx = x0 + 40
    while xx < x1:
        ln = 9 if i % 5 == 0 else 4
        t.append(f"M{xx:.1f} {y1} L{xx:.1f} {y1-ln}")
        xx += step; i += 1
    g.append(f'<path d="M{x0+40} {y1} L{x1} {y1}" stroke="{L}" stroke-width="1"/>')
    g.append(f'<path d="{" ".join(t)}" stroke="{L}" stroke-width="1"/>')
    i = 0; yy = y0 + 40
    t = []
    while yy < y1 - 30:
        ln = 9 if i % 5 == 0 else 4
        t.append(f"M{x0} {yy:.1f} L{x0+ln} {yy:.1f}")
        yy += step; i += 1
    g.append(f'<path d="{" ".join(t)}" stroke="{L}" stroke-width="1"/>')
    g.append(Th(th, "mono", caption, lab, x1 - 24, (y0 + 22) if cap_top else (y1 - 16), c, "end", lab * 0.16, halo=4))
    return "".join(g)

# ---------------------------------------------------------------- copy
TAG = ["Steady at the centre.", "Forming at the edges."]
PROPOSAL = "Shared memory for AI agents and crews. Nothing joins until someone other than its writer has looked, and every decision stays on record."

def wrap(key, text, size, maxw):
    words, lines, cur = text.split(), [], ""
    for wd in words:
        trial = (cur + " " + wd).strip()
        if measure(key, trial, size) > maxw and cur:
            lines.append(cur); cur = wd
        else:
            cur = trial
    lines.append(cur)
    return lines


def obstacles(cx, cy, s, an, extra_rects=()):
    pts = []
    for (rx, ry) in (O1, O2, O3):
        pts += [P(cx, cy, rx, ry, a / 2, s) for a in range(720)]
    pts += [P(cx, cy, O3[0] + 30, O3[1] + 13, -95 + a * 0.25, s) for a in range(661)]
    C, E, h = an["C"], an["E"], an["h"]
    pts += [(C[0] + (E[0] - C[0]) * t / 200, C[1] + (E[1] - C[1]) * t / 200) for t in range(201)]
    circles = [(an["star"][0], an["star"][1], an["star"][2] + 10), (*an["A"], an["ra"] + 4), (*an["B"], an["rb"] + 4),
               (*an["F"], an["fr"] + 4), (E[0], E[1], h * 1.45), (C[0], C[1], an["rc"] + 10)]
    return pts, circles, list(extra_rects)

def place(th, obs, bounds, key, text, size, ax, ay, ar, fill, prefer=None, clear=9):
    """Put a caps label near (ax, ay) where it touches no orbit line; leader line if it sits far off."""
    pts, circles, rects = obs
    tr = size * 0.11
    w = measure(key, text.upper(), size, tr); hgt = size * 0.74
    best = None
    angles = (prefer or []) + [a for a in range(0, 360, 15) if a not in (prefer or [])]
    for dist in range(int(ar + 8), int(ar + 280), 6):
        for ang in angles:
            a = math.radians(ang)
            px, py = ax + dist * math.cos(a), ay + dist * math.sin(a)
            c = math.cos(a)
            if c > 0.35: x0 = px
            elif c < -0.35: x0 = px - w
            else: x0 = px - w / 2
            sn = math.sin(a)
            y1 = py + hgt if sn > 0.35 else (py if sn < -0.35 else py + hgt / 2)
            y0 = y1 - hgt
            box = (x0 - clear, y0 - clear, x0 + w + clear, y1 + clear)
            if box[0] < bounds[0] or box[2] > bounds[2] or box[1] < bounds[1] or box[3] > bounds[3]:
                continue
            if any(box[0] <= qx <= box[2] and box[1] <= qy <= box[3] for qx, qy in pts):
                continue
            bad = False
            for (ccx, ccy, cr) in circles:
                nx, ny = min(max(ccx, box[0]), box[2]), min(max(ccy, box[1]), box[3])
                if (nx - ccx) ** 2 + (ny - ccy) ** 2 < cr ** 2:
                    bad = True; break
            if bad or any(not (box[2] < r[0] or box[0] > r[2] or box[3] < r[1] or box[1] > r[3]) for r in rects):
                continue
            best = (x0, y1, dist, a, w, hgt); break
        if best: break
    if not best:
        raise SystemExit("no place for " + text)
    x0, y1, dist, a, w, hgt = best
    rects.append((x0 - 6, y1 - hgt - 6, x0 + w + 6, y1 + 6))
    out = ""
    if dist > ar + 26:
        lx0, ly0 = ax + (ar + 4) * math.cos(a), ay + (ar + 4) * math.sin(a)
        lx1, ly1 = ax + (dist - 6) * math.cos(a), ay + (dist - 6) * math.sin(a)
        out += f'<path d="M{lx0:.1f} {ly0:.1f} L{lx1:.1f} {ly1:.1f}" stroke="{th["labDim"]}" stroke-width="1.2"/>'
        pts.extend((lx0 + (lx1 - lx0) * t / 60, ly0 + (ly1 - ly0) * t / 60) for t in range(61))
    return out + Th(th, key, text.upper(), size, x0, y1, fill, "start", tr, halo=0)

def social():
    th = THEMES["dark"]; W, H = 1280, 640
    cx, cy, s = 948, 352, 1.0
    g = [f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {H}" width="{W}" height="{H}">',
         f'<rect width="{W}" height="{H}" fill="{th["bg"]}"/>']
    g.append(atmosphere(th, W, H, cx, cy, s, "s", avoid=[(40, 120, 560, 580), (560, 150, 1280, 560)]))
    sysg, an = system(th, cx, cy, s, "s")
    g.append(atlas_marks(th, 572, 44, 1280, 604, "PLATE I · THE SYSTEM OF SHARED MEMORY", 12))
    g.append(sysg)
    # labels: each placed in clear space, away from every orbit line
    lab = 15; ST, LAB, FG, MU = th["star"], th["lab"], th["fg"], th["muted"]
    obs = obstacles(cx, cy, s, an, [(560, 560, 1280, 640)])
    bd = (575, 50, 1272, 596)
    X, Y, rs = an["star"]
    g.append(place(th, obs, bd, "sansS", "The foundation", 16, X, Y, rs + 10, ST, prefer=[90, 75, 105, 60, 120]))
    g.append(place(th, obs, bd, "sansS", "second look", 16, *an["C"], an["rc"] + 6, ST, prefer=[180, 165, 195, 150, 210, 270, 90]))
    g.append(place(th, obs, bd, "sansM", "waits to join", lab, *an["E"], an["h"] * 1.4, FG, prefer=[0, 15, 30, 345, 45, 60, 90, 270]))
    g.append(place(th, obs, bd, "sansM", "shared vocabularies", lab, *an["A"], an["ra"], LAB, prefer=[270, 255, 285, 240, 225, 210, 195, 180, 90]))
    g.append(place(th, obs, bd, "sansM", "whole datasets", lab, *an["B"], an["rb"], LAB, prefer=[90, 75, 105, 120, 60, 270, 180]))
    g.append(place(th, obs, bd, "sansM", "fenced experiments", lab, *an["F"], an["fr"], LAB, prefer=[90, 105, 75, 120, 135, 180]))
    gp = P(cx, cy, O3[0] + 30, O3[1] + 13, 40, s)
    g.append(place(th, obs, bd, "sansM", "growing", lab, *gp, 4, LAB, prefer=[90, 60, 120, 45, 135, 270]))
    # left column
    lx = 64
    g.append(wordmark(th, lx, 168, 36))
    hs = 58
    while measure("disp", TAG[1], hs) > 480: hs -= 1
    g.append(T("disp", TAG[0], hs, lx - 2, 268, ST))
    g.append(T("disp", TAG[1], hs, lx - 2, 268 + hs * 1.1, LAB))
    y0 = 268 + hs * 1.1 + 56
    for i, ln in enumerate(wrap("sans", PROPOSAL, 21, 450)):
        g.append(T("sans", ln, 21, lx, y0 + i * 31, MU))
    g.append(f'<path d="M{lx} 524 L{lx+56} 524" stroke="{ST}" stroke-width="2"/>')
    g.append(T("mono", "OPEN SOURCE · AI-FIRST, HUMAN IN THE LOOP", 13, lx, 552, FG, tracking=13 * 0.14))
    g.append(vignette(th, W, H, "s"))
    g.append("</svg>")
    return "".join(g)

def hero(mode):
    th = THEMES[mode]; W, H = 1280, 380
    cx, cy, s = 1000, 206, 0.64
    g = [f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {H}" width="{W}" height="{H}">',
         f'<rect width="{W}" height="{H}" fill="{th["bg"]}"/>']
    g.append(atmosphere(th, W, H, cx, cy, s, "h" + mode, avoid=[(30, 40, 640, 350), (680, 40, 1280, 360)], n=14))
    sysg, an = system(th, cx, cy, s, "h" + mode)
    g.append(atlas_marks(th, 690, 24, 1280, 360, "PLATE I · THE SYSTEM", 13, cap_top=True))
    g.append(sysg)
    ST, LAB, FG, MU = th["star"], th["lab"], th["fg"], th["muted"]
    lab = 19
    X, Y, rs = an["star"]
    # foundation: directly below the star, short vertical leader, below the outer orbit
    fy = 342
    fw = measure("sansS", "FOUNDATION", lab, lab * 0.11)
    g.append(f'<path d="M{X:.1f} {Y+rs+5:.1f} L{X:.1f} {fy-lab*0.74-7:.1f}" stroke="{th["labDim"]}" stroke-width="1.2"/>')
    g.append(Th(th, "sansS", "FOUNDATION", lab, X, fy, ST, "middle", lab * 0.11, halo=0))
    obs = obstacles(cx, cy, s, an, [(1040, 24, 1280, 48), (X - fw / 2 - 8, fy - lab, X + fw / 2 + 8, fy + 6)])
    obs[0].extend((X, y) for y in range(int(Y + rs), int(fy - 14), 2))
    bd = (696, 30, 1274, 352)
    g.append(place(th, obs, bd, "sansS", "second look", lab, *an["C"], an["rc"] + 6, ST, prefer=[180, 165, 195, 150, 210], clear=6))
    g.append(place(th, obs, bd, "sansS", "fenced", lab, *an["F"], an["fr"], LAB, prefer=[180, 165, 195, 150, 210, 135, 90], clear=6))
    gp = P(cx, cy, O3[0] + 30, O3[1] + 13, 40, s)
    g.append(place(th, obs, bd, "sansS", "growing", lab, *gp, 4, LAB, prefer=[90, 60, 120, 45, 30, 0, 135], clear=6))
    lx = 56
    g.append(wordmark(th, lx, 98, 36))
    g.append(T("disp", TAG[0], 48, lx - 2, 182, ST))
    g.append(T("disp", TAG[1], 48, lx - 2, 236, LAB))
    g.append(T("sans", "Shared memory for AI agents and crews.", 22, lx, 290, MU))
    g.append(T("sans", "Nothing joins until a second look.", 22, lx, 320, MU))
    g.append(vignette(th, W, H, "h" + mode))
    g.append("</svg>")
    return "".join(g)


# ---------------------------------------------------------------- kit pieces
SVGNS = 'xmlns="http://www.w3.org/2000/svg"'

def mark_plain(theme):
    th = THEMES[theme]
    return f'<svg {SVGNS} viewBox="0 0 64 64" width="64" height="64">' + mark_group(0, 0, 64, th["lab"], th["star"], th["fg"]) + '</svg>'

def mark_mono():
    return (f'<svg {SVGNS} viewBox="0 0 64 64" width="64" height="64">'
            + mark_group(0, 0, 64, "currentColor", "currentColor", "currentColor") + '</svg>')

def icon():
    th = THEMES["dark"]
    return (f'<svg {SVGNS} viewBox="0 0 64 64" width="512" height="512"><rect width="64" height="64" rx="14" fill="{th["bg"]}"/>'
            + mark_group(5, 5, 54, th["lab"], th["star"], th["fg"]) + '</svg>')

def favicon():
    """Same mark, tile edge cut down and strokes thickened so it holds at 16px."""
    th = THEMES["dark"]
    return (f'<svg {SVGNS} viewBox="0 0 64 64" width="32" height="32"><rect width="64" height="64" rx="12" fill="{th["bg"]}"/>'
            + mark_group(2, 2, 60, th["lab"], th["star"], th["fg"], sw=8.2, comp_r=6.6, star_r=11.6) + '</svg>')

def lockup(theme, tagline):
    th = THEMES[theme]
    size = 64
    ms = size * 1.18
    gap = size * 0.26
    tx = ms + gap
    ww = measure("disp", "Open Memory Lab", size, -size * 0.005)
    pad = 4
    base = ms * 0.80 + pad
    g = [mark_group(pad, base - ms * 0.80, ms, th["lab"], th["star"], th["fg"]),
         T("disp", "Open Memory Lab", size, pad + tx, base, th["fg"], tracking=-size * 0.005)]
    W = pad + tx + ww + pad
    H = base + size * 0.30
    if tagline:
        ts = size * 0.36
        a = TAG[0] + " "
        aw = measure("disp", a, ts)
        y2 = base + ts * 1.75
        g.append(T("disp", a, ts, pad + tx + 2, y2, th["star"]))
        g.append(T("disp", TAG[1], ts, pad + tx + 2 + aw, y2, th["lab"]))
        W = max(W, pad + tx + 2 + aw + measure("disp", TAG[1], ts) + pad)
        H = y2 + ts * 0.35
    W, H = math.ceil(W), math.ceil(H)
    return f'<svg {SVGNS} viewBox="0 0 {W} {H}" width="{W}" height="{H}">' + "".join(g) + '</svg>'

def contrast(a, b):
    def lum(h):
        c = [int(h[i:i + 2], 16) / 255 for i in (1, 3, 5)]
        c = [x / 12.92 if x <= 0.03928 else ((x + 0.055) / 1.055) ** 2.4 for x in c]
        return 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
    l1, l2 = sorted([lum(a), lum(b)], reverse=True)
    return (l1 + 0.05) / (l2 + 0.05)

def tokens_md():
    d, l = THEMES["dark"], THEMES["light"]
    def row(name, role, th, key, deco=False):
        r = contrast(th[key], th["bg"])
        if deco:
            return f"| {name} | `{th[key]}` | {role} | {r:.1f}:1 | decorative lines only, never text |"
        grade = "AAA" if r >= 7 else "AA" if r >= 4.5 else "AA large (18px+ / 14px+ bold)" if r >= 3 else "graphics only"
        return f"| {name} | `{th[key]}` | {role} | {r:.1f}:1 | {grade} |"
    out = ["# Open Memory Lab design tokens", "",
           "Direction: Atlas, cosmic. A stellar system on an engraved atlas plate. The foundation is the star at the centre;",
           "the lab is the system forming around it.", "",
           "## What each colour means", "",
           "- **Warm (star)** is the foundation and only the foundation: the star, its glow, the companion star that gives the",
           "  second look, the line of sight and the bracket around a body waiting to join. Never use warm for lab features.",
           "- **Ice (dark) / steel (light)** is the lab: orbits, planets (shared vocabularies, whole datasets), the fenced",
           "  experiment and its dashed fence, and the second line of the tagline.",
           "- **The dashed outer arc is the forming disk: growing.** It stands for lab work that is designed but not built yet.",
           "  Draw it dashed, never solid.",
           "- **Night / paper** are grounds; **text** and **muted** are for words; **line** and **faint** are for leaders,",
           "  construction lines and plate ticks only.", "",
           "## Dark (primary)", "",
           f"Ground `{d['bg']}`, with a warm radial glow from the star (star colour at {int(d['glow']*100)}% at the centre, fading to 0) and a soft vignette.", "",
           "| Token | Hex | Role | Contrast on ground | Use |", "|---|---|---|---|---|",
           row("text", "Headlines, wordmark, body", d, "fg"),
           row("muted", "Proposal and secondary copy", d, "muted"),
           row("star", "Foundation; first tagline line; foundation labels", d, "star"),
           row("ice", "Lab; second tagline line; lab labels", d, "lab"),
           row("line", "Leader lines, plate ticks", d, "labDim", True),
           f"| faint | `{d['faint']}` | Construction axis | n/a | graphics only |", "",
           "## Light", "",
           f"Ground `{l['bg']}` (star-chart paper), with a fainter warm glow and no vignette.", "",
           "| Token | Hex | Role | Contrast on ground | Use |", "|---|---|---|---|---|",
           row("text", "Headlines, wordmark, body", l, "fg"),
           row("muted", "Proposal and secondary copy", l, "muted"),
           row("star (on paper)", "Foundation; first tagline line; foundation labels", l, "star"),
           row("steel", "Lab; second tagline line; lab labels", l, "lab"),
           row("line", "Leader lines, plate ticks", l, "labDim", True),
           f"| faint | `{l['faint']}` | Construction axis | n/a | graphics only |", "",
           "Warm on paper passes only as large text (3:1), so set warm words on paper at 18px or above, or 14px bold and above. "
           "(Hardened in the kit: the v3 mockup value #C8730F was 2.98:1 and is nudged to #C4700D.)", "",
           "## Type", "",
           "| Family | Weight | Role |", "|---|---|---|",
           "| Newsreader Display (OFL), optical size 72 | SemiBold 600 | Wordmark, headlines, tagline |",
           "| Inter (OFL), optical size 14 | Regular 400, Medium 500, SemiBold 600 | Body copy (Regular); plate labels in caps with +11% tracking (Medium, SemiBold for foundation and second look) |",
           "| JetBrains Mono (OFL) | Medium 500 | Plate captions and small caps lines, +14 to 16% tracking |", "",
           "All text in the kit is outlined to paths. Headline line one is star, line two is ice/steel.", "",
           "## Mark", "",
           "A full orbit ring that is also the O of Open (lab colour), the warm star off-centre inside it, and a companion",
           "body sitting in a small gap in the ring. Clear space: one ring stroke width... use at least 25% of the mark's",
           "width on every side. Minimum size: 16px (use favicon.svg, which has thicker strokes, below 24px).", "",
           "## Files", "",
           "| File | Use |", "|---|---|",
           "| oml-mark.svg | Mark on dark grounds |",
           "| oml-mark-light.svg | Mark on light grounds |",
           "| oml-mark-mono.svg | One colour, follows `currentColor` |",
           "| oml-icon.svg, oml-icon-512.png | App tile / avatar on the night ground; works in light and dark UIs |",
           "| favicon.svg | Favicon tile, strokes tuned for 16px |",
           "| oml-lockup-dark.svg / -light.svg | Mark, wordmark and tagline |",
           "| oml-wordmark-dark.svg / -light.svg | Mark and wordmark, no tagline |",
           "| readme-hero-dark.svg / -light.svg | 1280x380 README banner; pair them with a `<picture>` element and `prefers-color-scheme` so each theme gets its own |",
           "| social-preview.svg / .png | 1280x640 link preview |", ""]
    return "\n".join(out).replace("Clear space: one ring stroke width... use at least", "Clear space: at least")

if __name__ == "__main__":
    D = "Open Memory Lab mark: a full orbit ring forming the O of Open, a warm star off-centre inside it, and a companion body in a gap of the ring."
    write("oml-mark.svg", mark_plain("dark"), "Open Memory Lab mark (for dark backgrounds)", D)
    write("oml-mark-light.svg", mark_plain("light"), "Open Memory Lab mark (for light backgrounds)", D)
    write("oml-mark-mono.svg", mark_mono(), "Open Memory Lab mark (one colour)", D + " Drawn in currentColor.")
    write("oml-icon.svg", icon(), "Open Memory Lab icon", D + " On a night-indigo rounded tile.")
    write("favicon.svg", favicon(), "Open Memory Lab", D + " Favicon tile tuned for 16px.")
    for mode, ground in (("dark", "dark"), ("light", "light")):
        write(f"oml-lockup-{mode}.svg", lockup(mode, True), "Open Memory Lab",
              f"Open Memory Lab logo lockup for {ground} backgrounds: the mark, the wordmark, and the tagline: Steady at the centre. Forming at the edges.")
        write(f"oml-wordmark-{mode}.svg", lockup(mode, False), "Open Memory Lab",
              f"Open Memory Lab logo for {ground} backgrounds: the mark and the wordmark.")
    HD = ("Open Memory Lab. Steady at the centre. Forming at the edges. Shared memory for AI agents and crews. Nothing joins until a second look. "
          "Illustration: a star system on an atlas plate. The foundation is the star; a companion star gives a second look at a new body waiting to join an orbit; "
          "a fenced experiment circles inside its own boundary; a dashed outer arc is growing.")
    write("readme-hero-dark.svg", hero("dark"), "Open Memory Lab", HD)
    write("readme-hero-light.svg", hero("light"), "Open Memory Lab", HD)
    write("social-preview.svg", social(), "Open Memory Lab: Steady at the centre. Forming at the edges.",
          "Open Memory Lab. Steady at the centre. Forming at the edges. " + PROPOSAL + " Open source, AI-first, human in the loop. "
          "Illustration: the foundation as a star with shared vocabularies and whole datasets on orbits, a fenced experiment, a growing outer arc, "
          "and a companion star giving a second look to a body waiting to join.")
    with open(os.path.join(OUT, "tokens.md"), "w", encoding="utf-8") as f:
        f.write(tokens_md())
    print("ok", OUT)
