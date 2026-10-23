"""Paint simple shapes (circle, square, triangle) with oxide-run commands.

Each function takes the brush, color, position and size, and returns a list
of oxide-run commands (dicts). Nothing here is random: a shape is painted from
exactly the values it is sent, so the same inputs always give the same shape, and
a conductor (see randomizers.py) can send random values or exact ones. Add the
shapes to a script the way abstract.py adds its layers:

    import brushes, shapes
    brush = brushes.list_brushes()[0]   # a brush dict (or brushes.random_brush(rng, brush_list))
    cmds += shapes.circle(brush, "#C2151B", x=300, y=200, size=120)
    cmds += shapes.square(brush, "#2021A0", x=600, y=200, size=100, fill=True)
    cmds += shapes.triangle(brush, "#E2C43F", x=450, y=450, size=140, rotation=15)

Common arguments:
  brush     a brush dict from brushes.list_brushes()
  color     a hex string such as "#C2151B"
  x, y      the CENTER of the shape, in canvas pixels (origin top-left)
  size      the width of the shape in pixels: a circle's diameter, a square's
            side, a triangle's base (the triangle is equilateral)
  fill      False (default) draws the outline only; True paints it solid
  rotation  degrees, clockwise (no visible change for a circle)
  tool      the paint tool: "oils" (default), "ink", "pencil", "marker", ...
  brush_size  the brush radius in pixels; by default the brush's own size
  settings  extra set_brush settings applied on top of the brush, such as
            brushes.random_settings(rng, brush) for a random variation of it

The shapes are painted with the tool, brush and color you pass in, so they
look like the brush: a Sponge circle is sponge-textured, an Engraver square
is made of tapered lines.

Running this file paints a small demo: python3 scripts/abstract/shapes.py --run
"""

import argparse
import math
import sys
from pathlib import Path

import brushes

# A circle is drawn as a polygon with this many corners; at this many the
# straight segments look round at any size the brush allows.
CIRCLE_SIDES = 64

# ---------- geometry ----------

def _rotate(points, x, y, degrees):
    """Turn `points` clockwise by `degrees` around (x, y)."""
    a = math.radians(degrees)  # the canvas y axis points down, so a positive angle looks clockwise
    cos, sin = math.cos(a), math.sin(a)
    return [(x + (px - x) * cos - (py - y) * sin, y + (px - x) * sin + (py - y) * cos) for px, py in points]


def _circle_points(x, y, size):
    """The corners of a circle's outline, going around once."""
    r = size / 2
    return [(x + r * math.cos(math.tau * i / CIRCLE_SIDES), y + r * math.sin(math.tau * i / CIRCLE_SIDES))
            for i in range(CIRCLE_SIDES)]


def _square_points(x, y, size):
    """The four corners of a square, starting top-left."""
    h = size / 2
    return [(x - h, y - h), (x + h, y - h), (x + h, y + h), (x - h, y + h)]


def _triangle_points(x, y, size):
    """The three corners of an equilateral triangle with its base at the bottom.

    `size` is the base. The height is size * sqrt(3) / 2, and the center is
    the middle of that height, so the shape sits centered on (x, y).
    """
    height = size * math.sqrt(3) / 2
    return [(x, y - height / 2), (x + size / 2, y + height / 2), (x - size / 2, y + height / 2)]


def _fill_rows(corners, step):
    """Horizontal strokes that cover a convex polygon, one row every `step` px.

    Returns a list of point lists, each a left-to-right row across the
    polygon, ready for a stroke command. Works for the circle, square and
    triangle because they are all convex: each row crosses the outline
    exactly twice, so the row runs from the smallest to the largest x found.
    """
    top = min(py for _, py in corners)
    bottom = max(py for _, py in corners)
    # The heights of the rows: from the top edge down in steps. The bottom
    # edge is added too, so the last row doesn't stop short of it.
    heights = [top + i * step for i in range(int((bottom - top) / step) + 1)] + [bottom]
    rows = []
    for y in heights:
        # Where the row at height y crosses each side of the outline.
        xs = []
        for (x1, y1), (x2, y2) in zip(corners, corners[1:] + corners[:1]):
            if y1 != y2 and min(y1, y2) <= y <= max(y1, y2):
                xs.append(x1 + (x2 - x1) * (y - y1) / (y2 - y1))
        if len(xs) >= 2:
            rows.append([(min(xs), y), (max(xs), y)])
    return rows


