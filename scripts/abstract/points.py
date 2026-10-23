"""Strong points on a canvas, from classic composition rules.

Objects placed on these points (or near them) look intentional, where objects
placed at random often look accidental. Every function works from the canvas
size, so it fits any canvas. Points are (x, y) in canvas pixels, origin top-left.

The rules: thirds, golden (four points each), diagonal (four points along a
diagonal), and spiral (a golden spiral: biggest region first, the eye last).
`mirror()` turns any of them to another orientation.

    import points
    strong = points.RULES["golden"](1200, 900)      # four points
    x, y = points.pick_points(rng, strong, 1)[0]    # one of them
    x, y = points.nudge(rng, (x, y), 1200, 900)     # not exactly on it
    x, y = points.fit(x, y, extent=120, width=1200, height=900)  # keep it on the canvas
"""

import math

# The golden ratio's two parts: the larger one, 0.618, and the smaller, 0.382
# (they add up to 1). The golden points sit 0.382 and 0.618 across the canvas.
PHI_BIG = (math.sqrt(5) - 1) / 2
PHI_SMALL = 1 - PHI_BIG


def _grid_points(width, height, low, high):
    """The four points where the lines at `low` and `high` (fractions) cross."""
    return [(width * fx, height * fy) for fy in (low, high) for fx in (low, high)]


def thirds_points(width, height):
    """Rule of thirds: split the canvas into thirds each way; the four points
    where the lines cross, at 1/3 and 2/3 across and down."""
    return _grid_points(width, height, 1 / 3, 2 / 3)


def golden_points(width, height):
    """Golden ratio (the "phi grid"): the same idea, but the lines sit at
    0.382 and 0.618 across and down. A little closer to the center than thirds
    (on a 1200x900 canvas, 58 px across and 44 px down from the thirds points)."""
    return _grid_points(width, height, PHI_SMALL, PHI_BIG)


# Where the diagonal's points sit along it, as fractions of the way from one
# corner to the opposite one. Spaced about like the golden ratio, and kept
# away from the corners so objects fit.
DIAGONAL_STEPS = (0.15, PHI_SMALL, PHI_BIG, 0.85)


def diagonal_points(width, height):
    """Diagonal: four points along the line from the top-left corner to the
    bottom-right one. A diagonal leads the eye across the picture and gives
    a feeling of movement. (Mirror it with `mirror()` for the other diagonal.)"""
    return [(width * t, height * t) for t in DIAGONAL_STEPS]


def spiral_points(width, height, count=5):
    """Golden spiral: the centers of the pieces you get by cutting the canvas
    again and again at the golden ratio, plus the "eye" where it winds in.

    Cut the canvas so the left piece is 0.618 of the width and set it aside;
    cut the top 0.618 of what's left; then the right; then the bottom; and so
    on, round and round. Each piece is about 0.618 times the size of the one
    before, so the points come back **ordered from the biggest region to the
    smallest, and the last point is the eye**. Put the biggest object on the
    first point and smaller ones on the following points; the eye is the
    natural focal point. `count` is the number of points including the eye
    (at least 2). The spiral starts at the left and winds toward the
    lower right; use `mirror()` to turn it.
    """
    x0, y0, x1, y1 = 0.0, 0.0, float(width), float(height)  # the part of the canvas still to cut
    spiral = []
    for step in range(max(count, 2) - 1):
        w, h = x1 - x0, y1 - y0
        side = step % 4  # which side to cut a piece from: left, top, right, bottom
        if side == 0:
            cut = w * PHI_BIG
            piece = (x0, y0, x0 + cut, y1)
            x0 += cut
        elif side == 1:
            cut = h * PHI_BIG
            piece = (x0, y0, x1, y0 + cut)
            y0 += cut
        elif side == 2:
            cut = w * PHI_BIG
            piece = (x1 - cut, y0, x1, y1)
            x1 -= cut
        else:
            cut = h * PHI_BIG
            piece = (x0, y1 - cut, x1, y1)
            y1 -= cut
        spiral.append(((piece[0] + piece[2]) / 2, (piece[1] + piece[3]) / 2))  # the piece's center
    spiral.append(((x0 + x1) / 2, (y0 + y1) / 2))  # the eye: the center of what's left
    return spiral


def mirror(strong_points, width, height, flip_x=False, flip_y=False):
    """Flip points left-right (`flip_x`) and/or top-bottom (`flip_y`).

    A diagonal flipped one way runs the other diagonal; a spiral's eye moves
    to another corner. Thirds and golden points land on the same four spots.
    """
    return [(width - x if flip_x else x, height - y if flip_y else y) for x, y in strong_points]


# The rules a painting can use, by name: each takes (width, height) and returns points.
RULES = {"thirds": thirds_points, "golden": golden_points, "diagonal": diagonal_points, "spiral": spiral_points}

# Rules whose points come in an order that means something (spiral: biggest
# region first, the eye last). pick_points keeps that order for these.
ORDERED = {"spiral"}


def pick_points(rng, strong_points, count, ordered=False):
    """Choose `count` different points, spread so the picture stays balanced.

    The first is random. The second is the point diagonally opposite the
    first (a big object on one side and a smaller one across from it feels
    balanced, where two neighbors feel crowded). Any more are random from the
    ones left. Asking for more points than there are returns them all.

    With `ordered=True` (for rules in ORDERED, like the spiral) nothing is
    random: you get the first `count` points in order.
    """
    if ordered:
        return list(strong_points[:count])
    remaining = list(strong_points)
    chosen = []
    while remaining and len(chosen) < count:
        if len(chosen) == 1:
            # The mirror image of the first point through the middle of all the points.
            cx = sum(p[0] for p in strong_points) / len(strong_points)
            cy = sum(p[1] for p in strong_points) / len(strong_points)
            mirror = (2 * cx - chosen[0][0], 2 * cy - chosen[0][1])
            pick = min(remaining, key=lambda p: (p[0] - mirror[0]) ** 2 + (p[1] - mirror[1]) ** 2)
        else:
            pick = rng.choice(remaining)
        remaining.remove(pick)
        chosen.append(pick)
    return chosen


def nudge(rng, point, width, height, amount=0.04):
    """Move a point a little, at random: up to `amount` (a fraction) of the
    canvas size in each direction. An object exactly on a point looks like a
    diagram; a small nudge keeps the point's pull without the stiffness."""
    x, y = point
    return x + rng.uniform(-amount, amount) * width, y + rng.uniform(-amount, amount) * height


def fit(x, y, extent, width, height):
    """Move a center (x, y) just far enough that an object reaching `extent`
    pixels from it in every direction stays on the canvas. An object too big
    for the canvas is centered."""
    def clamp(v, size):
        if extent * 2 >= size:
            return size / 2
        return min(max(v, extent), size - extent)
    return clamp(x, width), clamp(y, height)


def golden_sizes(base, count):
    """`count` sizes starting at `base`, each 0.618 times the one before.
    Objects sized this way relate to each other the way the golden ratio does."""
    return [base * PHI_BIG ** i for i in range(count)]
