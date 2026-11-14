"""Paint styles: bold paint marks with ragged dry-brush edges and splatter.

Each style is one function that paints one mark in a particular style (a diagonal
slash, a fan of bristles, a ragged block, ...). They are PAINT ONLY: every
function uses the oils tool in "paint" mode (pure color, no mixing), never ink,
pencil, marker or knife. Add them to a script like the shapes and clusters:

    import brushes, paint_styles
    cmds += paint_styles.slash(brush, "#C2151B", x=300, y=200, size=220)
    cmds += paint_styles.STYLES["fan"](brush, "#2021A0", 600, 200, 220, seed=7)   # by name

NOTHING HERE IS RANDOM except fine texture. A style paints exactly the values it is
sent, so a conductor (see randomizers.py) can send random values or exact ones to
get a certain effect. The same inputs always give the same mark. The only randomness
inside is the small, uncontrollable wobble of each stroke (its exact wander, where
the paint runs dry, the scatter of the flecks), and it is driven by `seed`: the same
`seed` gives the same texture, a different `seed` gives different texture.

The inputs every style takes:
  brush      a brush dict from brushes.list_brushes() / random_brush(). The style keeps the
             brush's tip and then gives it bristles, streaks and a dry edge, so the mark
             looks like the style whichever brush you pass
  color      a hex string
  x, y       the CENTER of the mark, in canvas pixels (origin top-left)
  size       the width of the area the mark fills, in pixels. Tails and hairlines can reach
             as far as `size` from the center, and splatter farther (see `reach`)
  rotation   degrees, clockwise: turns the whole mark about its center
  brush_size the radius of the main brush, in pixels (default: 7% of `size`)
  paint      a dict of paint settings (below). Any you leave out use PAINT_DEFAULTS
  seed       a whole number: the seed for the fine texture

The `paint` settings (all 0 to 1 unless noted):
  bristles (4-300), bristle_thickness, bristle_spread, bristle_stiffness   the hairs of the brush
  streaks, roughness                       the dry-brush texture
  loading                                  how much paint the brush carries
  thickness                                how thick the paint goes on
  oiliness, opacity                        how oily, how solid
  hair_taper, hair_roughness               how the hairlines thin out and how ragged they are
Each stroke in the mark wanders a little around these (see TEXTURE).

Each style also has its own options for its structure, with a default for each:
  slash   heading (degrees: which way the sweep goes, + or -), width_share (how wide, as a share of `size`),
          hairs (how many hairlines), splatter (how many flecks)
  fan     spread (degrees the fan opens to each side), hairs, splatter
  block   height_share (as a share of `size`), hairs, drips, splatter
  wedge   tip_shift (sideways shift of the point, as a share of `size`), hairs, splatter
  swoosh  first (degrees: where the arc starts), sweep (degrees it covers), hairs, splatter
  flame   phase and wavelength (the shape of the ragged top), hairs, splatter

The styles:
  slash   a diagonal sweep of side-by-side strokes, ragged at both ends, with thin tails
  fan     a solid base with long thin bristle lines fanning out of it
  block   horizontal strokes stacked into a block: straight on the left, ragged on the right
  wedge   strokes that start along a base and meet at a point, like a triangle
  swoosh  curved strokes around an arc, with hairs trailing off one end
  flame   upright strokes of different heights, like a flame or a tuft

Running this file paints a sheet with one of each style (default options):
    python3 scripts/abstract/paint_styles.py --seed 1 --run
"""

import argparse
import math
import random
import sys
from pathlib import Path

import brushes

# The paint settings a style uses for any you don't send: a middle-of-the-road dry-brush
# look. (These are plain fixed numbers: a conductor that wants variety sends its own.)
PAINT_DEFAULTS = {
    "bristles": 80, "bristle_thickness": 0.35, "bristle_spread": 0.6, "bristle_stiffness": 0.55,
    "streaks": 0.7, "roughness": 0.5,
    "loading": 0.9, "thickness": 0.3,
    "oiliness": 0.15, "opacity": 0.95,
    "hair_taper": 0.8, "hair_roughness": 0.3,
}

