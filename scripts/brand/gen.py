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

def C(dark):
    """Theme-resolved colours for the story art."""
    if dark:
        return dict(bg=P["void"], fg=P["star"], sub=P["dust"], faint=P["faint"], line=P["line"],
                    gold=P["gold"], teal=P["teal"], green=P["green"], violet=P["violet"], node=P["panel"])
    return dict(bg=P["paper"], fg=P["ink"], sub=P["ink2"], faint="#8C96B8", line=P["lline"],
                gold=P["gold_d"], teal=P["teal_d"], green="#1F9D57", violet=P["violet_d"], node="#FFFFFF")

def story(ox, oy, k, dark, uid, labels=True):
    """The path of one memory across a workarea, drawn in a 560x330 box at (ox, oy), scale k.
    Written by one agent -> held -> reviewed by someone who did not write it -> shared with the
    crew. Each step drops into the signed journal along the bottom. Other memories sit quietly
    in orbit so the one story reads first."""
    import math
    c = C(dark); r = random.Random(5)
    T = lambda x, y: (ox + x*k, oy + y*k)
    o = [f'<defs><marker id="ah{uid}" viewBox="0 0 10 10" refX="7" refY="5" markerWidth="{7*k:.1f}" markerHeight="{7*k:.1f}" '
         f'markerUnits="userSpaceOnUse" orient="auto"><path d="M1 1 L8 5 L1 9" fill="none" stroke="{c["sub"]}" stroke-width="1.6"/></marker></defs>']
    # workarea orbits (tilted ellipses) and quiet, already-shared memories
    cx, cy = T(320, 160)
    for rr in (250, 170, 95):
        o.append(f'<ellipse cx="{cx:.1f}" cy="{cy:.1f}" rx="{rr*k:.1f}" ry="{rr*0.42*k:.1f}" fill="none" stroke="{c["line"]}" '
                 f'stroke-width="{1.2*k:.2f}" transform="rotate(-12 {cx:.1f} {cy:.1f})"/>')
    for i, rr in enumerate((250, 170, 95)):
        for j in range(5 - i):
            a = r.uniform(0, 2*math.pi); x = rr*math.cos(a); y = rr*0.42*math.sin(a); th = math.radians(-12)
            X = 320 + x*math.cos(th) - y*math.sin(th); Y = 160 + x*math.sin(th) + y*math.cos(th)
            if 20 < X < 540 and 20 < Y < 275:
                px, py = T(X, Y); o.append(diamond(px, py, 7*k, c["teal"], 'opacity="0.28"'))
    # stations
    W, H, R, S = (70, 225), (175, 92), (355, 62), (365, 200)
    crew = [(470, 135), (505, 240), (255, 262)]
    def P2(p): return T(*p)
    # share lines to the crew
    for q in crew:
        a, b = P2(S), P2(q)
        o.append(f'<line x1="{a[0]:.1f}" y1="{a[1]:.1f}" x2="{b[0]:.1f}" y2="{b[1]:.1f}" stroke="{c["green"]}" stroke-width="{1.4*k:.2f}" opacity="0.55"/>')
        o.append(f'<circle cx="{b[0]:.1f}" cy="{b[1]:.1f}" r="{7*k:.1f}" fill="{c["node"]}" stroke="{c["sub"]}" stroke-width="{2*k:.2f}"/>')
    # the path of the one memory
    pts = [P2(W), P2(H), P2(R), P2(S)]
    ctrl = [T(80, 120), T(255, 40), T(440, 110)]
    d = f'M{pts[0][0]:.1f} {pts[0][1]:.1f}'
    for cc, pp in zip(ctrl, pts[1:]):
        d += f' Q{cc[0]:.1f} {cc[1]:.1f} {pp[0]:.1f} {pp[1]:.1f}'
    o.append(f'<path d="{d}" fill="none" stroke="{c["sub"]}" stroke-width="{1.8*k:.2f}" stroke-dasharray="{5*k:.1f} {5*k:.1f}"/>')
    # arrowheads mid-segment
    for (a, cc, b) in zip(pts[:-1], ctrl, pts[1:]):
        t = 0.55; x = (1-t)**2*a[0] + 2*(1-t)*t*cc[0] + t*t*b[0]; y = (1-t)**2*a[1] + 2*(1-t)*t*cc[1] + t*t*b[1]
        dx = 2*(1-t)*(cc[0]-a[0]) + 2*t*(b[0]-cc[0]); dy = 2*(1-t)*(cc[1]-a[1]) + 2*t*(b[1]-cc[1])
        ang = math.degrees(math.atan2(dy, dx))
        o.append(f'<path d="M{-5*k:.1f} {-5*k:.1f} L{3*k:.1f} 0 L{-5*k:.1f} {5*k:.1f}" fill="none" stroke="{c["sub"]}" '
                 f'stroke-width="{1.8*k:.2f}" transform="translate({x:.1f} {y:.1f}) rotate({ang:.1f})"/>')
    # journal: one signed block per step, linked
    jy = 300; xs = [70, 175, 355, 365+60]
    blocks = [40 + i*58 for i in range(9)]
    bx0, by = T(blocks[0], jy)
    for i, bxv in enumerate(blocks):
        bx, byy = T(bxv, jy)
        if i:
            px, _ = T(blocks[i-1] + 26, jy)
            o.append(f'<line x1="{px:.1f}" y1="{byy+7*k:.1f}" x2="{bx:.1f}" y2="{byy+7*k:.1f}" stroke="{c["violet"]}" stroke-width="{1.4*k:.2f}" opacity="0.7"/>')
        o.append(f'<rect x="{bx:.1f}" y="{byy:.1f}" width="{26*k:.1f}" height="{14*k:.1f}" rx="{3*k:.1f}" fill="{c["node"]}" stroke="{c["violet"]}" stroke-width="{1.2*k:.2f}" opacity="0.9"/>')
    # drop lines from each step into the journal
    for st, col, bi in ((W, c["gold"], 0), (H, c["gold"], 2), (R, c["teal"], 5), (S, c["green"], 6)):
        a = P2(st); bxv = blocks[bi]; b = T(bxv + 13, jy)
        o.append(f'<line x1="{a[0]:.1f}" y1="{a[1]+14*k:.1f}" x2="{b[0]:.1f}" y2="{b[1]:.1f}" stroke="{col}" stroke-width="{1.2*k:.2f}" stroke-dasharray="{2*k:.1f} {4*k:.1f}" opacity="0.6"/>')
        o.append(f'<rect x="{T(bxv, jy)[0]:.1f}" y="{b[1]:.1f}" width="{26*k:.1f}" height="{14*k:.1f}" rx="{3*k:.1f}" fill="{col}" opacity="0.9"/>')
    # station glyphs
    w = P2(W); o.append(f'<circle cx="{w[0]:.1f}" cy="{w[1]:.1f}" r="{11*k:.1f}" fill="{c["node"]}" stroke="{c["gold"]}" stroke-width="{3*k:.2f}"/>')
    h = P2(H); o.append(f'<circle cx="{h[0]:.1f}" cy="{h[1]:.1f}" r="{24*k:.1f}" fill="none" stroke="{c["gold"]}" stroke-width="{1.5*k:.2f}" stroke-dasharray="{4*k:.1f} {4*k:.1f}"/>')
    o.append(diamond(h[0], h[1], 17*k, c["gold"]))
    rv = P2(R); o.append(f'<circle cx="{rv[0]:.1f}" cy="{rv[1]:.1f}" r="{13*k:.1f}" fill="{c["node"]}" stroke="{c["teal"]}" stroke-width="{3*k:.2f}"/>')
    o.append(f'<path d="M{rv[0]-5.5*k:.1f} {rv[1]+0.5*k:.1f} L{rv[0]-1.5*k:.1f} {rv[1]+4.5*k:.1f} L{rv[0]+6*k:.1f} {rv[1]-4*k:.1f}" fill="none" stroke="{c["teal"]}" stroke-width="{2.6*k:.2f}" stroke-linecap="round" stroke-linejoin="round"/>')
    s = P2(S); o.append(f'<circle cx="{s[0]:.1f}" cy="{s[1]:.1f}" r="{28*k:.1f}" fill="{c["green"]}" opacity="0.14"/>')
    o.append(diamond(s[0], s[1], 21*k, c["green"]))
    if labels:
        L = lambda txt, x, y, col, anc="middle": tp("mono", txt, 14*k, *T(x, y), col, anchor=anc)[0]
        o.append(L("written", 70, 258, c["sub"]))
        o.append(L("held", 175, 56, c["gold"]))
        o.append(L("reviewed by someone else", 355, 30, c["teal"]))
        o.append(L("shared", 365, 247, c["green"]))
        o.append(L("every step signed", 560, 330, c["faint"], "end"))
    return "".join(o)