# ---------- painting ----------

def _paint(corners, brush, color, tool, fill, brush_size, settings):
    """Commands that set up the brush, then paint the outline (and fill) of `corners`."""
    radius = brush_size if brush_size is not None else brush["size"]
    cmds = [
        {"command": "set_tool", "tool": tool},
        # The brush, then any extra settings (see brushes.random_settings) on top of it.
        {"command": "set_brush", "preset": brush["name"], "size": radius, **(settings or {})},
        {"command": "set_color", "hex": color},
    ]
    if fill:
        # Rows closer together than the brush is wide (the radius is half the
        # width), so no gaps show between them.
        step = max(0.5, radius * 1.2)
        for row in _fill_rows(corners, step):
            cmds.append({"command": "stroke", "points": [[round(px, 1), round(py, 1)] for px, py in row]})
    # The outline goes last so it lands on top of the fill: all the corners,
    # then the first one again to close the shape.
    outline = corners + corners[:1]
    cmds.append({"command": "stroke", "points": [[round(px, 1), round(py, 1)] for px, py in outline]})
    return cmds


def circle(brush, color, x, y, size, fill=False, rotation=0, tool="oils", brush_size=None, settings=None):
    """A circle centered on (x, y) with diameter `size`."""
    corners = _rotate(_circle_points(x, y, size), x, y, rotation)
    return _paint(corners, brush, color, tool, fill, brush_size, settings)


def square(brush, color, x, y, size, fill=False, rotation=0, tool="oils", brush_size=None, settings=None):
    """A square centered on (x, y) with sides of length `size`."""
    corners = _rotate(_square_points(x, y, size), x, y, rotation)
    return _paint(corners, brush, color, tool, fill, brush_size, settings)


def triangle(brush, color, x, y, size, fill=False, rotation=0, tool="oils", brush_size=None, settings=None):
    """An equilateral triangle centered on (x, y) with a base of length `size`."""
    corners = _rotate(_triangle_points(x, y, size), x, y, rotation)
    return _paint(corners, brush, color, tool, fill, brush_size, settings)


def reach(kind, size, brush_size):
    """How far a shape of this `kind` ("circle", "square" or "triangle"), `size` and `brush_size`
    reaches from its center in any direction, however it is turned: half the size for a circle,
    a bit over 0.7 of it for a square's corners and a triangle's, plus the brush's width so the
    outline fits too. Use it to keep a shape on the canvas (see Composition.anchor in abstract.py)."""
    return size * (0.5 if kind == "circle" else 0.71) + brush_size


# ---------- demo ----------

def demo(width=900, height=600):
    """A test painting: one of each shape, with exact values (nothing random)."""
    by_name = {b["name"]: b for b in brushes.list_brushes()}
    cmds = [{"command": "new_painting", "width": width, "height": height}]
    cmds += circle(by_name["Round Hard"], "#C2151B", 220, 300, 240, fill=True, brush_size=14)
    cmds += square(by_name["Flat"], "#2021A0", 450, 300, 200, fill=False, rotation=20, brush_size=10)
    cmds += triangle(by_name["Filbert"], "#E2C43F", 690, 300, 240, fill=True, rotation=-10, brush_size=14)
    return cmds


if __name__ == "__main__":
    import commands  # write_script and run_script live there

    ap = argparse.ArgumentParser(description="Paint a demo of circle, square and triangle.")
    ap.add_argument("--out", default=".", help="folder for the .jsonl and .png files")
    ap.add_argument("--run", action="store_true", help="paint it with oxide-run too")
    args = ap.parse_args()

    out = Path(args.out).expanduser()
    out.mkdir(parents=True, exist_ok=True)
    png, script = (out / "shapes-demo.png").resolve(), out / "shapes-demo.jsonl"
    cmds = demo() + [{"command": "export", "path": str(png)}]
    commands.write_script(cmds, script)
    print(script)
    if args.run:
        if commands.run_script(script.resolve()):
            print(f"  painted {png}")
        else:
            sys.exit(1)
