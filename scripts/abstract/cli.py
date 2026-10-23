"""The command line shared by every conductor.

A conductor (a file like abstract.py) only has to write its own `generate`; this file does the
rest: it reads the options, picks a palette and a seed for each painting, calls the conductor's
`generate`, saves the script, and with --run paints it. The conductor's main() is one line:

    def main():
        cli.run(generate, "abstract", "Generate random abstract paintings for oxide-run.")

`generate` is called as generate(palette, seed, width, height, png_path, oxide_path, brush_list)
and returns the list of oxide-run commands (see commands.py). `name` starts the names of the files
it writes, like abstract-moss-42.jsonl, so two conductors never overwrite each other.

The options (the same for every conductor): --palette, --seed, --width, --height, --count,
--out, --run, --list-palettes (and --help).
"""

import argparse
import random
import sys
from pathlib import Path

import brushes
from commands import run_script, write_script
from palettes import load_palette, load_palettes


def run(generate, name, description, width=1200, height=900):
    """Read the command-line options, then make --count paintings with `generate`.

    For each one it picks a palette and seed, writes the .jsonl script with generate() and
    write_script(), and, with --run, paints it with run_script(). `width` and `height` are the
    defaults for --width and --height.
    """
    # Define the options you can type after the script name. argparse also provides --help.
    ap = argparse.ArgumentParser(description=description)
    ap.add_argument("--palette", help="palette name from palettes.json (default: random)")
    ap.add_argument("--seed", type=int, help="random seed (default: random); the same seed repeats a painting")
    ap.add_argument("--width", type=int, default=width)
    ap.add_argument("--height", type=int, default=height)
    ap.add_argument("--count", type=int, default=1, help="how many paintings to make")
    ap.add_argument("--out", default=".", help="folder for the .jsonl, .png and .oxide files")
    ap.add_argument("--run", action="store_true", help="paint them with oxide-run too")
    ap.add_argument("--list-palettes", action="store_true")
    args = ap.parse_args()  # read what was actually typed

    # --list-palettes: print every palette and stop without making anything.
    if args.list_palettes:
        for palette_name, colors in load_palettes().items():
            print(f"{palette_name:16} {', '.join(colors)}")
        return

    # Make the output folder if needed ("~" means your home folder).
    out = Path(args.out).expanduser()
    out.mkdir(parents=True, exist_ok=True)
    ok = True  # becomes False if any painting fails
    # Ask Oxide for its brushes once, up front (this starts oxide-run, which is slow).
    brush_list = brushes.list_brushes()
    for i in range(args.count):
        # With --seed, painting i uses seed+i; otherwise each gets a random seed.
        seed = args.seed + i if args.seed is not None else random.randrange(1_000_000)
        # Use the --palette given, or pick a random one for each painting.
        palette = load_palette(args.palette or random.choice(list(load_palettes())))
        # All three files share a name like abstract-moss-42.
        base = out / f"{name}-{palette['name'].lower()}-{seed}"
        png, oxide, script = base.with_suffix(".png"), base.with_suffix(".oxide"), base.with_suffix(".jsonl")
        # Build the commands and save them. resolve() makes the png/oxide paths
        # absolute so oxide-run saves them where we expect.
        write_script(generate(palette, seed, args.width, args.height, png.resolve(), oxide.resolve(), brush_list), script)
        print(f"{script}  (palette {palette['name']}, seed {seed})")
        if args.run:
            if run_script(script.resolve()):
                print(f"  painted {png}")
            else:
                ok = False
    sys.exit(0 if ok else 1)  # a nonzero exit code means at least one painting failed