# How far each stroke's settings wander from the mark's values: up to this much either
# way (and up to 20% either way for the bristle count). This is the fine texture: it
# makes every stroke run dry at its own point, so the body has dry gaps in it.
TEXTURE = 0.12

# The brush used for the splatter dots and flicks: a plain round one, whatever
# brush the mark itself is painted with (a dot made with a bristle brush isn't round).
SPECK_BRUSH = "Round Hard"


def _clamp_radius(radius):
    """Keep a brush radius within what set_brush allows (0.5 to 200)."""
    return round(min(200.0, max(0.5, radius)), 1)


def _wander(rng, value):
    """`value` (0 to 1) moved a little, at random, by up to TEXTURE either way."""
    return round(min(max(value + rng.uniform(-TEXTURE, TEXTURE), 0.0), 1.0), 2)


def _along(center, angle, along, across):
    """The point `along` pixels from `center` in the direction `angle` (radians)
    and `across` pixels to the side of that direction."""
    return (center[0] + along * math.cos(angle) - across * math.sin(angle),
            center[1] + along * math.sin(angle) + across * math.cos(angle))


def _line(rng, start, end, wobble=0.0, start_pressure=0.6, end_pressure=0.2):
    """Points along a stroke from `start` to `end`, each as [x, y, pressure].

    The stroke wanders sideways by up to `wobble` pixels (a smooth wave, so it
    isn't a ruler line). Pressure rises quickly from `start_pressure` to full,
    holds, and eases down to `end_pressure` over the last third: a dry-brush
    stroke that thins out where it ends.
    """
    length = math.hypot(end[0] - start[0], end[1] - start[1]) or 1.0
    steps = max(4, int(length / 20))  # about one point every 20 px
    nx, ny = -(end[1] - start[1]) / length, (end[0] - start[0]) / length  # sideways, at a right angle
    phase = rng.uniform(0, math.tau)
    points = []
    for i in range(steps + 1):
        t = i / steps
        side = wobble * math.sin(math.pi * t + phase) * math.sin(math.pi * t)  # no wobble at the very ends
        pressure = start_pressure + (1 - start_pressure) * min(1.0, t / 0.15) if t < 0.7 else \
            1 - (1 - end_pressure) * (t - 0.7) / 0.3
        points.append([start[0] + (end[0] - start[0]) * t + nx * side,
                       start[1] + (end[1] - start[1]) * t + ny * side,
                       round(pressure, 2)])
    return points