# ---------- README hero ----------
def hero(dark):
    W, Hh = 1280, 380; c = C(dark)
    defs = (f'<defs><radialGradient id="glow" cx="0.78" cy="0.45" r="0.55">'
            f'<stop offset="0" stop-color="{c["violet"]}" stop-opacity="{0.18 if dark else 0.08}"/>'
            f'<stop offset="1" stop-color="{c["bg"]}" stop-opacity="0"/></radialGradient></defs>')
    b = [defs, f'<rect width="{W}" height="{Hh}" rx="18" fill="{c["bg"]}"/>', f'<rect width="{W}" height="{Hh}" rx="18" fill="url(#glow)"/>']
    if not dark: b.append(f'<rect x="0.5" y="0.5" width="{W-1}" height="{Hh-1}" rx="17.5" fill="none" stroke="{P["lline"]}"/>')
    b.append(starfield(W, Hh, 70, 3, P["star"] if dark else "#8C96B8", x0=640))
    b.append(story(690, 22, 1.0, dark, "h"))
    b.append(mark_group(110, 118, 104, c["gold"], c["teal"], c["fg"]))
    t, _ = tp("disp-bold", "Open Memory Lab", 60, 186, 138, c["fg"], track=-0.015); b.append(t)
    t, _ = tp("medium", "Every memory is a claim under test.", 26, 64, 222, c["fg"]); b.append(t)
    for i, line in enumerate(["Shared memory for AI agents and crews. Nothing one agent",
                              "writes reaches the others until someone else has checked it."]):
        t, _ = tp("regular", line, 19, 64, 268 + i*28, c["sub"]); b.append(t)
    return svg(W, Hh, "".join(b), "Open Memory Lab: every memory is a claim under test",
               "Shared memory for AI agents and crews. A memory is written, held, reviewed by someone who did not write it, and only then shared; every step is signed.")
