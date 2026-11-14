#!/usr/bin/env python3
"""Generate random abstract paintings as oxide-run scripts (.jsonl).

This file is a CONDUCTOR: it decides what goes on the canvas (how many marks, which
styles, where) and sends the values to the object files (background.py, paint_styles.py,
shapes.py, cluster.py). Those files only paint what they are sent; the random values come
from randomizers.py, and a conductor can send exact values instead to get a certain effect.

Randomness is steered by a few composition principles:
  - Big to small: a broad ground, then shapes, then fine marks and accents.
  - Each painting uses one composition rule (rule of thirds, golden ratio,
    diagonal or golden spiral) for its strong points. A focal point on one of
    them pulls strokes toward it, and shapes are centered on them.
  - A flow field gives the strokes a shared direction (rhythm); a few go
    against it for tension.
  - Each layer paints with a random brush from the app's brush list (brushes.py).
  - Right after the background come a random number of bold paint marks in
    random styles (paint_styles.py), then one random shape (shapes.py) and one
    random cluster of marks (cluster.py), each on its own strong point.
  - Color follows 60-30-10: the palette's first color dominates, the middle
    ones support, the last is a sparing accent.

    python3 scripts/abstract/abstract.py                       # random palette and seed
    python3 scripts/abstract/abstract.py --palette moss --seed 42 --run
    python3 scripts/abstract/abstract.py --count 5 --run --out ~/Desktop/abstract
    python3 scripts/abstract/abstract.py --list-palettes

Palettes are entries in scripts/abstract/palettes.json:
    {"moss": ["dominant", ..., "accent"], ...}
Colors are hex strings ("#D9825B").
"""

import math
import random

import brushes
import paint_styles
import points as strong  # strong points: thirds, golden (not `points`, which strokes use as a variable)
import cli
from commands import brush_command, color_command
from commands import finish as finish_commands
from composition import Composition
from makers import make_background, make_cluster, make_paint_mark, make_shape
from palettes import luminance, role_weights

# Testing switches. While the shapes and clusters are being tested, the Ground /
# Shapes / Marks / Accent layers are off and a shape and a cluster are always
# painted. To go back to full paintings, set STROKE_LAYERS = True,
# SHAPE_CHANCE = 0.5 and CLUSTER_CHANCE = 0.5.
STROKE_LAYERS = False  # True = paint the four stroke layers after the background, shape and cluster
SHAPE_CHANCE = 1.0  # the chance (0 to 1) that a painting gets a shape
CLUSTER_CHANCE = 1.0  # the chance (0 to 1) that a painting gets a cluster

# How many bold paint marks (see paint_styles.py) a painting gets: a random whole number
# from the lowest to the highest here. (0, 0) = none.
MARK_COUNT = (3, 8)


# ---------- painting ----------

