#!/usr/bin/env python3
"""Generate 1:1 drill templates (SVG, units = mm) for the Hammond 1590F pedal case.

Run:  python3 docs/hardware/enclosure/gen_templates.py
Print each SVG at 100 % scale ("actual size", no fit-to-page), tape it to the
face, centre-punch through the crosshairs. Coordinates are defined in
docs/hardware/enclosure.md section 4. All views are looking AT the face from
OUTSIDE the box.
"""
import os

OUT = os.path.dirname(os.path.abspath(__file__))
S = 188.0          # outer face of a 1590F is 188 x 188 mm
H = 67.0           # outer depth incl. lid (wall faces are 188 x 67)
WALL_Z = 30.0      # side-wall hole centres: 30 mm below top-panel outer face

def svg(name, w, h, items, title):
    o = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{w}mm" height="{h+14}mm" viewBox="0 0 {w} {h+14}">',
         f'<text x="2" y="6" font-family="sans-serif" font-size="4">{title} - print at 100% (no scaling)</text>',
         f'<g transform="translate(0,10)"><rect x="0" y="0" width="{w}" height="{h}" fill="none" stroke="#000" stroke-width="0.4"/>']
    for kind, u, v, a, b, label in items:       # v already in SVG (down) coords
        if kind == 'circle':
            o.append(f'<circle cx="{u}" cy="{v}" r="{a/2}" fill="none" stroke="#c00" stroke-width="0.3"/>')
        elif kind == 'slot':   # a = length (u), b = width (v)
            o.append(f'<rect x="{u-a/2}" y="{v-b/2}" width="{a}" height="{b}" rx="{b/2}" fill="none" stroke="#c00" stroke-width="0.3"/>')
        elif kind == 'rect':
            o.append(f'<rect x="{u-a/2}" y="{v-b/2}" width="{a}" height="{b}" rx="3" fill="none" stroke="#c00" stroke-width="0.3"/>')
        o.append(f'<path d="M{u-3} {v}H{u+3}M{u} {v-3}V{v+3}" stroke="#06c" stroke-width="0.2"/>')
        if label:
            o.append(f'<text x="{u+2}" y="{v-2}" font-family="sans-serif" font-size="2.6" fill="#06c">{label}</text>')
    o.append('</g></svg>')
    open(os.path.join(OUT, name), 'w').write('\n'.join(o))

# ---- TOP PANEL: viewed from above, x right, y up (back of pedal at top) ----
fy = lambda y: S - y
top = [
    ('rect',   94, fy(124), 156, 88, 'DISPLAY WINDOW 156x88 R3 (x16..172, y80..168)'),
    ('circle', 47, fy(30), 12.2, 0, 'FS_D 12.2'),
    ('circle', 141, fy(30), 12.2, 0, 'FS_U 12.2'),
    ('rect',   94, fy(30), 24, 13, 'OLED window 24x13'),
    ('circle', 47, fy(52), 8.0, 0, 'LED FS_D 8'),
    ('circle', 141, fy(52), 8.0, 0, 'LED FS_U 8'),
    ('circle', 94, fy(52), 8.0, 0, 'LED STATUS 8'),
]
svg('top-panel.svg', S, S, top, 'TOP PANEL (view from above)')

# ---- LID: viewed from OUTSIDE (underneath). u = 188 - x (mirrored), v = y (up) ----
ROWS = [101.5 + 7*k for k in range(6)]                 # slot rows, centred on y=119
lid = []
for y in ROWS:                                         # intake slots: 30 long, centre x=30
    lid.append(('slot', S-30, fy(y), 30, 5, 'IN' if y == ROWS[0] else ''))
for y in ROWS:                                         # exhaust slots: 20 long, centre x=172
    lid.append(('slot', S-172, fy(y), 20, 5, 'EXH' if y == ROWS[0] else ''))
for (x, y) in ((81, 94.5), (139, 94.5), (81, 143.5), (139, 143.5)):   # Pi 5 58x49 pattern
    lid.append(('circle', S-x, fy(y), 2.7, 0, 'Pi M2.5 clearance'))
for (x, y) in ((20, 20), (168, 20), (20, 168), (168, 168)):
    lid.append(('circle', S-x, fy(y), 3.2, 0, 'foot M3'))
svg('lid-bottom.svg', S, S, lid,
    'LID / BOTTOM PLATE (view from OUTSIDE, mirrored: u=188-x). Pi long axis along x')

# ---- WALLS: 188 wide x 67 tall strips; z measured down from top outer face ----
def wall(name, title, holes):
    items = [(k, u, WALL_Z, a, b, l) for (k, u, a, b, l) in holes]
    svg(name, S, H, items, title + ' (top panel edge = top of this drawing)')
# left wall x=0, viewed from outside: u = 188 - y
wall('wall-left.svg', 'LEFT WALL x=0 (view from outside)', [
    ('circle', S-50, 9.5, 0, 'OUT-L y=50'),
    ('circle', S-76, 9.5, 0, 'OUT-R y=76')])
# right wall x=188, viewed from outside: u = y
wall('wall-right.svg', 'RIGHT WALL x=188 (view from outside)', [
    ('circle', 60, 9.5, 0, 'INPUT y=60'),
    ('circle', 146, 24.0, 0, 'USB-A panel, 24 dia (confirm vs part) y=146')])
# rear wall y=188, viewed from outside: u = 188 - x
wall('wall-rear.svg', 'REAR WALL y=188 (view from outside)', [
    ('circle', S-40, 12.0, 0, 'DC 2.1mm x=40'),
    ('circle', S-100, 9.5, 0, 'EXPRESSION TRS x=100')])
print('wrote SVGs to', OUT)
