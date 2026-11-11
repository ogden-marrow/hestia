#!/usr/bin/env python3
"""Hestia's icons, drawn on a 16x16 pixel grid like an Apple IIe icon: a black tile, a chunky orange
flame with a white core. One bitmap makes everything: the app icon (SVG of crisp squares), the
one-colour top bar flame (raw ARGB at 16, 32 and 48 pixels) and the symbolic icon for the icon theme.
Run from anywhere: python3 make-icons.py"""
import os

here = os.path.dirname(os.path.abspath(__file__))
root = os.path.abspath(os.path.join(here, "../.."))

# (first column, pixels): # = flame, o = white core. Everything else is the black tile.
FLAME = {
    2: (7, "##"), 3: (7, "##"), 4: (6, "####"), 5: (6, "####.#"), 6: (5, "#####.##"),
    7: (4, "#########"), 8: (4, "##oooo##"), 9: (3, "###oooo###"),
    10: (3, "###oooo###"), 11: (3, "###oooo###"), 12: (4, "##oooo##"),
    13: (4, "########"), 14: (5, "######"),
}
grid = [["." for _ in range(16)] for _ in range(16)]
for y, (x0, row) in FLAME.items():
    for i, c in enumerate(row):
        if c in "#o":
            grid[y][x0 + i] = c
assert all(len(r) == 16 for r in grid)

ORANGE, WHITE, TILE, RING = "#ff7a1a", "#ffffff", "#0d0d0d", "#9a9a9a"
CORNER = {(0, 0), (1, 0), (0, 1), (15, 0), (14, 0), (15, 1), (0, 15), (1, 15), (0, 14), (15, 15), (14, 15), (15, 14)}

def inside_tile(x, y):
    return (x, y) not in CORNER

def ring(x, y):  # a one-pixel light border just inside the edge, like a classic window
    return inside_tile(x, y) and (x in (0, 15) or y in (0, 15) or (x, y) in {(1, 1), (14, 1), (1, 14), (14, 14)})

rects = []
for y in range(16):
    for x in range(16):
        if not inside_tile(x, y):
            continue
        c = grid[y][x]
        fill = ORANGE if c == "#" else WHITE if c == "o" else RING if ring(x, y) else TILE
        rects.append((x, y, fill))
# merge runs of the same colour on a row into one rect, so the SVG stays small and seam-free
svg = ['<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" shape-rendering="crispEdges" role="img" aria-label="Hestia">']
for y in range(16):
    run = None
    for x, yy, f in [r for r in rects if r[1] == y] + [(99, y, None)]:
        if run and run[2] == f and run[1] + run[3] == x:
            run[3] += 1
            continue
        if run:
            svg.append(f'<rect x="{run[1]}" y="{y}" width="{run[3]}" height="1" fill="{run[2]}"/>')
        run = [y, x, f, 1] if f else None
        if run:
            run = [y, x, f, 1]
svg.append("</svg>")
# fix the tuple layout used above (y, x, fill, width)
svg2 = ['<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" shape-rendering="crispEdges" role="img" aria-label="Hestia">']
for y in range(16):
    xs = [(x, f) for x, yy, f in rects if yy == y]
    i = 0
    while i < len(xs):
        j = i
        while j + 1 < len(xs) and xs[j + 1][1] == xs[i][1] and xs[j + 1][0] == xs[j][0] + 1:
            j += 1
        svg2.append(f'<rect x="{xs[i][0]}" y="{y}" width="{j - i + 1}" height="1" fill="{xs[i][1]}"/>')
        i = j + 1
svg2.append("</svg>")
open(os.path.join(here, "app.hestia.Hestia.svg"), "w").write("\n".join(svg2) + "\n")

# the one-colour flame for the top bar: white pixels where the flame is, the core left clear
os.makedirs(os.path.join(root, "tray/icons"), exist_ok=True)
for old in os.listdir(os.path.join(root, "tray/icons")):
    os.remove(os.path.join(root, "tray/icons", old))
# the tip of the flame flickers: four frames that differ only in the top rows
TIPS = [
    {},                                              # 0 steady
    {2: (6, "##"), 3: (6, "##")},                    # 1 leans left
    {2: (8, "##"), 3: (7, "##")},                    # 2 leans right
    {2: None},                                       # 3 short
]
def frame_grid(tip):
    g = [r[:] for r in grid]
    for y, spec in tip.items():
        g[y] = ["." for _ in range(16)]
        if spec:
            x0, row = spec
            for i, c in enumerate(row):
                g[y][x0 + i] = c
    return g
for f, tip in enumerate(TIPS):
    g = frame_grid(tip)
    for scale, n in ((1, 16), (2, 32), (3, 48)):
        out = bytearray()
        for y in range(n):
            for x in range(n):
                out += bytes([255, 242, 242, 242]) if g[y // scale][x // scale] == "#" else bytes([0, 0, 0, 0])
        open(os.path.join(root, f"tray/icons/flame-{n}-{f}.argb"), "wb").write(out)

sym = ['<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16" shape-rendering="crispEdges">']
for y in range(16):
    for x in range(16):
        if grid[y][x] == "#":
            sym.append(f'<rect x="{x}" y="{y}" width="1" height="1" fill="#bebebe"/>')
sym.append("</svg>")
open(os.path.join(here, "app.hestia.Hestia-symbolic.svg"), "w").write("\n".join(sym) + "\n")
print("wrote the app icon, the top bar flame and the symbolic icon")