def generate(palette, seed, width, height, png_path, oxide_path, brush_list):
    """Build the whole painting as a list of oxide-run commands.

    Each item is a dict (one JSON command) or a "# comment" string. The same
    palette, seed, size and brush list always give the same list, because all
    randomness comes from `rng`. `brush_list` is what brushes.list_brushes()
    returned: each layer picks one brush from it at random.
    """
    rng = random.Random(seed)
    comp = Composition(rng, width, height)
    colors = palette["colors"]
    weights = role_weights(len(colors))
    pick = lambda: rng.choices(colors, weights)[0]  # a palette color, weighted 60-30-10
    accent = colors[-1]  # the last color, used sparingly
    darkest = min(colors, key=luminance)  # used for the thin marks
    short = min(width, height)
    # Brush size as a fraction of the shorter canvas side, kept within the brush limits (0.5 to 200).
    size = lambda frac: round(min(200.0, max(0.5, frac * short)), 1)

    cmds = [
        f"# Abstract: palette {palette['name']}, seed {seed}, composition {comp.rule}",
        {"command": "new_painting", "width": width, "height": height},
    ]
    # The background: a solid coat of one palette color, painted with a random brush (see background.py).
    cmds += make_background(rng, comp, colors, brush_list)

    # A random number of bold paint marks (see paint_styles.py), laid down first so the
    # shape and the cluster go on top of them. Which styles get painted is random too:
    # the six styles are shuffled and dealt out in turn, so a painting with fewer marks
    # than styles shows a random few of them (no repeats); with more marks, styles repeat.
    style_names = rng.sample(list(paint_styles.STYLES), len(paint_styles.STYLES))
    # Each mark goes on a strong point: all of them, in a balanced random order, dealt out
    # in turn the same way (a point gets a second mark, nudged, once they have all been used).
    mark_points = strong.pick_points(rng, comp.points, len(comp.points))
    mark_count = rng.randint(*MARK_COUNT)
    mark_cmds = []
    for i in range(mark_count):
        mark_cmds += make_paint_mark(
            rng, comp, colors, weights, brush_list,
            style=style_names[i % len(style_names)], point=mark_points[i % len(mark_points)]) + [{"command": "dry"}]
    if mark_cmds:
        # All the marks share one layer, dried after each so a mark doesn't smear the one before it.
        cmds += [f"# Paint marks ({mark_count})", {"command": "add_layer", "name": "Paint marks"}] + mark_cmds

    # Two different strong points, so the shape and the cluster don't land on the
    # same spot: the first is random, the second is the diagonal opposite of it.
    shape_point, cluster_point = strong.pick_points(rng, comp.points, 2)

    # Maybe a random shape right on top of the background (see make_shape).
    # It gets its own layer, and is dried so the layers after it don't smear it.
    shape_cmds = make_shape(rng, comp, colors, weights, brush_list, chance=SHAPE_CHANCE, point=shape_point)
    if shape_cmds:
        cmds += [shape_cmds[0], {"command": "add_layer", "name": "Shape"}] + shape_cmds[1:] + [{"command": "dry"}]

    # Maybe a random cluster of marks, the same way (see make_cluster).
    cluster_cmds = make_cluster(rng, comp, colors, weights, brush_list, chance=CLUSTER_CHANCE, point=cluster_point)
    if cluster_cmds:
        cmds += [cluster_cmds[0], {"command": "add_layer", "name": "Cluster"}] + cluster_cmds[1:] + [{"command": "dry"}]

    # Finish by exporting a PNG and saving the editable .oxide file.
    finish = finish_commands(png_path, oxide_path)
    # With the stroke layers off (see STROKE_LAYERS), stop here: background, paint marks, shape and cluster only.
    if not STROKE_LAYERS:
        return cmds + finish

    # 1. Ground: a few broad, lean strokes across the whole canvas.
    # Each stage gets its own layer. Ground colors skip the accent (the last color).
    # Lean, well-loaded paint; the brush itself is random.
    ground_brush = brushes.random_brush(rng, brush_list)
    cmds += [f"# Ground (brush: {ground_brush['name']})", {"command": "add_layer", "name": "Ground"},
             {"command": "set_tool", "tool": "oils"},
             brush_command(ground_brush, size=size(rng.uniform(0.09, 0.14)), oiliness=0.2, loading=0.9)]
    for _ in range(rng.randint(7, 11)):
        cmds.append(color_command(rng.choices(colors[:-1] or colors, weights[:-1] or weights)[0]))
        cmds.append({"command": "stroke", "smooth": True,
                     "points": comp.flow_path(comp.start_point(0.2), rng.uniform(0.6, 1.2) * width)})
    cmds.append({"command": "dry"})  # let the ground set so shapes sit on top instead of mixing in

    # 2. Shapes: medium strokes that follow the flow, gathering near the focal point.
    shapes_brush = brushes.random_brush(rng, brush_list)
    cmds += [f"# Shapes (brush: {shapes_brush['name']})", {"command": "add_layer", "name": "Shapes"},
             {"command": "set_tool", "tool": rng.choice(["oils", "oils", "knife"])},
             brush_command(shapes_brush)]
    for _ in range(rng.randint(14, 24)):
        # The same brush for the whole layer, but a new size and paint feel for every stroke.
        cmds.append({"command": "set_brush", "size": size(rng.uniform(0.02, 0.06)),
                     "oiliness": round(rng.uniform(0.3, 0.7), 2), "thickness": round(rng.uniform(0.3, 0.8), 2)})
        cmds.append(color_command(pick()))
        against = rng.random() < 0.2  # a few strokes cut across the flow for tension
        cmds.append({"command": "stroke", "smooth": True,
                     "points": comp.flow_path(comp.start_point(0.55), rng.uniform(0.12, 0.45) * width, against)})

    # 3. Marks: thin dark lines that echo the flow.
    marks_brush = brushes.random_brush(rng, brush_list)
    cmds += [f"# Marks (brush: {marks_brush['name']})", {"command": "add_layer", "name": "Marks"},
             {"command": "set_tool", "tool": rng.choice(["ink", "pencil", "marker"])},
             color_command(darkest), brush_command(marks_brush)]
    for _ in range(rng.randint(5, 12)):
        # Very thin: only the size changes from stroke to stroke.
        cmds.append({"command": "set_brush", "size": size(rng.uniform(0.002, 0.008))})
        cmds.append({"command": "stroke", "smooth": True,
                     "points": comp.flow_path(comp.start_point(0.5), rng.uniform(0.1, 0.5) * width,
                                              against=rng.random() < 0.3)})

    # 4. Accent: a few small touches of the accent color at the focal point, plus some dots.
    accent_brush = brushes.random_brush(rng, brush_list)
    cmds += [f"# Accent (brush: {accent_brush['name']})", {"command": "set_tool", "tool": "oils"}, color_command(accent),
             brush_command(accent_brush, size=size(rng.uniform(0.012, 0.025)), oiliness=0.3, thickness=0.8)]
    for _ in range(rng.randint(2, 4)):
        cmds.append({"command": "stroke", "smooth": True,
                     "points": comp.flow_path(comp.start_point(0.9), rng.uniform(0.04, 0.12) * width)})
    # Dots: a very short two-point stroke in a random direction (a = angle, r = length, p = pressure).
    for _ in range(rng.randint(3, 9)):
        x, y = comp.start_point(0.7)
        a, r, p = rng.uniform(0, math.tau), size(0.01), round(rng.uniform(0.6, 1), 2)
        cmds.append({"command": "stroke", "points": [[round(x, 1), round(y, 1), p],
                                                     [round(x + math.cos(a) * r, 1), round(y + math.sin(a) * r, 1), p]]})

    return cmds + finish


def main():
    """The entry point: the command line is shared by every conductor (see cli.py)."""
    cli.run(generate, "abstract", "Generate random abstract paintings for oxide-run.")


if __name__ == "__main__":
    main()
