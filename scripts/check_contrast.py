#!/usr/bin/env python3
"""Check every documented contrast pair in OML's design tokens (ADR-0069 d7).

    python scripts/check_contrast.py [tokens.json] [--table]

The design system's contrast script, unchanged in what it computes: WCAG 2 relative luminance,
-soft tints alpha-composited over the ground they sit on, 4.5:1 for text and 3:1 for borders,
marks and focus. APCA Lc is printed for the 12px state words as an advisory column only; it is
no pass/fail gate. Prints a summary (and with --table the markdown table); exits 1 when a pair
is below its floor. Standard library only.
"""
import argparse
import json
import sys
from pathlib import Path

DEFAULT = Path(__file__).resolve().parents[1] / "design" / "tokens.json"


def rgba(h):
    h = h.lstrip('#'); a = int(h[6:8], 16) / 255 if len(h) == 8 else 1
    return [int(h[i:i+2], 16) for i in (0, 2, 4)], a


def comp(top, under):
    (c, a), (u, _) = rgba(top), rgba(under)
    return '#' + ''.join(f'{round(c[i]*a + u[i]*(1-a)):02x}' for i in range(3))


def lum(h):
    c, _ = rgba(h)
    f = lambda x: x/12.92 if x <= 0.04045 else ((x+0.055)/1.055)**2.4  # noqa: E731
    r, g, b = [f(x/255) for x in c]; return 0.2126*r + 0.7152*g + 0.0722*b


def cr(a, b):
    la, lb = sorted([lum(a), lum(b)], reverse=True); return (la+0.05)/(lb+0.05)


def apca(fg, bg):  # APCA 0.0.98G-4g, advisory
    def y(h):
        c, _ = rgba(h); r, g, b = [(x/255)**2.4 for x in c]; Y = 0.2126729*r + 0.7151522*g + 0.0721750*b
        return Y if Y > 0.022 else Y + (0.022 - Y)**1.414
    t, s = y(fg), y(bg)
    if s > t: c = (s**0.56 - t**0.57) * 1.14; c = 0 if c < 0.1 else c - 0.027
    else: c = (s**0.65 - t**0.62) * 1.14; c = 0 if c > -0.1 else c + 0.027
    return round(c * 100, 1)


GROUNDS = ['ground', 'ground-alt', 'panel']
STATES = ['held', 'promoted', 'superseded', 'retired', 'rejected', 'in-progress', 'unreviewed', 'stale', 'self-signed']


def pairs(T):
    toks = {t['name']: t['value'] for t in T['color']['tokens']}
    assert not any(isinstance(v, str) and v.startswith('{') for t in toks.values() for v in (t.values() if isinstance(t, dict) else [t])), 'aliases are not used'

    def val(n, th):
        v = toks[n]; return v[th] if isinstance(v, dict) else v
    rows = []

    def add(fg, bg, need, th, fgv=None, bgv=None, note='', lc=False):
        f = fgv or val(fg, th); b = bgv or val(bg, th)
        rows.append((th, fg, bg, f, b, cr(f, b), need, note, apca(f, b) if lc else None))
    for th in ['dark', 'light']:
        for t in ['ink', 'ink-dim', 'ink-faint', 'star', 'ice']:
            for g in GROUNDS: add(t, g, 4.5, th)
        for t in ['edge', 'control-border', 'focus']:
            for g in GROUNDS: add(t, g, 3.0, th)
        for s in STATES:
            soft = s + '-soft' if s + '-soft' in toks else 'in-progress-soft'
            for g in GROUNDS:
                add(s, g, 4.5, th, lc=True)
                add(s, f'{soft} on {g}', 4.5, th, bgv=comp(val(soft, th), val(g, th)), note='chip word on its tint', lc=True)
                add(s, g, 3.0, th, note='lifecycle-bar segment or timeline glyph')
        add('on-star', 'star-fill', 4.5, th, note='primary button label; the current step number')
        add('on-error', 'error', 4.5, th)
        for g in ['ground', 'ground-alt']: add('star', g, 3.0, th, note='primary button border against the ground')
        for g in ['ground', 'ground-alt']:
            add('focus', g, 3.0, th, note='primary button focus ring: the 2px gap between ring and fill is ' + g)
        for g in ['ground-alt', 'panel']:
            add('control-border', g, 3.0, th, note='checkbox border, unchecked (2px, drawn by bundle.css, not the UA)')
            add('ice', g, 3.0, th, note='checkbox fill, checked')
        add('ground', 'ice', 3.0, th, note='the check mark on a checked box; a done step\'s tick')
        for i in range(1, 7):
            for g in GROUNDS:
                add(f'chart-{i}', g, 3.0, th, note='series mark')
                add(f'chart-{i}', g, 4.5, th, note='series colour used as text')
    return rows


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('tokens', nargs='?', default=str(DEFAULT))
    ap.add_argument('--table', action='store_true', help='print the markdown table')
    args = ap.parse_args(argv)
    rows = pairs(json.loads(Path(args.tokens).read_text()))
    fails = [r for r in rows if r[5] < r[6]]
    if args.table:
        print('| theme | foreground | background | fg | bg | ratio | needs | APCA Lc (advisory) |')
        print('|---|---|---|---|---|---|---|---|')
        for th, fg, bg, f, b, r, need, note, lc in rows:
            print(f'| {th} | `{fg}` | `{bg}` | {f} | {b} | {r:.2f}:1 | {need:g}:1{" FAIL" if r < need else ""} | {"" if lc is None else lc} |')
    mins = {}
    for th, fg, bg, f, b, r, need, note, lc in rows:
        k = (th, need); mins[k] = min(mins.get(k, 99), r)
    print(f'{len(rows)} pairs, {len(fails)} below their floor. Minimum per floor: ' + ', '.join(f'{k[0]} {k[1]:g}:1 floor -> {v:.2f}' for k, v in sorted(mins.items())), file=sys.stderr)
    for f in fails: print('FAIL', f, file=sys.stderr)
    return 1 if fails else 0


if __name__ == '__main__':
    sys.exit(main())
