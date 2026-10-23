"""The shared "make" steps: get values, work out where an object goes, send it to its object file.

Each `make_...` function does the same few steps for one kind of object, so every conductor (a file
like abstract.py) can use them instead of repeating them:
  1. get values: random ones from randomizers.py (or the conductor edits them first)
  2. work out the position: on a strong point of the composition, nudged and kept on the canvas
  3. send it all to the object file (background.py, paint_styles.py, shapes.py, cluster.py, tree.py)
and return the oxide-run commands. The object files never randomize or place themselves, so a
conductor that wants exact values skips these and calls the object files directly.

`comp` is a composition.Composition (these functions only use comp.w, comp.h and comp.anchor).
"""

import background
import cluster
import ground
import paint_styles
import randomizers
import shapes
import tree


def make_background(rng, comp, colors, brush_list):
    """The background: a random palette color, paper and brush (randomizers.background_args)."""
    return background.background(width=comp.w, height=comp.h, **randomizers.background_args(rng, colors, brush_list))


def make_paint_mark(rng, comp, colors, weights, brush_list, style=None, point=None, tuning=None):
    """One bold paint mark (see paint_styles.py): random values from randomizers.style_args,
    centered on a strong point (`point`, or a random one), nudged and kept on the canvas.
    `style` names the style; leave it out for a random one. `tuning` ({"weights": ..., "bias": ...},
    see brushes.py) limits and leans the paint settings; the same goes for make_shape and make_cluster."""
    a = randomizers.style_args(rng, colors, weights, brush_list, min(comp.w, comp.h), name=style, tuning=tuning)
    x, y = comp.anchor(paint_styles.reach(a["size"]), point)
    marks = paint_styles.STYLES[a["name"]](
        a["brush"], a["color"], round(x), round(y), a["size"], rotation=a["rotation"],
        brush_size=a["brush_size"], paint=a["paint"], seed=a["seed"], **a["options"])
    return [f"# Paint mark ({a['name']}, brush: {a['brush']['name']})"] + marks


def make_shape(rng, comp, colors, weights, brush_list, chance=0.5, point=None, tuning=None):
    """Maybe one shape (see shapes.py); returns its commands, or [] for no shape.

    Random values from randomizers.shape_args (which shape, brush, color, size, fill,
    rotation, tool, brush size, brush settings), centered on a strong point (`point`, or a
    random one), nudged and kept on the canvas.
    """
    # 1. Whether to paint a shape at all.
    #if rng.random() >= chance:
      #  return []

    a = randomizers.shape_args(rng, colors, weights, brush_list, min(comp.w, comp.h), tuning)
    x, y = comp.anchor(shapes.reach(a["kind"], a["size"], a["brush_size"]), point)
    paint = getattr(shapes, a["kind"])  # shapes.circle, shapes.square or shapes.triangle
    return [f"# Shape ({a['kind']}, brush: {a['brush']['name']})"] + paint(
        a["brush"], a["color"], round(x), round(y), a["size"], fill=a["fill"], rotation=a["rotation"],
        tool=a["tool"], brush_size=a["brush_size"], settings=a["settings"])


def make_cluster(rng, comp, colors, weights, brush_list, chance=0.5, point=None, tuning=None):
    """Maybe one cluster of marks (see cluster.py); returns its commands, or [] for none.

    Random values and marks from randomizers.cluster_args, centered on a strong point
    (`point`, or a random one), nudged and kept on the canvas.
    """
    # 1. Whether to paint a cluster at all.
    if rng.random() >= chance:
        return []

    a = randomizers.cluster_args(rng, colors, weights, brush_list, min(comp.w, comp.h), tuning)
    x, y = comp.anchor(cluster.Cluster.reach(a["size"], a["brush_size"]), point)
    c = cluster.Cluster(x=round(x), y=round(y), **a)
    return ["# " + c.label] + c.commands()


def make_tree(rng, comp, colors, weights, brush_list, point=None, tuning=None, base=None, short=None):
    """One tree (see tree.py), painted from the palette `colors`: random values from
    randomizers.tree_args, the whole tree centered on a strong point (`point`, or a random one),
    nudged and kept on the canvas. (A tree's x, y is its BASE, so the base goes half the tree's
    height below the centre the composition picks.)

    `base` = (x, y) puts the trunk's base exactly there instead (no anchoring: a forest wants trees
    standing on the ground, and cropped at the edges is fine). `short` is the size the tree is scaled
    to, in place of the canvas's shorter side: a small one makes a small, far-away tree."""
    a = randomizers.tree_args(rng, colors, weights, brush_list, short or min(comp.w, comp.h), tuning)
    if base is not None:
        x, y = base
    else:
        reach = tree.Tree.reach(a["height"], a["crown_width"], a["lean"], a["crown_shift"], a["trunk_share"])
        x, y = comp.anchor(reach, point)
        y += a["height"] / 2
    t = tree.Tree(x=round(x), y=round(y), **a)
    return ["# " + t.label] + t.commands()


def make_ground(rng, comp, colors, brush_list, horizon, avoid=None):
    """The ground below `horizon` (pixels from the top): a palette color other than `avoid` (the sky
    color), a random brush and a texture seed (randomizers.ground_args)."""
    return ground.ground(width=comp.w, height=comp.h, horizon=horizon,
                         **randomizers.ground_args(rng, colors, brush_list, avoid))

