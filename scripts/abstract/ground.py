"""The ground: a solid coat of one color from the horizon down to the bottom of the canvas.

`ground(color, width, height, horizon, brush, ...)` returns the oxide-run lines (JSON strings)
that paint it, the way background.py paints the whole canvas but only below `horizon`. A forest
conductor paints the sky with the background, then this, then the trees standing on it.

NOTHING HERE IS RANDOM except fine texture: the color, the horizon and the brush are all sent in
(see randomizers.ground_args for random ones). The only randomness inside is the small wobble of
each row of strokes, driven by `seed`.
"""

import json
import random


def ground(color, width, height, horizon, brush, seed=0):
    """Paint the canvas below the line `horizon` (pixels from the top) with `color` (a hex string).

    `brush` is a brush dict from brushes.list_brushes(). Rows of side-to-side strokes overlap from
    the horizon to the bottom, in lean full paint, dried so later paint sits on top. The strokes
    start a little above the horizon so no sky shows between the coat and the horizon line.
    Returns a list of lines.
    """
    rng = random.Random(seed)  # fine texture only: the wobble of each row
    radius = min(200.0, max(10.0, (height - horizon) / 4))
    row_step = radius * 1.2  # rows overlap by 40%
    commands = [
        f"# Ground (brush: {brush['name']})",
        {"command": "set_tool", "tool": "oils"},
        {"command": "set_paint_mode", "mode": "paint"},
        {"command": "set_brush", "preset": brush["name"], "size": round(radius, 1), "opacity": 1.0,
         "loading": 1.0, "length": 1.0, "oiliness": 0.0, "thickness": 0.2},
        {"command": "set_color", "hex": color},
    ]
    y = horizon
    while y < height + radius:
        left, right = -radius, width + radius
        wobble = lambda: round(y + rng.uniform(-0.1, 0.1) * radius, 1)
        points = [[round(left + (right - left) * t / 4, 1), wobble()] for t in range(5)]
        commands.append({"command": "stroke", "points": points, "pressure": 1.0, "smooth": True})
        y += row_step
    commands.append({"command": "dry"})
    return [c if isinstance(c, str) else json.dumps(c) for c in commands]