write("readme-hero-dark.svg", hero(True))
write("readme-hero-light.svg", hero(False))

# ---------- social preview 1280x640 ----------
def social():
    W, Hh = 1280, 640; dark = True; c = C(True)
    defs = (f'<defs><radialGradient id="glow" cx="0.76" cy="0.46" r="0.6">'
            f'<stop offset="0" stop-color="{c["violet"]}" stop-opacity="0.2"/><stop offset="1" stop-color="{c["bg"]}" stop-opacity="0"/></radialGradient></defs>')
    b = [defs, f'<rect width="{W}" height="{Hh}" fill="{c["bg"]}"/>', f'<rect width="{W}" height="{Hh}" fill="url(#glow)"/>',
         starfield(W, Hh, 110, 11, P["star"], x0=620), story(660, 140, 1.0, True, "s")]
    b.append(mark_group(124, 168, 96, c["gold"], c["teal"], c["fg"]))
    t, _ = tp("disp-bold", "Open Memory Lab", 68, 80, 302, c["fg"], track=-0.02); b.append(t)
    t, _ = tp("medium", "Every memory is a claim under test.", 31, 82, 356, c["fg"]); b.append(t)
    for i, line in enumerate(["Shared memory for AI agents and crews.", "Nothing one agent writes reaches the others",
                              "until someone else has checked it."]):
        t, _ = tp("regular", line, 23, 82, 410 + i*32, c["sub"]); b.append(t)
    t, _ = tp("mono", "github.com/e8kor/open-memory-lab", 17, 82, 560, c["faint"], track=0.02); b.append(t)
    return svg(W, Hh, "".join(b), "Open Memory Lab", "Every memory is a claim under test. Shared memory for AI agents and crews.")
write("social-preview.svg", social())
