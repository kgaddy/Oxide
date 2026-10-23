"""Paint a loose, painterly tree: a leaning trunk, a few dark branches, and a crown of chunky dabs.

    import brushes, randomizers, tree
    args = randomizers.tree_args(rng, colors, weights, brush_list, short)   # random values...
    t = tree.Tree(x=700, y=800, **args)                                      # ...sent in
    cmds += t.commands()

The look is a plein-air oil study: a thick trunk lit on one side and dark on the other, dark
branches reaching into the crown, and a crown built from many short, flat, angular dabs of a few
leaf colors with gaps where the sky shows through, plus a few long dry smears dragged outward.

A Tree paints exactly what it is given, so a conductor (see randomizers.py) can send random
values, or exact ones for a certain tree: a tall bare one (no dabs), a short round one, a tree
that leans hard. NOTHING HERE IS RANDOM except the fine texture of each stroke, driven by `seed`.

The inputs:
  brush        a brush dict from brushes.list_brushes() / random_brush(); every stroke is made into a
               bristle version of it (see paint_styles._Mark.bristle)
  x, y         the BASE of the trunk (where it meets the ground), in canvas pixels (origin top-left)
  height       the whole tree's height, from the base to the top of the crown, in pixels
  colors       a dict of hex strings: "light" (the lit side of the trunk), "dark" (the shadow side of
               the trunk and the branches)
  trunk_width  the trunk's width at its middle, in pixels
  lean         degrees: how far the trunk tips to the right (negative = to the left)
  bow          how far the trunk bends sideways in the middle, as a share of its length
  crown_width  how wide the crown is, in pixels
  trunk_share  how much of the height the trunk takes, 0 to 1 (default 0.6): the trunk's top is that high
  crown_height how tall the crown is, as a share of `height` (default 0.64); the crown's top is the tree's top
  crown_shift  how far the crown sits to the right of the trunk's top, as a share of `crown_width`
               (negative = to the left): a lopsided tree
  branches     the list of branches (below)
  dabs         the list of dabs (below), painted in the order given: put dark ones first
  paint        paint settings for every stroke (see paint_styles.PAINT_DEFAULTS); any you leave out
               use those defaults
  seed         a whole number that drives the fine texture only

Each branch is a dict:
  t            where it leaves the trunk: 0 = the base, 1 = the top
  angle        degrees from straight up, positive to the right
  length       its length, as a share of `crown_width`
  bend         how far its middle is pushed to the side, as a share of its length
  width        its brush radius at the trunk, in pixels

Each dab is a dict (a short flat stroke):
  u, v         where it is in the crown: -1 to 1 across and down it (0, 0 = the middle of the crown)
  length       its length, as a share of `height`
  width        its brush radius, in pixels
  heading      its direction, in radians (0 = right, pi/2 = down)
  color        its hex color
  dry          0 to 1: how much it breaks up into streaks (a smear is high, a dab low)
  roundness    the tip's roundness, 0.05 to 1 (low = a flat, chunky tip turned to its direction)

Running this file paints a small demo of one tree: python3 scripts/abstract/tree.py --run
(--count 3 paints three side by side)
(a random palette and seed each time; add --palette moss --seed 7 to repeat one)
"""

import argparse
import math
import sys
from pathlib import Path

import brushes
import paint_styles


