"""Ask Oxide for every brush in the app (built-in and saved) as a Python list.

    import brushes
    all_brushes = brushes.list_brushes()
    names = [b["name"] for b in all_brushes]
    brush = brushes.random_brush(brushes=all_brushes)   # one random brush
    settings = brushes.random_settings(rng, brush)      # a random variation of it

Each brush is a dict like {"name": "Bristle Flat", "group": "Bristle",
"saved": False, "size": 18.0, "hardness": 0.7, ...}. The setting names match
the oxide-run set_brush command, and set_brush also takes {"preset": "<name>"}
to select a brush by name.
"""

import json
import random
import subprocess
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent.parent


def list_brushes():
    """Run the list_brushes command in oxide-run and return its brushes."""
    # oxide-run reads one JSON command per line from stdin and prints one
    # JSON reply per command. cargo builds it first if needed, so the first
    # call can be slow.
    result = subprocess.run(
        ["cargo", "run", "-q", "--release", "-p", "oxide-agent", "--bin", "oxide-run"],
        input='{"command": "list_brushes"}\n', cwd=REPO, capture_output=True, text=True)
    if result.returncode != 0:
        raise RuntimeError(f"list_brushes failed:\n{result.stdout}{result.stderr}")
    return json.loads(result.stdout)["result"]


def random_brush(rng=None, brushes=None):
    """Return one brush (a dict from list_brushes) chosen at random.

    `rng` is a random.Random, so a seeded generator gives the same pick every
    time (abstract.py passes its own to keep a painting repeatable); the
    default is Python's global random. `brushes` is a list you already got
    from list_brushes(). Pass it when picking many times, because each
    list_brushes() call starts oxide-run, which is slow.
    """
    rng = rng or random
    return rng.choice(brushes or list_brushes())


def _jitter(rng, value, low, high, amount):
    """Move `value` by a random amount, up to `amount` (0 to 1) of the way
    across the range from `low` to `high`, and keep it inside that range."""
    moved = value + rng.uniform(-1, 1) * amount * (high - low)
    return round(min(max(moved, low), high), 2)


# Paint settings (they act on oils) that no brush in the list carries, so there is no value of
# the brush's own to vary: (lowest, highest, middle). The middle is where the setting sits when
# its weight (below) is 0. Narrow a range to pin a setting down; widen it for wilder paint.
PAINT_SETTINGS = {
    "opacity": (0.2, 1.0, 0.85),
    "loading": (0.2, 1.0, 0.75),
    "length": (0.2, 1.0, 0.7),
    "oiliness": (0.0, 1.0, 0.4),
    "thickness": (0.0, 1.0, 0.5),
    "grain": (0.0, 1.0, 0.5),
}

# How much each setting is allowed to vary: a weight from 0 to 1 per setting name (shape or
# paint). 1 = the full spread (the default for any setting not listed); 0 = never varies (a
# paint setting stays at its middle, a shape setting at the brush's own value); in between
# = that share. For a paint setting the weight shrinks the range toward its middle; for a
# shape setting it multiplies `amount`. Lower one to limit its randomness, for example
# {"oiliness": 0.2, "opacity": 0.5} keeps the paint lean and mostly solid.
SETTING_WEIGHTS = {}

# Which way each paint setting leans: a power per setting name. 1 = even (every value in the
# band equally likely, the default for any setting not listed). Above 1 leans LOW (2 puts half
# the draws in the lowest quarter of the band; 3 leans harder); below 1 leans HIGH (0.5). The
# weights above set how wide the band is; the bias sets where in the band values tend to land,
# and they combine. Loading leans low so most marks carry thin, dry paint.
SETTING_BIAS = {"loading": 2.0}


def tuning_for(tuning=None):
    """The (weights, bias) tables to use: a conductor's `tuning` dict, or this file's own.

    A tuning is {"weights": {...}, "bias": {...}} (either key can be left out, and then that
    table's default above is used). A conductor builds one and passes it down; see
    randomizers.py. A tuning that lists a setting replaces the default for it only if you
    list it: {"weights": {}} means no limits at all, not the default weights.
    """
    tuning = tuning or {}
    return tuning.get("weights", SETTING_WEIGHTS), tuning.get("bias", SETTING_BIAS)


