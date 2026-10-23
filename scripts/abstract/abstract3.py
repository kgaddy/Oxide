#!/usr/bin/env python3
"""The third conductor: a forest. A sky, a ground, and many trees standing on it, far to near.

A conductor decides what goes on the canvas and calls the classes to draw it (see abstract.py,
the first one). This one paints:
  1. the sky: the background, a solid coat of one palette color
  2. the ground: a coat of another palette color from the horizon down
  3. a random number of trees, painted from the farthest to the nearest. Each has a depth
     (0 = far, 1 = near): a far tree is small, stands near the horizon and is hazed toward the sky
     color; a near tree is big, stands low on the canvas and has the full palette colors. Near trees
     overlap and cover far ones, which is what makes it look like a forest.

    python3 scripts/abstract/abstract3.py                       # random palette and seed
    python3 scripts/abstract/abstract3.py --palette moss --seed 7 --run
    python3 scripts/abstract/abstract3.py --count 5 --run --out ~/Desktop/forest
    python3 scripts/abstract/abstract3.py --list-palettes

The options are the same as abstract.py's (see cli.py).
"""

import random

import cli
from commands import finish
from composition import Composition
from makers import make_background, make_ground, make_tree
from palettes import blend, role_weights

# How many trees a painting gets: a random whole number from the lowest to the highest here
# (both ends included).
TREE_COUNT = (8, 30)

# Where the horizon is, as a share of the canvas height from the top: a random one in this range.
HORIZON = (0.4, 0.6)

# How big the trees are, as a share of the size the tree maker would use for a lone tree: the
# farthest tree is FAR_SCALE of it and the nearest NEAR_SCALE (a share of the canvas's shorter side).
FAR_SCALE = 0.3
NEAR_SCALE = 0.85

# How far a tree sits from the horizon at its base, as a share of the ground's height: far trees
# stand just below the horizon, near ones at the very bottom.
FAR_BASE = 0.05

# How hazed the farthest trees are: their colors are mixed this much (0 to 1) toward the sky color.
# The haze fades to nothing for the nearest tree.
HAZE = 0.7

# How random this conductor's brush settings are (see brushes.py). "weights": 0 to 1 per setting,
# "bias": a power per setting (above 1 leans low). A setting not listed is left free and even.
BRUSH_TUNING = {
    "weights": {"oiliness": 0.3},
    "bias": {"loading": 2.0},
}


def generate(palette, seed, width, height, png_path, oxide_path, brush_list):
    """Build the whole painting as a list of oxide-run commands.

    Each item is a dict (one JSON command) or a "# comment" string. The same palette, seed, size
    and brush list always give the same list, because all randomness comes from `rng`.
    """
    rng = random.Random(seed)
    comp = Composition(rng, width, height)
    colors = palette["colors"]
    weights = role_weights(len(colors))

    tree_count = rng.randint(*TREE_COUNT)
    horizon = round(rng.uniform(*HORIZON) * height)

    cmds = [
        f"# Abstract3: palette {palette['name']}, seed {seed}, {tree_count} trees, horizon {horizon}",
        {"command": "new_painting", "width": width, "height": height},
    ]
    # The sky: a solid coat of one palette color. Its color is found by reading the command that sets
    # the paper, since make_background chose it at random.
    sky_cmds = make_background(rng, comp, colors, brush_list)
    sky = next(c for c in map(_as_dict, sky_cmds) if c and c.get("command") == "set_paper")["color"]
    cmds += sky_cmds

    # The ground, on its own layer, in a different palette color (if the palette has one).
    cmds += [{"command": "add_layer", "name": "Ground"}] + make_ground(rng, comp, colors, brush_list, horizon, avoid=sky)

    # The trees, on their own layer, far to near so the near ones cover the far ones.
    depths = sorted(rng.random() for _ in range(tree_count))
    tree_cmds = []
    for depth in depths:
        # A far tree stands near the horizon, a near one at the bottom; and it is small or big.
        base = (round(rng.uniform(-0.03, 1.03) * width), round(horizon + (height - horizon) * (FAR_BASE + (1 - FAR_BASE) * depth)))
        scale = FAR_SCALE + (NEAR_SCALE - FAR_SCALE) * depth
        # Far trees are hazed toward the sky color: every palette color mixed toward it.
        hazed = [blend(c, sky, HAZE * (1 - depth)) for c in colors]
        tree_cmds += make_tree(rng, comp, hazed, weights, brush_list, tuning=BRUSH_TUNING, base=base,
                               short=min(width, height) * scale) + [{"command": "dry"}]
    cmds += [f"# Trees ({tree_count})", {"command": "add_layer", "name": "Trees"}] + tree_cmds

    # Finish by exporting a PNG and saving the editable .oxide file.
    return cmds + finish(png_path, oxide_path)


def _as_dict(command):
    """A command as a dict: the background gives its commands as JSON strings (and "# comment" lines)."""
    import json
    return json.loads(command) if isinstance(command, str) and command.startswith("{") else (command if isinstance(command, dict) else None)


def main():
    """The entry point: the command line is shared by every conductor (see cli.py)."""
    cli.run(generate, "abstract3", "Generate forest paintings for oxide-run.")


if __name__ == "__main__":
    main()