class Tree:
    """A tree made of a trunk, branches and a crown of dabs. See the module docstring."""

    def __init__(self, brush, x, y, height, colors, trunk_width, lean, bow, crown_width, branches, dabs,
                 crown_shift=0.0, trunk_share=0.6, crown_height=0.64, paint=None, seed=0):
        self.brush, self.x, self.y, self.height = brush, x, y, height
        self.colors, self.trunk_width, self.crown_width = colors, trunk_width, crown_width
        self.lean, self.bow = lean, bow
        self.branches, self.dabs = branches, dabs
        self.crown_shift, self.trunk_share, self.crown_height = crown_shift, trunk_share, crown_height
        self.paint, self.seed = paint, seed

    @staticmethod
    def reach(height, crown_width, lean=0.0, crown_shift=0.0, trunk_share=0.6):
        """How far a tree of this size reaches from its middle (halfway up from the base) in any
        direction, in pixels. A static method so you can work it out (to place the tree) before
        making it. It covers the crown (shifted by `crown_shift`), the lean, and the dabs and smears
        that stick out past the crown's edge."""
        half_wide = crown_width * (0.5 + abs(crown_shift)) + height * 0.18 + height * abs(math.tan(math.radians(lean))) * trunk_share
        half_high = height * 0.55
        # The longer way, plus a little of the shorter (so it grows with both).
        return max(half_wide, half_high) + 0.15 * min(half_wide, half_high)

    @property
    def label(self):
        """A short description, for a comment line in the script."""
        return f"Tree ({len(self.branches)} branches, {len(self.dabs)} dabs, brush: {self.brush['name']})"

    # ----- geometry (local coordinates: origin at the base, x right, y down, pixels) -----

    def _trunk_points(self):
        """The trunk's middle line from the base to its top, as local [x, y] points."""
        length = self.height * self.trunk_share
        dx = length * math.tan(math.radians(self.lean))
        bend = self.bow * length
        points = []
        for i in range(6):
            t = i / 5
            # A straight line from the base to the top, pushed sideways by a half wave (the bow).
            points.append([dx * t + bend * math.sin(math.pi * t), -length * t])
        return points

    def _trunk_at(self, t):
        """The point a share `t` (0 to 1) of the way up the trunk's middle line."""
        points = self._trunk_points()
        i = min(int(t * 5), 4)
        f = t * 5 - i
        return [points[i][0] + (points[i + 1][0] - points[i][0]) * f, points[i][1] + (points[i + 1][1] - points[i][1]) * f]

    def _crown_center(self):
        """The middle of the crown, in local coordinates: above the top of the trunk."""
        top = self._trunk_points()[-1]
        return [top[0] + self.crown_shift * self.crown_width, -self.height * (1 - self.crown_height / 2)]

    def _crown_radii(self):
        """Half the crown's width and half its height."""
        return self.crown_width / 2, self.height * self.crown_height / 2

    # ----- the parts -----

    def commands(self):
        """The oxide-run commands that paint the tree: trunk, branches, then the crown."""
        m = paint_styles._Mark(self.seed, self.brush, self.colors["light"], self.x, self.y, self.height, 0,
                               self.trunk_width, self.paint)
        self._trunk(m)
        for b in self.branches:
            self._branch(m, b)
        for d in self.dabs:
            self._dab(m, d)
        return m.cmds

    def _trunk(self, m):
        """The trunk: a wide light stroke up the middle, a flared foot, then a narrow dark stroke down the
        shadow side (the right) so it reads as round."""
        path = self._trunk_points()
        half = self.trunk_width / 2
        pressures = [0.9, 1.0, 1.0, 0.95, 0.8, 0.5]
        solid = {"loading": 1.0, "opacity": 1.0, "roundness": 0.8}  # the trunk is solid paint, whatever the paint inputs say
        m.cmds.append({"command": "set_color", "hex": self.colors["light"]})
        m.stroke([p + [q] for p, q in zip(path, pressures)], half * 1.25, dry=0.2, **solid)
        # The foot: a short, wide stroke across the base so the trunk widens where it meets the ground.
        m.stroke([[-half * 1.5, 0, 0.8], [0, -half * 0.2, 1.0], [half * 1.7, 0, 0.8]], half * 1.2, dry=0.3, **solid)
        # The shadow side: the same path shifted right by a third of the width, narrower.
        shade = [[p[0] + half * 0.55, p[1], q] for p, q in zip(path, pressures)]
        m.cmds.append({"command": "set_color", "hex": self.colors["dark"]})
        m.stroke(shade, half * 0.45, dry=0.2, **solid)

    def _branch(self, m, b):
        """One branch: a dark stroke from the trunk, thinning toward its tip."""
        start = self._trunk_at(b["t"])
        # A little of the trunk's top is inside the crown; branches start there and reach outward.
        length = b["length"] * self.crown_width
        angle = math.radians(b["angle"])
        end = [start[0] + length * math.sin(angle), start[1] - length * math.cos(angle)]
        bend = b["bend"] * length
        mid = [(start[0] + end[0]) / 2 + math.cos(angle) * bend, (start[1] + end[1]) / 2 + math.sin(angle) * bend]
        m.cmds.append({"command": "set_color", "hex": self.colors["dark"]})
        m.stroke([start + [0.9], mid + [0.8], end + [0.3]], b["width"], dry=0.3, loading=1.0, opacity=1.0, roundness=0.8)

    def _dab(self, m, d):
        """One dab: a short, flat stroke in its color, placed in the crown's oval."""
        cx, cy = self._crown_center()
        rx, ry = self._crown_radii()
        px, py = cx + d["u"] * rx, cy + d["v"] * ry
        half = d["length"] * self.height / 2
        dx, dy = math.cos(d["heading"]) * half, math.sin(d["heading"]) * half
        m.cmds.append({"command": "set_color", "hex": d["color"]})
        # A flat tip turned to the dab's direction makes the angular, chunky look.
        m.stroke([[px - dx, py - dy, 0.7], [px, py, 1.0], [px + dx, py + dy, 0.5]], d["width"], dry=d["dry"],
                 roundness=d["roundness"], angle=round(math.degrees(d["heading"])))