def lean(rng, name, low, high, tuning=None):
    """A random value from `low` to `high` leaning the way the bias says for `name`.

    One random draw whatever the bias, so changing a bias never shifts other random choices.
    """
    bias = tuning_for(tuning)[1]
    return round(low + (high - low) * rng.random() ** bias.get(name, 1.0), 2)

# Shape settings a brush in the list does carry, varied around its own value:
# (lowest, highest). The ones in EFFECTS only change if the brush already has some.
SHAPE_SETTINGS = {"hardness": (0.0, 1.0), "roundness": (0.05, 1.0), "roughness": (0.0, 1.0),
                  "streaks": (0.0, 1.0), "lift_off": (0.0, 1.0), "taper": (0.0, 1.0),
                  "jitter_angle": (0.0, 1.0), "jitter_size": (0.0, 1.0), "scatter": (0.0, 1.0)}
EFFECTS = {"taper", "jitter_angle", "jitter_size", "scatter"}


def random_settings(rng, brush, amount=0.3, tuning=None):
    """Random set_brush settings for `brush`: the common way to vary a brush.

    Use it for shapes, clusters and anything else that paints, so every kind
    of mark varies its brush the same way. It returns a dict of set_brush
    settings (no "preset" and no "size", which the caller sets):

        settings = brushes.random_settings(rng, brush, amount=0.3)
        {"command": "set_brush", "preset": brush["name"], "size": 20, **settings}

    `tuning` ({"weights": {...}, "bias": {...}}; default this file's SETTING_WEIGHTS and
    SETTING_BIAS) limits and leans individual settings; a conductor passes its own.
    `amount` (0 to 1) is how far each setting can move. 0 changes nothing (an
    empty dict); at 1 a setting can land anywhere in its range.
      - Shape settings (hardness, roundness, roughness, streaks, lift_off,
        spacing, angle, and a bristle brush's hairs) move around the brush's
        own values, so a Flat brush stays recognizably flat.
        taper, jitter_angle, jitter_size and scatter only move when the brush
        already uses them (a round brush doesn't suddenly scatter).
      - Paint settings (opacity, loading, length, oiliness, thickness, grain) are
        drawn from their range (PAINT_SETTINGS, shrunk by SETTING_WEIGHTS), since the brush list doesn't
        carry them. `amount` doesn't narrow these; only 0 (no variation) skips them.
    Not varied: the tip (texture), the rotation mode and which ends taper,
    because changing those changes what kind of brush it is.
    """
    if amount <= 0:
        return {}
    weights = tuning_for(tuning)[0]
    weight = lambda name: min(max(weights.get(name, 1.0), 0.0), 1.0)
    settings = {}
    for name, (low, high) in SHAPE_SETTINGS.items():
        if brush.get(name) is None or (name in EFFECTS and not brush[name]):
            continue
        settings[name] = _jitter(rng, brush[name], low, high, amount * weight(name))
    # Spacing moves by a share of itself: texture tips need wide spacing, and
    # a fixed step would make a dense brush sparse or the other way round.
    if brush.get("spacing"):
        settings["spacing"] = round(min(max(brush["spacing"] * (1 + rng.uniform(-1, 1) * amount), 0.02), 1.0), 2)
    # The tip's angle: a turn of up to `amount` of a half turn either way.
    settings["angle"] = round(brush.get("angle", 0.0) + rng.uniform(-1, 1) * amount * 90, 1)
    hairs = brush.get("bristles")
    if hairs:
        settings["bristles"] = int(min(max(hairs["count"] * (1 + rng.uniform(-1, 1) * amount), 4), 300))
        for name in ("thickness", "spread", "stiffness"):
            settings["bristle_" + name] = _jitter(rng, hairs[name], 0.0, 1.0, amount)
    for name, (low, high, middle) in PAINT_SETTINGS.items():
        # The range shrinks toward the middle as the weight falls: full range at 1, the middle at 0.
        w = weight(name)
        settings[name] = lean(rng, name, middle - w * (middle - low), middle + w * (high - middle), tuning)
    return settings


if __name__ == "__main__":
    for brush in list_brushes():
        print(f"{brush['group']:16} {brush['name']}")
