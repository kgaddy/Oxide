import json
from pathlib import Path
import sys



SCRIPT_DIR = Path(__file__).resolve().parent  # the folder this file is in
PALETTE_FILE = SCRIPT_DIR / "palettes.json"

def load_palettes():
    """Read palettes.json and return it as a dict of palette name -> list of hex colors.

    Example result: {"moss": ["#4F7329", "#CC9938", ...], "swan": [...], ...}
    Called every time a palette is listed, looked up or picked at random, so
    edits to palettes.json take effect on the next run with no other steps.
    """
    # read_text() gives the file's contents as a string; json.loads turns that
    # string into a Python dict.
    palettes = json.loads(PALETTE_FILE.read_text())
    # An empty file would leave nothing to paint with, so stop with a message.
    if not palettes:
        sys.exit(f"abstract: no palettes found in {PALETTE_FILE}")
    return palettes

def load_palette(name):
    """Find one palette by name and return {"name": ..., "colors": [...]}.

    Capitals don't matter: "--palette LEMONADE" finds "Lemonade". If there is
    no such palette, the script stops and prints every name you could use.
    """
    palettes = load_palettes()
    # Walk the palette names and take the first one that matches ignoring
    # case; `None` if none match. `key` is the name as spelled in the file.
    key = next((k for k in palettes if k.lower() == name.lower()), None)
    if key is None:
        sys.exit(f"abstract: no palette '{name}' (have: {', '.join(palettes)})")
    # Return the file's spelling of the name (used in the output file names)
    # along with its colors.
    return {"name": key, "colors": palettes[key]}


def role_weights(n):
    """How likely each palette color is to be picked: 60-30-10 across the palette.

    The first color gets 60%, the last (accent) 10%, and the middle colors
    share the remaining 30% equally. The weights add up to 1.

    `n` is how many colors the palette has. Example for n = 5:
    [0.6, 0.1, 0.1, 0.1, 0.1]. The weights line up with the colors list, so
    weights[0] belongs to colors[0], and so on.
    """
    # One color: it is picked every time.
    if n == 1:
        return [1.0]
    # Two colors: there is no middle, so split 70/30.
    if n == 2:
        return [0.7, 0.3]
    # Three or more: each of the n - 2 middle colors gets an equal slice of 30%.
    middle = 0.3 / (n - 2)
    return [0.6] + [middle] * (n - 2) + [0.1]


def luminance(color):
    """How bright a hex color looks, as a number from 0 (black) to 1 (white).

    Used to find the darkest color in a palette, which becomes the thin
    "marks" lines. Example: luminance("#000000") is 0.0, luminance("#ffffff")
    is 1.0, and luminance("#808080") is about 0.5.
    """
    # Drop the leading "#": "#a7d129" becomes "a7d129".
    h = color.lstrip("#")
    # Some hex colors are written short ("#fa0" means "#ffaa00"); double each
    # digit so every color has the same 6-digit shape.
    if len(h) == 3:
        h = "".join(c * 2 for c in h)
    # Split "a7d129" into its red, green and blue pairs ("a7", "d1", "29"),
    # read each pair as a base-16 number (0 to 255), and divide by 255 so
    # each channel is between 0 and 1.
    r, g, b = (int(h[i:i + 2], 16) / 255 for i in (0, 2, 4))
    # Add the channels up, but not equally: the eye sees green as brightest
    # and blue as darkest, so pure green counts for much more than pure blue.
    # The three weights add up to 1, which keeps the answer between 0 and 1.
    return 0.299 * r + 0.587 * g + 0.114 * b


def blend(color, other, amount):
    """`color` mixed toward `other` (both hex strings) by `amount` (0 = all `color`, 1 = all `other`).

    Used for distance haze: a far-away tree is its palette color mixed toward the sky color.
    Example: blend("#000000", "#ffffff", 0.5) is "#808080".
    """
    def channels(c):
        h = c.lstrip("#")
        if len(h) == 3:
            h = "".join(ch * 2 for ch in h)
        return [int(h[i:i + 2], 16) for i in (0, 2, 4)]
    mixed = [round(a + (b - a) * amount) for a, b in zip(channels(color), channels(other))]
    return "#{:02X}{:02X}{:02X}".format(*mixed)

