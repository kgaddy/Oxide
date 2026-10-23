"""The composition of a painting: its strong points, its focal point and its flow field.

`Composition(rng, width, height)` is made once per painting, from a seeded `rng`, so everything
that is painted agrees on where the center of interest is and which way the strokes flow. A
conductor (a file like abstract.py) makes one and asks it where to put things:

    comp = Composition(rng, 1200, 900)
    comp.points                      # the strong points of this painting (thirds, golden, diagonal or spiral)
    x, y = comp.anchor(extent=120)   # a place for an object that reaches 120 px: a strong point, nudged, kept on the canvas
    start = comp.start_point(0.5)    # where a stroke begins: near the focal point half the time
    path = comp.flow_path(start, 300)   # a stroke's points, following the flow field

The strong points come from points.py; the objects it places (shapes, clusters, paint styles) are
in their own files and never place themselves.
"""

import math

import points as strong  # strong points: thirds, golden, diagonal, spiral (not `points`, which strokes use as a variable)


class Composition:
    """The layout shared by all strokes: a focal point and a flow field.

    Everything is chosen once per painting from the seeded `rng`, so every
    stroke agrees on where the center of interest is and which way to flow.
    """

    def __init__(self, rng, width, height):
        """Make the layout for one painting. `rng` is the seeded random generator;
        `width` and `height` are the canvas size in pixels. Runs once per painting.
        """
        self.rng = rng
        self.w, self.h = width, height
        # Strokes stay this far from the canvas edge.
        self.margin = 0.04 * min(width, height)
        # Strong points: one composition rule per painting (thirds, golden, diagonal
        # or spiral), which gives points to place things on (see points.py).
        self.rule = rng.choice(list(strong.RULES))
        # Turned a random way, so a diagonal can run either way and a spiral's
        # eye can be in any corner.
        self.flip = (rng.random() < 0.5, rng.random() < 0.5)
        self.points = strong.mirror(strong.RULES[self.rule](width, height), width, height, *self.flip)
        # Focal point: a spiral's eye, or else one of the points at random; nudged a little.
        focus = self.points[-1] if self.rule in strong.ORDERED else strong.pick_points(rng, self.points, 1)[0]
        self.focal = strong.nudge(rng, focus, width, height)
        # Flow field: a dominant direction bent by a couple of slow waves.
        # Each wave is (x frequency, y frequency, phase, strength).
        self.base_angle = rng.uniform(-math.pi / 3, math.pi / 3)
        self.waves = [
            (rng.uniform(0.5, 2.5) / width, rng.uniform(0.5, 2.5) / height,
             rng.uniform(0, math.tau), rng.uniform(0.3, 0.9))
            for _ in range(2)
        ]

    def anchor(self, extent, point=None):
        """Where to center an object that reaches `extent` pixels from its center.

        Takes `point` (one of the strong points) or picks one at random,
        nudges it a little so it isn't exactly on the point, then moves it
        just far enough that the whole object stays on the canvas.
        Returns (x, y).
        """
        point = point or strong.pick_points(self.rng, self.points, 1)[0]
        x, y = strong.nudge(self.rng, point, self.w, self.h)
        return strong.fit(x, y, extent, self.w, self.h)

    def angle_at(self, x, y):
        """The flow direction at pixel (x, y), as an angle in radians.

        0 points right, pi/2 points down (the canvas y axis grows downward).
        Every stroke asks this at each step to know which way to keep going,
        which is what makes neighboring strokes look like they move together.
        """
        # Start from the painting-wide direction...
        a = self.base_angle
        # ...then bend it by each wave. sin() and cos() of the position make
        # a smooth ripple, so the direction changes gradually across the canvas
        # instead of jumping. `amp` is how strongly the wave bends the flow.
        for fx, fy, phase, amp in self.waves:
            a += amp * math.sin(x * fx * math.tau + phase) * math.cos(y * fy * math.tau - phase)
        return a

    def clamp(self, x, y):
        """Return (x, y) moved, if needed, to the nearest spot inside the margin.

        A point already inside comes back unchanged. flow_path also uses this
        to detect that a stroke has left the safe area: if clamp() changes the
        point, the stroke is outside.
        """
        m = self.margin
        return min(max(x, m), self.w - m), min(max(y, m), self.h - m)

    def start_point(self, focus):
        """Pick where a stroke begins, as (x, y).

        `focus` is a chance from 0 to 1. At 0.9, nine strokes in ten start
        near the focal point and the rest start anywhere; at 0 they start
        anywhere. Early layers use low values (spread out) and the accent
        uses high values (clustered), which draws the eye to the focal point.
        """
        if self.rng.random() < focus:
            # Bell-curve scatter around the focal point (most land close to it).
            spread = 0.18 * min(self.w, self.h)
            x = self.rng.gauss(self.focal[0], spread)
            y = self.rng.gauss(self.focal[1], spread)
        else:
            x = self.rng.uniform(0, self.w)
            y = self.rng.uniform(0, self.h)
        return self.clamp(x, y)

    def flow_path(self, start, length, against=False, steps=None):
        """Make the points of one stroke by walking along the flow field.

        `start` is the (x, y) to begin at and `length` is roughly how many
        pixels long the stroke is. `against=True` walks across the flow
        instead of with it. `steps` is how many segments to use (by default
        about one per 25 px). Returns a list of [x, y, pressure] points that
        goes straight into an oxide-run "stroke" command.
        """
        steps = steps or max(4, int(length / 25))  # about one point every 25 px
        step = length / steps
        turn = math.pi / 2 if against else 0.0  # `against` turns the stroke 90 degrees off the flow
        x, y = start
        points = []  # each point is [x, y, pressure]
        for i in range(steps + 1):
            t = i / steps  # 0 at the start of the stroke, 1 at the end
            # Pressure rises from light to full in the middle and eases off at the end.
            pressure = 0.35 + 0.65 * math.sin(math.pi * t) ** 0.7
            points.append([round(x, 1), round(y, 1), round(pressure, 2)])
            # Head in the flow direction (plus a little wobble) and move one step.
            a = self.angle_at(x, y) + turn + self.rng.gauss(0, 0.08)
            x, y = x + math.cos(a) * step, y + math.sin(a) * step
            # If the new position is outside the margin, clamp() would move it,
            # so it differs from itself. End the stroke at the margin rather
            # than sliding along the edge.
            if (x, y) != self.clamp(x, y):
                break
        if len(points) == 1:  # a stroke needs two points, so make a tiny one
            points.append([points[0][0] + 1, points[0][1], 0.35])
        return points