# ---------- demo ----------

def demo(seed, palette, count=1, width=900, height=600):
    """A test painting with `count` trees (one by default), painted from `palette` (a name in palettes.json). The values
    come from randomizers.py (the demo is a tiny conductor); the Tree itself only paints what it is given."""
    import random

    import palettes
    import randomizers

    rng = random.Random(seed)
    brush_list = brushes.list_brushes()
    colors = palettes.load_palette(palette)["colors"]
    weights = palettes.role_weights(len(colors))
    cmds = [{"command": "new_painting", "width": width, "height": height},
            {"command": "set_paper", "color": max(colors, key=palettes.luminance), "kind": "panel", "depth": 0.1}]
    # The trees are spread evenly across the canvas (one goes in the middle).
    for i in range(count):
        x = round(width * (i + 1) / (count + 1))
        args = randomizers.tree_args(rng, colors, weights, brush_list, 420)
        t = Tree(x=x, y=540, **args)
        cmds += ["# " + t.label] + t.commands() + [{"command": "dry"}]
    return cmds


if __name__ == "__main__":
    import commands  # write_script and run_script live there

    ap = argparse.ArgumentParser(description="Paint a demo of one or more trees.")
    ap.add_argument("--seed", type=int, help="the seed for the demo's random values (default: random; the same seed repeats the painting)")
    ap.add_argument("--palette", help="the palette's name in palettes.json (default: random)")
    ap.add_argument("--count", type=int, default=1, help="how many trees to paint side by side (default 1)")
    ap.add_argument("--out", default=".", help="folder for the .jsonl and .png files")
    ap.add_argument("--run", action="store_true", help="paint it with oxide-run too")
    args = ap.parse_args()

    out = Path(args.out).expanduser()
    out.mkdir(parents=True, exist_ok=True)
    # Like the conductors: a random seed and palette unless you give them.
    import random

    import palettes

    seed = args.seed if args.seed is not None else random.randrange(1_000_000)
    palette = palettes.load_palette(args.palette or random.choice(list(palettes.load_palettes())))["name"]
    png, script = (out / f"tree-{palette.lower()}-{seed}.png").resolve(), out / f"tree-{palette.lower()}-{seed}.jsonl"
    cmds = demo(seed, palette, args.count) + [{"command": "export", "path": str(png)}]
    commands.write_script(cmds, script)
    print(f"{script}  (palette {palette}, seed {seed})")
    if args.run:
        if commands.run_script(script.resolve()):
            print(f"  painted {png}")
        else:
            sys.exit(1)
