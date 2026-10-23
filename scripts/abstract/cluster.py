"""Paint a cluster of marks. The marks are sent in; nothing here is random.

    import brushes, cluster, randomizers
    args = randomizers.cluster_args(rng, colors, weights, brush_list, short)   # random values...
    c = cluster.Cluster(x=300, y=200, **args)                                    # ...sent in
    cmds += c.commands()

A Cluster paints exactly the marks it is given, so a conductor (see randomizers.py)
can send random marks, or exact ones to get a certain effect: twelve marks in a ring,
all pressed hard, all the same width.

The inputs:
  brush      a brush dict from brushes.list_brushes() / random_brush()
  color      a hex string: the color of a mark that doesn't have its own
  x, y       the CENTER of the cluster, in canvas pixels (origin top-left)
  size       the width of the cluster's oval area, in pixels
  marks      the list of marks (below)
  rotation   degrees, clockwise: which way the oval area is turned
  aspect     how wide the oval is across, as a share of `size` (1 = a circle)
  tool       "oils" (default), "ink", "pencil", "marker", ...
  brush_size the brush radius in pixels; by default the brush's own size

Each mark is a dict (it is a short curved stroke) with these keys:
  u, v       where it is in the oval area: -1 to 1 across and down it (0, 0 = the middle)
  length     its length, as a share of `size`
  heading    its direction, in radians
  bend       how far its middle is pushed to the side, as a share of its length
  pressure   three pressures (0 to 1): start, middle, end
  radius     its brush radius, in pixels
  color      (optional) its color, if not the cluster's
  settings   (optional) extra set_brush settings for it, such as a brushes.random_settings dict

Running this file paints a small demo: python3 scripts/abstract/cluster.py --run
"""

import argparse
import math
import sys
from pathlib import Path

import brushes


class Cluster:
    """A group of marks inside an oval area. See the module docstring."""

    def __init__(self, brush, color, x, y, size, marks, rotation=0, aspect=1.0, tool="oils", brush_size=None):
        self.brush, self.color, self.tool = brush, color, tool
        self.x, self.y, self.size, self.aspect = x, y, size, aspect
        self.marks = marks
        self.angle = math.radians(rotation)
        self.radius = brush_size if brush_size is not None else brush["size"]

    @staticmethod
    def reach(size, radius):
        """How far a cluster of this `size` painted with a brush of this `radius` reaches
        from its center in any direction, in pixels: the area, the marks that stick out
        past its edge, and the widest brush (a mark's radius can be up to 1.6 times the
        brush size). A static method so you can work it out (to place the cluster) before
        making the cluster."""
        return size * 0.8 + radius * 1.6

    @property
    def extent(self):
        """How far this cluster reaches from its center (see `reach`)."""
        return self.reach(self.size, self.radius)

    @property
    def label(self):
        """A short description, for a comment line in the script."""
        return f"Cluster ({len(self.marks)} marks, brush: {self.brush['name']})"

    def _points(self, mark):
        """The three points of one mark's stroke: start, middle (pushed to the side), end."""
        # Where the mark is centered: its place in the oval, turned by the rotation, moved to the cluster's center.
        local_x = mark["u"] * self.size / 2
        local_y = mark["v"] * self.size / 2 * self.aspect
        cx = self.x + local_x * math.cos(self.angle) - local_y * math.sin(self.angle)
        cy = self.y + local_x * math.sin(self.angle) + local_y * math.cos(self.angle)
        # A short stroke in the mark's direction; the middle point is pushed `bend` pixels to the
        # side, at a right angle to that direction: that is the curve.
        length = mark["length"] * self.size
        heading = mark["heading"]
        dx, dy = math.cos(heading) * length / 2, math.sin(heading) * length / 2
        bend = mark["bend"] * length
        mid_x, mid_y = cx - math.sin(heading) * bend, cy + math.cos(heading) * bend
        start, middle, end = mark["pressure"]
        return [[round(cx - dx, 1), round(cy - dy, 1), start],
                [round(mid_x, 1), round(mid_y, 1), middle],
                [round(cx + dx, 1), round(cy + dy, 1), end]]

    def commands(self):
        """The oxide-run commands that paint the cluster: the tool, then for every mark its
        brush, its color and its stroke."""
        cmds = [{"command": "set_tool", "tool": self.tool}]
        for mark in self.marks:
            cmds += [
                {"command": "set_brush", "preset": self.brush["name"], "size": mark["radius"], **mark.get("settings", {})},
                {"command": "set_color", "hex": mark.get("color", self.color)},
                {"command": "stroke", "points": self._points(mark), "smooth": True},
            ]
        return cmds


# ---------- demo ----------

def demo(seed, width=900, height=600):
    """A test painting with three clusters. The marks come from randomizers.py (the
    demo is a tiny conductor); the Cluster itself only paints what it is given."""
    import random

    import randomizers

    rng = random.Random(seed)
    brush_list = brushes.list_brushes()
    cmds = [{"command": "new_painting", "width": width, "height": height}]
    for x, color, rotation in ((220, "#C2151B", 0), (450, "#2021A0", 45), (690, "#E2C43F", 100)):
        brush = brushes.random_brush(rng, brush_list)
        marks = randomizers.cluster_marks(rng, brush, color, rng.randint(6, 24), 14, 0.3, 0.08)
        c = Cluster(brush, color, x, 300, 200, marks, rotation=rotation, aspect=round(rng.uniform(0.4, 1.0), 2), brush_size=14)
        cmds += ["# " + c.label] + c.commands()
    return cmds


if __name__ == "__main__":
    import commands  # write_script and run_script live there

    ap = argparse.ArgumentParser(description="Paint a demo of three clusters.")
    ap.add_argument("--seed", type=int, default=1, help="the seed for the demo's random marks")
    ap.add_argument("--out", default=".", help="folder for the .jsonl and .png files")
    ap.add_argument("--run", action="store_true", help="paint it with oxide-run too")
    args = ap.parse_args()

    out = Path(args.out).expanduser()
    out.mkdir(parents=True, exist_ok=True)
    png, script = (out / f"cluster-{args.seed}.png").resolve(), out / f"cluster-{args.seed}.jsonl"
    cmds = demo(args.seed) + [{"command": "export", "path": str(png)}]
    commands.write_script(cmds, script)
    print(script)
    if args.run:
        if commands.run_script(script.resolve()):
            print(f"  painted {png}")
        else:
            sys.exit(1)