class _Mark:
    """The commands for one mark, built up stroke by stroke in LOCAL coordinates.

    Local coordinates have the origin at the center of the mark, x to the right
    and y down, in pixels, and are turned by `rotation` and moved to (x, y) only
    when a stroke is added. So each style can be written as if it were upright and
    centered, and rotation and placement are handled here, once.

    `self.rng` is seeded with `seed` and used only for fine texture (see the module docstring).
    """

    def __init__(self, seed, brush, color, x, y, size, rotation, brush_size, paint):
        self.rng = random.Random(seed)
        self.brush, self.size = brush, size
        self.x, self.y = x, y
        self.angle = math.radians(rotation)
        # The main brush's radius: given, or 7% of the size (limited to the brush maximum).
        self.radius = _clamp_radius(brush_size if brush_size is not None else size * 0.07)
        # The mark's paint settings: the defaults, with whatever was sent on top.
        self.values = {**PAINT_DEFAULTS, **(paint or {})}
        # Paint only: the oils tool in "paint" mode (pure color, no mixing with what's on the canvas).
        self.cmds = [{"command": "set_tool", "tool": "oils"},
                     {"command": "set_paint_mode", "mode": "paint"},
                     {"command": "set_color", "hex": color}]

    def _place(self, px, py):
        """Turn a local point by the rotation and move it to the mark's center."""
        cos, sin = math.cos(self.angle), math.sin(self.angle)
        return self.x + px * cos - py * sin, self.y + px * sin + py * cos

    def paint(self):
        """The paint settings for one stroke: the mark's values, each wandering a little
        (TEXTURE) so no two strokes are alike."""
        rng, v = self.rng, self.values
        return {"loading": _wander(rng, v["loading"]), "thickness": _wander(rng, v["thickness"]),
                "oiliness": _wander(rng, v["oiliness"]),
                "opacity": max(0.05, _wander(rng, v["opacity"]))}

    def bristle(self, radius, dry=0.5, **extra):
        """A `set_brush` command: the brush the mark came with, made into a
        bristle brush with streaks and a dry edge.

        `radius` is the brush radius in pixels; `dry` (0 to 1) is how much the
        ends of strokes break up into streaks and gaps. `extra` settings go on top.
        """
        rng, v = self.rng, self.values
        settings = {"bristles": int(min(max(v["bristles"] * (1 + rng.uniform(-0.2, 0.2)), 4), 300)),
                    "bristle_thickness": _wander(rng, v["bristle_thickness"]),
                    "bristle_spread": _wander(rng, v["bristle_spread"]),
                    "bristle_stiffness": _wander(rng, v["bristle_stiffness"]),
                    "streaks": _wander(rng, v["streaks"]), "roughness": _wander(rng, v["roughness"]),
                    "lift_off": round(dry, 2), **self.paint(), **extra}
        return {"command": "set_brush", "preset": self.brush["name"], "size": _clamp_radius(radius), **settings}

    def stroke(self, points, radius, dry=0.5, **extra):
        """Add one stroke through local `points` ([x, y] or [x, y, pressure]) with a bristle brush."""
        placed = [[round(v, 1) for v in self._place(p[0], p[1])] + list(p[2:]) for p in points]
        self.cmds += [self.bristle(radius, dry, **extra), {"command": "stroke", "points": placed, "smooth": True}]

    def hair(self, points, radius=None):
        """Add one fine hairline through local `points`: a thin stroke that comes
        to a point at both ends, like a wisp the brush dragged off. It uses a plain
        tip, not bristles (a bristle brush this thin just makes a blob)."""
        radius = radius if radius is not None else max(1.2, self.radius * 0.12)
        placed = [[round(v, 1) for v in self._place(p[0], p[1])] + list(p[2:]) for p in points]
        settings = {"bristles": 0, "streaks": 0.0, "roughness": _wander(self.rng, self.values["hair_roughness"]),
                    "lift_off": 0.0, "taper": _wander(self.rng, self.values["hair_taper"]),
                    "jitter_angle": 0.0, "jitter_size": 0.0, "scatter": 0.0, **self.paint()}
        self.cmds += [{"command": "set_brush", "preset": self.brush["name"], "size": _clamp_radius(radius), **settings},
                      {"command": "stroke", "points": placed, "smooth": True}]

    def splatter(self, count, spread, toward=None):
        """Flecks of paint around the mark: tiny dots and short flicks.

        `count` flecks, scattered in a bell curve around the center with a
        spread of `spread` pixels. `toward` (an angle in radians) pulls them in
        that direction, since paint flies off the way the brush was going.
        """
        rng = self.rng
        # Dots and flicks use a plain round brush, whatever the mark is painted with.
        plain = {"command": "set_brush", "preset": SPECK_BRUSH, "bristles": 0, "streaks": 0.0, "roughness": 0.0,
                 "lift_off": 0.0, "taper": 0.0, "jitter_angle": 0.0, "jitter_size": 0.0, "scatter": 0.0, **self.paint()}
        for _ in range(count):
            px, py = rng.gauss(0, spread), rng.gauss(0, spread)
            if toward is not None:
                pull = abs(rng.gauss(0, spread * 0.6))
                px, py = px + pull * math.cos(toward), py + pull * math.sin(toward)
            radius = _clamp_radius(rng.choice([0.5, 0.6, 0.8, 1.0, 1.3, 1.8, 2.6]) * max(0.7, self.size / 250))
            # Most flecks are dots (two points almost on top of each other); some are short flicks.
            reach = rng.uniform(1, 3) if rng.random() < 0.7 else rng.uniform(0.02, 0.07) * self.size
            heading = rng.uniform(0, math.tau) if toward is None or rng.random() < 0.4 else toward + rng.gauss(0, 0.4)
            a = self._place(px, py)
            b = self._place(px + reach * math.cos(heading), py + reach * math.sin(heading))
            self.cmds += [{**plain, "size": radius},
                          {"command": "stroke", "points": [[round(a[0], 1), round(a[1], 1), 0.9], [round(b[0], 1), round(b[1], 1), 0.4]]}]

    def commands(self):
        return self.cmds


