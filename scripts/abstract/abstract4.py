#!/usr/bin/env python3
"""The fourth conductor: a background, then mark1 painted a set number of times at random places.

A conductor decides what goes on the canvas and calls the classes to draw it (see abstract.py,
the first one). This one paints a background coat, then `mark1` (a mark Kevin painted by hand in
the app and converted with recordings.py; see recorded/mark1.py) MARK_COUNT times, each at a
random place on the canvas, all the same size, each in shades of one palette color, with the brushes it was recorded with.
Everything shared (the palette, the composition, the background, the command line) comes from
the shared modules, so this file holds only what is particular to this painting.

From a recording to a painting (run from the repo root, /Users/kevingaddy/Documents/Rust/Oxide):

1. Record the mark in the app: press ⏺ Record in the status bar, paint it, press ⏹ Stop and
   save the .jsonl (here ~/Desktop/mark1.jsonl). Open the app with:
    cargo run -p oxide-app

2. Check the recording paints the same outside the app (optional):
    cargo run -q --release -p oxide-agent --bin oxide-run -- ~/Desktop/mark1.jsonl

3. Convert it to a Python function (writes scripts/abstract/recorded/mark1.py; --name and --out
   change the name and folder), and paint a demo of the converted function:
    python3 scripts/abstract/recordings.py ~/Desktop/mark1.jsonl
    python3 scripts/abstract/recorded/mark1.py --run --out ~/Desktop

4. Paint with this conductor (writes a .jsonl script; --run also paints the .png and .oxide):
    python3 scripts/abstract/abstract4.py                       # random palette and seed, script only
    python3 scripts/abstract/abstract4.py --palette moss --seed 7 --run --out ~/Desktop/abstract4
    python3 scripts/abstract/abstract4.py --count 5 --run --out ~/Desktop/abstract4   # five, seeds in a row
    python3 scripts/abstract/abstract4.py --width 1600 --height 1000 --run            # another canvas size
    python3 scripts/abstract/abstract4.py --list-palettes                             # the palette names

5. Run the tests after a change:
    python3 scripts/abstract/test_items.py TestAbstract4        # this conductor
    python3 scripts/abstract/test_items.py                      # all the fast tests
    python3 scripts/abstract/test_items.py --app                # also through the real app (slow)

What to change: MARK_COUNT (how many) and MARK_SIZE (how big) below; in the mark1.mark1(...)
call, `shade=shade` (shades of one palette color per mark), or `palette=colors` (the whole
palette by lightness), nothing (the recorded colors), and `brush="Round Hard"` (another brush).
The options are the same as abstract.py's (see cli.py).
"""

import random

import cli
import recordings
from commands import finish
from composition import Composition
from makers import make_background
from recorded import mark1, mark2


# How many times mark1 is painted.
MARK_COUNT = 8

# How big each mark is: how far it reaches from its middle, as a share of half the canvas's
# shorter side (1 = one mark just fits the canvas; 0.5 = about half of that).
MARK_SIZE = 0.5


def generate(palette, seed, width, height, png_path, oxide_path, brush_list):
    """Build the whole painting as a list of oxide-run commands.

    Each item is a dict (one JSON command) or a "# comment" string. The same palette, seed, size
    and brush list always give the same list, because all randomness comes from `rng`.
    """
    rng = random.Random(seed)
    comp = Composition(rng, width, height)
    colors = palette["colors"]

    cmds = [
        f"# Abstract4: palette {palette['name']}, seed {seed}, {MARK_COUNT} marks",
        {"command": "new_painting", "width": width, "height": height},
    ]
    # The background: a solid coat of one palette color, painted with a random brush (see background.py).
    cmds += make_background(rng, comp, colors, brush_list)

    # The marks, all on one layer, each at a random place anywhere on the canvas, kept fully on it.
    # mark1 is long and flat, so it is kept on by its own box (half its width or height, plus its
    # widest brush, from every edge) rather than by its reach, a circle that would hold it to a
    # band across the middle. A `dry` after each, so a mark painted over another doesn't smear
    # into its wet paint.
    scale = MARK_SIZE * min(width, height) / 2 / mark1.reach()
    mark = mark1.mark1(x=0, y=0, scale=scale)
    x0, y0, x1, y1 = recordings.bounds(mark)

    shade = rng.choice(colors)
    for _ in range(MARK_COUNT):

        brush = max(c["size"] for c in mark if isinstance(c, dict) and c["command"] == "set_brush" and "size" in c) / 2
        half_w, half_h = (x1 - x0) / 2 + brush, (y1 - y0) / 2 + brush
        cmds += [f"# Marks ({MARK_COUNT})", {"command": "add_layer", "name": "Marks"}]

        # (If a mark is wider than the canvas, it goes in the middle.)
        x = round(rng.uniform(min(half_w, width / 2), max(width - half_w, width / 2)), 1)
        y = round(rng.uniform(min(half_h, height / 2), max(height - half_h, height / 2)), 1)
        # Each mark in shades of one random palette color: every recorded color takes that color's hue
        # and keeps its own lightness (see recordings.shade_swap).

        cmds += [f"# Mark at ({x:.0f}, {y:.0f}) in shades of {shade}"] + mark1.mark1(x=x, y=y, scale=scale, shade=shade) +[{"command": "dry"}]

    for _ in range(MARK_COUNT + 10):

        brush = max(c["size"] for c in mark if isinstance(c, dict) and c["command"] == "set_brush" and "size" in c) / 2
        half_w, half_h = (x1 - x0) / 2 + brush, (y1 - y0) / 2 + brush
        cmds += [f"# Marks ({MARK_COUNT})", {"command": "add_layer", "name": "Marks"}]

        # (If a mark is wider than the canvas, it goes in the middle.)
        x = round(rng.uniform(min(half_w, width / 2), max(width - half_w, width / 2)), 1)
        y = round(rng.uniform(min(half_h, height / 2), max(height - half_h, height / 2)), 1)
        # Each mark in shades of one random palette color: every recorded color takes that color's hue
        # and keeps its own lightness (see recordings.shade_swap).

        cmds += [f"# Mark at ({x:.0f}, {y:.0f}) in shades of {shade}"] + mark2.mark2(x=x, y=y, scale=scale, shade=shade) +[{"command": "dry"}]


    # Finish by exporting a PNG and saving the editable .oxide file.
    return cmds + finish(png_path, oxide_path)


def main():
    """The entry point: the command line is shared by every conductor (see cli.py)."""
    cli.run(generate, "abstract4", "Generate abstract paintings: a background and mark1 painted at random places, for oxide-run.")


if __name__ == "__main__":
    main()
