#!/usr/bin/env python3
"""The second conductor: a background, then plant1 painted a set number of times at random places.

A conductor decides what goes on the canvas and calls the classes to draw it (see abstract.py,
the first one). This one paints a background coat, then `plant1` (a plant Kevin painted by hand
in the app and converted with recordings.py; see recorded/plant1.py) PLANT_COUNT times, each at
a random place on the canvas, all the same size, in the colors and brush it was recorded with.
Everything shared (the palette, the composition, the background, the command line) comes from
the shared modules, so this file holds only what is particular to this painting.

    python3 scripts/abstract/abstract2.py                       # random palette and seed
    python3 scripts/abstract/abstract2.py --palette swan --seed 7 --run
    python3 scripts/abstract/abstract2.py --count 5 --run --out ~/Desktop/abstract2
    python3 scripts/abstract/abstract2.py --list-palettes

The options are the same as abstract.py's (see cli.py).
"""

import random

import cli
from commands import finish
from composition import Composition
from makers import make_background
from recorded import plant1, falltree1


# How many times plant1 is painted.
PLANT_COUNT = 12

# How big each plant is: how far it reaches from its middle, as a share of half the canvas's
# shorter side (1 = one plant just fits the canvas; 0.3 = about a third of that).
PLANT_SIZE = 0.3


def generate(palette, seed, width, height, png_path, oxide_path, brush_list):
    """Build the whole painting as a list of oxide-run commands.

    Each item is a dict (one JSON command) or a "# comment" string. The same palette, seed, size
    and brush list always give the same list, because all randomness comes from `rng`.
    """
    rng = random.Random(seed)
    comp = Composition(rng, width, height)
    colors = palette["colors"]

    cmds = [
        f"# Abstract2: palette {palette['name']}, seed {seed}, {PLANT_COUNT} plants",
        {"command": "new_painting", "width": width, "height": height},
    ]
    # The background: a solid coat of one palette color, painted with a random brush (see background.py).
    cmds += make_background(rng, comp, colors, brush_list)

    # The plants, all on one layer, each at a random place anywhere on the canvas, kept fully on it
    # (its middle at least its reach from every edge). A `dry` after each, so a plant painted over
    # another doesn't smear into its wet paint.
    scale = PLANT_SIZE * min(width, height) / 2 / plant1.reach()
    reach = plant1.reach(scale)
    cmds += [f"# Plants ({PLANT_COUNT})", {"command": "add_layer", "name": "Plants"}]
    for _ in range(PLANT_COUNT):
        # (If a plant is wider than the canvas, it goes in the middle.)
        x = round(rng.uniform(min(reach, width / 2), max(width - reach, width / 2)), 1)
        y = round(rng.uniform(min(reach, height / 2), max(height - reach, height / 2)), 1)
        cmds += [f"# Plant at ({x:.0f}, {y:.0f})"] + plant1.plant1(x=x, y=y, scale=scale) + [{"command": "dry"}]

    # The fall trees, the same way: each recording needs its own scale and reach (falltree1 is much
    # smaller than plant1, so plant1's scale would paint it about 40 px across, as a thick lump).
    scale = PLANT_SIZE * min(width, height) / 2 / falltree1.reach()
    reach = falltree1.reach(scale)
    for _ in range(PLANT_COUNT):
        x = round(rng.uniform(min(reach, width / 2), max(width - reach, width / 2)), 1)
        y = round(rng.uniform(min(reach, height / 2), max(height - reach, height / 2)), 1)
        cmds += [f"# falltree at ({x:.0f}, {y:.0f})"] + falltree1.falltree1(x=x, y=y, scale=scale) + [{"command": "dry"}]

    # Finish by exporting a PNG and saving the editable .oxide file.
    return cmds + finish(png_path, oxide_path)


def main():
    """The entry point: the command line is shared by every conductor (see cli.py)."""
    cli.run(generate, "abstract2", "Generate abstract paintings: a background and plant1 painted at random places, for oxide-run.")


if __name__ == "__main__":
    main()
