"""The painting's background: a solid coat of one color over the whole canvas.

`background(color, width, height, brush, ...)` returns the oxide-run lines (JSON
strings) that paint it, which abstract.py writes right after new_painting.

NOTHING HERE IS RANDOM except fine texture: the color, the paper surface, how deep
its texture is and the brush are all sent in (see randomizers.background_args for
random ones), so a conductor can pick them or send exact values. The only
randomness inside is the small wobble of each row of strokes, driven by `seed`.
"""

import json
import random


def background(color, width, height, brush, seed=0, kind="canvas", depth=0.45):
    """Paint the whole canvas with `color` (a hex string).

    `brush` is the brush to paint with (a dict from brushes.list_brushes()). `kind` is the
    paper surface (see set_paper: "canvas", "linen", "panel", "rough", ...) and `depth` (0 to 1)
    how deep its texture is. `seed` drives the wobble of the strokes.
    Returns a list of lines: the paper, the brush in lean oil paint, then
    side-to-side strokes in overlapping rows from top to bottom, dried so
    later paint sits on top instead of mixing in. A brush with gaps in it
    (sponge, dotted, bristles) leaves gaps in the coat; the paper is the same
    color, so they don't show as white.
    """
    rng = random.Random(seed)  # fine texture only: the wobble of each row
    # The biggest brush allowed (radius 200), or smaller on small canvases.
    radius = min(200.0, height / 6)
    # Rows overlap by 40% so no paper shows between them.
    row_step = radius * 1.2

    commands = [
        f"# Background (brush: {brush['name']})",
        # Same color paper, so any gap between strokes doesn't show.
        {"command": "set_paper", "color": color, "kind": kind, "depth": depth},
        {"command": "set_layer", "name": "Background"},
        {"command": "set_tool", "tool": "oils"},
        {"command": "set_paint_mode", "mode": "paint"},
        # The chosen brush at the biggest size, with lean, fully loaded paint that
        # never runs out. Only the size and paint settings are set here: shape
        # settings (roundness, streaks, bristles...) would undo the brush.
        {"command": "set_brush", "preset": brush["name"], "size": round(radius, 1), "opacity": 1.0,
         "loading": 1.0, "oiliness": 0.0, "thickness": 0.2},
        {"command": "set_color", "hex": color},
    ]

    # Start and end each row past the edges so the strokes' ends are off the canvas.
    y = 0.0
    while y < height + radius:
        left, right = -radius, width + radius
        wobble = lambda: round(y + rng.uniform(-0.1, 0.1) * radius, 1)
        points = [[round(left + (right - left) * t / 4, 1), wobble()] for t in range(5)]
        commands.append({"command": "stroke", "points": points, "pressure": 1.0, "smooth": True})
        y += row_step

    commands.append({"command": "dry"})
    return [c if isinstance(c, str) else json.dumps(c) for c in commands]