def reach(size):
    """How far a mark of this `size` reaches from its center in any direction,
    counting the body, the tails and the hairlines (measured over 300 seeds per
    style: never more than 0.99 of `size`). It does NOT count the splatter, whose
    flecks scatter in a wide bell curve and can land up to about twice `size`
    away; they are tiny, and any that leave the canvas just aren't painted.
    Use it to keep a mark on the canvas (see Composition.anchor in abstract.py)."""
    return size * 1.0


# ---------- the styles ----------

def slash(brush, color, x, y, size, rotation=0, brush_size=None, paint=None, seed=0,
          heading=40.0, width_share=0.5, hairs=16, splatter=55):
    """A diagonal sweep: side-by-side strokes that are fullest in the middle,
    ragged at both ends, with fine hairlines running off the far end.

    `heading` is the direction the brush travels, in degrees (positive or negative: the
    sweep leans one way or the other). `width_share` is how wide the sweep is, as a share
    of `size`. `hairs` is how many hairlines trail off, and `splatter` how many flecks."""
    m = _Mark(seed, brush, color, x, y, size, rotation, brush_size, paint)
    rng = m.rng
    direction = math.radians(heading)  # the way the brush travels
    width = size * width_share  # across the sweep
    r = m.radius
    count = max(5, int(width / (r * 0.9)))
    for i in range(count):
        across = -width / 2 + width * (i + 0.5) / count + rng.uniform(-0.2, 0.2) * r
        edge = abs(across) / (width / 2)  # 0 in the middle of the sweep, 1 at its sides
        # Strokes toward the sides are shorter and start later, so the sweep is full in the middle.
        length = size * rng.uniform(0.75, 1.0) * (1 - 0.4 * edge ** 2)
        begin = -size * 0.5 + rng.uniform(0, 0.12) * size + 0.1 * size * edge
        start = _along((0, 0), direction, begin, across)
        end = _along((0, 0), direction, begin + length, across + rng.uniform(-0.1, 0.1) * r)
        m.stroke(_line(rng, start, end, wobble=r * 0.4, start_pressure=0.7, end_pressure=0.15), r, dry=rng.uniform(0.4, 0.8))
    # Hairlines the brush dragged off the far end, and a few (a quarter as many) off the near end.
    for _ in range(hairs):
        across = rng.uniform(-0.5, 0.5) * width
        begin = size * rng.uniform(0.1, 0.4)
        start = _along((0, 0), direction, begin, across)
        end = _along((0, 0), direction, begin + size * rng.uniform(0.2, 0.55), across + rng.uniform(-0.12, 0.12) * size)
        m.hair(_line(rng, start, end, wobble=r * 0.3, start_pressure=0.5, end_pressure=0.1))
    for _ in range(max(2, hairs // 4)):
        across = rng.uniform(-0.4, 0.4) * width
        start = _along((0, 0), direction, -size * 0.35, across)
        end = _along((0, 0), direction, -size * rng.uniform(0.5, 0.7), across + rng.uniform(-0.1, 0.1) * size)
        m.hair(_line(rng, start, end, wobble=r * 0.2, start_pressure=0.5, end_pressure=0.1))
    m.splatter(splatter, size * 0.35, toward=direction)
    return m.commands()


def fan(brush, color, x, y, size, rotation=0, brush_size=None, paint=None, seed=0,
        spread=38.0, hairs=60, splatter=55):
    """A solid base with many long fine bristle lines fanning out of it, like
    a brush pressed down and pulled up and apart.

    `spread` is how far the fan opens to each side of straight up, in degrees. `hairs` is
    how many bristle lines, and `splatter` how many flecks."""
    m = _Mark(seed, brush, color, x, y, size, rotation, brush_size, paint)
    rng = m.rng
    r = m.radius
    apex = (0.0, size * 0.42)  # where the fan comes from: the bottom middle
    opening = math.radians(spread)  # how far the fan opens, to each side of straight up
    up = -math.pi / 2
    # The solid base: wide strokes from the apex that stop partway up.
    for _ in range(max(10, int(size / (r * 0.9)))):
        a = up + rng.uniform(-0.85, 0.85) * opening
        length = size * rng.uniform(0.4, 0.7)
        end = _along(apex, a, length, 0)
        start = _along(apex, a, -size * 0.03, rng.uniform(-0.18, 0.18) * size)
        m.stroke(_line(rng, start, end, wobble=r * 0.2, start_pressure=0.8, end_pressure=0.3), r * 1.2, dry=rng.uniform(0.4, 0.7))
    # The bristle lines: many long hairlines, longest in the middle of the fan.
    for _ in range(hairs):
        a = up + rng.uniform(-1, 1) * opening
        length = size * rng.uniform(0.55, 1.0) * (1 - 0.3 * abs(a - up) / opening)
        start = _along(apex, a, size * rng.uniform(0.0, 0.3), 0)
        end = _along(apex, a, length, 0)
        m.hair(_line(rng, start, end, wobble=r * 0.25, start_pressure=0.7, end_pressure=0.1), max(1.2, r * rng.uniform(0.1, 0.22)))
    m.splatter(splatter, size * 0.4, toward=up)
    return m.commands()


def block(brush, color, x, y, size, rotation=0, brush_size=None, paint=None, seed=0,
          height_share=0.65, hairs=12, drips=8, splatter=50):
    """Horizontal strokes stacked into a block: fairly straight on the left,
    ragged on the right where each stroke runs dry, with hairlines and drips.

    `height_share` is the block's height as a share of `size` (its width is 0.8 of `size`).
    `hairs` is how many hairlines carry on to the right, `drips` how many run off the bottom,
    and `splatter` how many flecks."""
    m = _Mark(seed, brush, color, x, y, size, rotation, brush_size, paint)
    rng = m.rng
    r = m.radius
    width, height = size * 0.8, size * height_share
    rows = max(4, int(height / (r * 0.9)))
    ends = []  # where each row stops, so hairlines can carry on from some of them
    for i in range(rows):
        row_y = -height / 2 + height * (i + 0.5) / rows
        left = -width / 2 + rng.uniform(-0.07, 0.05) * size
        # Each row stops at its own distance from the right edge: the ragged side.
        right = width / 2 - rng.uniform(0, 0.22) * size
        ends.append((right, row_y))
        m.stroke(_line(rng, (left, row_y), (right, row_y + rng.uniform(-0.1, 0.1) * r), wobble=r * 0.3,
                       start_pressure=0.85, end_pressure=0.3), r, dry=rng.uniform(0.5, 0.9))
    # Hairlines carrying on to the right from some of the rows.
    for _ in range(hairs):
        ex, ey = rng.choice(ends)
        m.hair(_line(rng, (ex - r, ey + rng.uniform(-r, r)), (ex + size * rng.uniform(0.05, 0.22), ey + rng.uniform(-0.05, 0.05) * size),
                     wobble=r * 0.2, start_pressure=0.5, end_pressure=0.1))
    # Fine runs and drips off the bottom edge.
    for _ in range(drips):
        px = rng.uniform(-0.5, 0.35) * width
        start = (px, height / 2 - r)
        end = (px + rng.uniform(-0.04, 0.04) * size, height / 2 + size * rng.uniform(0.05, 0.22))
        m.hair(_line(rng, start, end, wobble=r * 0.2, start_pressure=0.5, end_pressure=0.1))
    m.splatter(splatter, size * 0.4, toward=0)
    return m.commands()


def wedge(brush, color, x, y, size, rotation=0, brush_size=None, paint=None, seed=0,
          tip_shift=0.0, hairs=14, splatter=50):
    """Strokes that start spread along a base and meet near a point at the
    top: a rough triangle, with hairlines trailing past the base.

    `tip_shift` moves the point sideways, as a share of `size` (0 = centered). `hairs` is how
    many hairlines trail past the base, and `splatter` how many flecks."""
    m = _Mark(seed, brush, color, x, y, size, rotation, brush_size, paint)
    rng = m.rng
    r = m.radius
    tip = (tip_shift * size, -size * 0.45)
    base_y, half = size * 0.4, size * 0.45
    count = max(7, int(2 * half / (r * 0.9)))
    for i in range(count):
        bx = -half + 2 * half * (i + 0.5) / count + rng.uniform(-0.2, 0.2) * r
        by = base_y + rng.uniform(-0.1, 0.03) * size  # a ragged base
        # The stroke runs from the base toward the tip, stopping short of it at a different spot each time.
        reach_to = rng.uniform(0.7, 1.0)
        target = (tip[0] + (bx - tip[0]) * (1 - reach_to) + rng.uniform(-0.02, 0.02) * size,
                  by + (tip[1] - by) * reach_to)
        m.stroke(_line(rng, (bx, by), target, wobble=r * 0.3, start_pressure=0.8, end_pressure=0.15), r, dry=rng.uniform(0.4, 0.8))
    # Hairlines trailing past the base, away from the tip, leaning to one side.
    for _ in range(hairs):
        bx = rng.uniform(-half, half * 0.6)
        start = (bx, base_y - r)
        end = (bx - rng.uniform(0, 0.15) * size, base_y + size * rng.uniform(0.06, 0.3))
        m.hair(_line(rng, start, end, wobble=r * 0.2, start_pressure=0.5, end_pressure=0.1))
    m.splatter(splatter, size * 0.4, toward=math.pi / 2)
    return m.commands()


def swoosh(brush, color, x, y, size, rotation=0, brush_size=None, paint=None, seed=0,
           first=180.0, sweep=150.0, hairs=14, splatter=50):
    """Curved strokes around an arc, one inside the next, like a sweep of the
    arm: a full rounded body, with hairlines trailing off the end.

    `first` is where the sweep starts on the circle, in degrees, and `sweep` how many degrees
    it covers. `hairs` is how many hairlines trail off its end, and `splatter` how many flecks."""
    m = _Mark(seed, brush, color, x, y, size, rotation, brush_size, paint)
    rng = m.rng
    r = m.radius
    center = (size * 0.05, size * 0.05)  # the arc's center
    outer = size * 0.52
    start_angle = math.radians(first)  # where the sweep starts on the circle
    last = start_angle + math.radians(sweep)  # and where it ends
    rings = max(5, int(outer * 0.95 / (r * 0.85)))
    for i in range(rings):
        radius = outer * (0.05 + 0.95 * (i + 0.5) / rings) * rng.uniform(0.95, 1.05)  # from the middle outward, so the body is full
        a0 = start_angle + rng.uniform(0, 0.2) * (last - start_angle)
        a1 = last - rng.uniform(0, 0.4) * (last - start_angle)
        steps = max(8, int((a1 - a0) * radius / 20))
        arc = [[center[0] + radius * math.cos(a0 + (a1 - a0) * t / steps),
                center[1] + radius * math.sin(a0 + (a1 - a0) * t / steps),
                round(0.85 - 0.7 * (t / steps) ** 2, 2)] for t in range(steps + 1)]
        m.stroke(arc, r, dry=rng.uniform(0.4, 0.8))
    # Hairlines trailing off the end of the sweep, along the arc's tangent.
    tangent = last + math.pi / 2
    for _ in range(hairs):
        radius = outer * rng.uniform(0.2, 1.0)
        begin = (center[0] + radius * math.cos(last), center[1] + radius * math.sin(last))
        end = (begin[0] + size * rng.uniform(0.1, 0.4) * math.cos(tangent + rng.gauss(0, 0.15)),
               begin[1] + size * rng.uniform(0.1, 0.4) * math.sin(tangent + rng.gauss(0, 0.15)))
        m.hair(_line(rng, begin, end, wobble=r * 0.2, start_pressure=0.5, end_pressure=0.1))
    m.splatter(splatter, size * 0.4, toward=tangent)
    return m.commands()


def flame(brush, color, x, y, size, rotation=0, brush_size=None, paint=None, seed=0,
          phase=0.0, wavelength=0.35, hairs=14, splatter=50):
    """Upright strokes of different heights side by side: a solid base with a
    ragged top, like a flame or a tuft, with hairlines thrown off the tips.

    `phase` (radians, 0 to 6.28) and `wavelength` (0.25 to 0.45 works well) shape the ragged
    top: where its peaks fall and how many there are. `hairs` is how many hairlines carry on up
    from the tips, and `splatter` how many flecks."""
    m = _Mark(seed, brush, color, x, y, size, rotation, brush_size, paint)
    rng = m.rng
    r = m.radius
    width = size * 0.6
    base_y = size * 0.42
    count = max(6, int(width / (r * 0.9)))
    tops = []  # where each stroke ends, so hairlines can start from the tips
    for i in range(count):
        t = (i + 0.5) / count
        px = -width / 2 + width * t + rng.uniform(-0.15, 0.15) * r
        edge = abs(t - 0.5) * 2  # 0 in the middle, 1 at the sides
        # How tall this stroke is: a smooth bump, so the top is ragged but not noise.
        height = size * (0.55 + 0.4 * (1 - edge ** 2) * (0.6 + 0.4 * math.sin(phase + t * math.tau / wavelength)))
        height *= rng.uniform(0.85, 1.05)
        top = (px + rng.uniform(-0.05, 0.05) * size, base_y - height)
        tops.append(top)
        m.stroke(_line(rng, (px, base_y + rng.uniform(-0.08, 0.02) * size), top, wobble=r * 0.3, start_pressure=0.9, end_pressure=0.15),
                 r, dry=rng.uniform(0.4, 0.8))
    # Hairlines carrying on up from the tips, leaning a little sideways.
    for _ in range(hairs):
        tx, ty = rng.choice(tops)
        end = (tx + rng.uniform(-0.15, 0.15) * size, ty - size * rng.uniform(0.06, 0.25))
        m.hair(_line(rng, (tx, ty + r), end, wobble=r * 0.2, start_pressure=0.5, end_pressure=0.1))
    m.splatter(splatter, size * 0.4, toward=-math.pi / 2)
    return m.commands()


# Every style by name: STYLES[name](brush, color, x, y, size, ...).
STYLES = {"slash": slash, "fan": fan, "block": block, "wedge": wedge, "swoosh": swoosh, "flame": flame}


# ---------- demo ----------

def demo(seed, brush_name="Bristle Flat", color="#D8403A", cell=330):
    """A sheet with one of each style on white, three across and two down: the default
    options and paint settings, so the only difference between seeds is the fine texture."""
    brush = next(b for b in brushes.list_brushes() if b["name"].lower() == brush_name.lower())
    width, height = cell * 3, cell * 2
    cmds = [{"command": "new_painting", "width": width, "height": height},
            # White, smooth paper with the texture, ridges and shadows turned down, like flat paint on a clean page.
            {"command": "set_paper", "color": "#FAFAFA", "kind": "smooth", "paper_texture": 1, "paint_texture": 1,
             "impasto_depth": 1, "gloss": 0, "shadows": 0}]
    for i, (name, style) in enumerate(STYLES.items()):
        cx, cy = cell * (i % 3 + 0.5), cell * (i // 3 + 0.5)
        cmds += [f"# {name}"] + style(brush, color, round(cx), round(cy), round(cell * 0.62), seed=seed * 100 + i)
    return cmds


if __name__ == "__main__":
    import commands  # write_script and run_script live there

    ap = argparse.ArgumentParser(description="Paint a sheet with one of each paint style.")
    ap.add_argument("--seed", type=int, default=1, help="the texture seed (the marks are otherwise the same every time)")
    ap.add_argument("--brush", default="Bristle Flat", help="the brush to paint with (a name from list_brushes)")
    ap.add_argument("--out", default=".", help="folder for the .jsonl and .png files")
    ap.add_argument("--run", action="store_true", help="paint it with oxide-run too")
    args = ap.parse_args()

    out = Path(args.out).expanduser()
    out.mkdir(parents=True, exist_ok=True)
    png, script = (out / f"paint-styles-{args.seed}.png").resolve(), out / f"paint-styles-{args.seed}.jsonl"
    cmds = demo(args.seed, args.brush) + [{"command": "export", "path": str(png)}]
    commands.write_script(cmds, script)
    print(script)
    if args.run:
        if commands.run_script(script.resolve()):
            print(f"  painted {png}")
        else:
            sys.exit(1)
