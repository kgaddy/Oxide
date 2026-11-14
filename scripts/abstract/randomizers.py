"""Shared random inputs for conductors.

A CONDUCTOR is a file like abstract.py: it decides what goes on the canvas and
sends the values to the object files (shapes.py, cluster.py, paint_styles.py,
background.py). The object files never randomize anything the conductor might
want to control; they only paint what they are told. So all the randomizing lives
on the conductor's side, and this file holds the common parts of it, so every
conductor can reuse them and none has to copy them:

    import randomizers
    args = randomizers.shape_args(rng, colors, weights, brush_list, short)   # a dict of values
    args["color"] = "#C2151B"                                                # ...change any you like
    cmds = shapes.circle(args["brush"], args["color"], x, y, args["size"], ...)

Each function here takes a random.Random and returns plain values (numbers, or a
dict of them) that can be sent straight to an object file, edited first, or thrown
away in favor of values you choose yourself. Nothing here paints or places anything:
the position is the conductor's job (it knows the composition).

The tables below (RANGES, STYLE_RANGES, TOOL_WEIGHTS, ...) are where the random ranges
live: widen or narrow one to change how wild the random values get.
"""

import math

import brushes
import tree
from palettes import luminance

# ---------- the ranges ----------

# The paper surfaces a background can be painted on (see set_paper for the full list).
SURFACES = ["canvas", "linen", "panel", "rough"]

# The paint tools a shape can use, and how likely each is. The numbers are relative
# weights (here they add up to 100, so each is also its percent chance). Oils is the
# heavy one. To drop a tool set its weight to 0; to add one, add its name.
TOOL_WEIGHTS = {"oils": 50, "ink": 15, "marker": 15, "pencil": 10, "knife": 10}

# The paint settings of a paint-style mark (see paint_styles.py). Each is drawn from a
# (lowest, highest) range. These give the marks their dry-brush look, so the ranges are
# fairly tight; widen one for wilder marks, or narrow it to pin the look down.
PAINT_RANGES = {
    # The bristles (the hairs of the brush).
    "bristles": (50, 110),             # how many hairs
    "bristle_thickness": (0.25, 0.5),  # how thick each hair is
    "bristle_spread": (0.4, 0.8),      # how much the hairs splay under pressure
    "bristle_stiffness": (0.4, 0.7),   # how stiff the hairs are
    # The dry-brush texture.
    "streaks": (0.5, 0.9),             # bristle streaks along the stroke
    "roughness": (0.3, 0.7),           # ragged, grainy edges
    # The paint itself (the oils settings other than loading and thickness).
    "oiliness": (0.0, 0.3),            # lean (covers) to a little oily
    "opacity": (0.85, 1.0),            # nearly solid
    # The hairlines.
    "hair_taper": (0.5, 1.0),          # how much a hairline thins to a point at its ends
    "hair_roughness": (0.1, 0.5),      # how ragged a hairline is
}

# Loading (how much paint the brush carries) and thickness (how thick the paint goes on)
# are TOTALLY random: drawn from this whole range, 0 to 1.
FULL_RANGE = (0.0, 1.0)

# The main brush's radius as a share of a paint mark's size.
BRUSH_SHARE = (0.05, 0.09)

# The structure of each paint style, as (lowest, highest) ranges. Whole numbers draw whole
# numbers, decimals draw decimals. What each means is in the style's own docstring
# (paint_styles.py). `heading` of the slash is drawn leaning either way.
STYLE_RANGES = {
    "slash": {"heading": (25.0, 55.0), "width_share": (0.4, 0.6), "hairs": (10, 22), "splatter": (40, 80)},
    "fan": {"spread": (30.0, 45.0), "hairs": (45, 85), "splatter": (40, 80)},
    "block": {"height_share": (0.55, 0.75), "hairs": (8, 16), "drips": (6, 12), "splatter": (35, 70)},
    "wedge": {"tip_shift": (-0.1, 0.1), "hairs": (10, 20), "splatter": (35, 70)},
    "swoosh": {"first": (165.0, 200.0), "sweep": (130.0, 175.0), "hairs": (10, 20), "splatter": (35, 70)},
    "flame": {"phase": (0.0, math.tau), "wavelength": (0.25, 0.45), "hairs": (10, 20), "splatter": (35, 70)},
}


# ---------- small picks ----------

def _draw(rng, low, high):
    """A random number in [low, high]: a whole number if both ends are whole numbers, else a decimal."""
    if isinstance(low, int) and isinstance(high, int):
        return rng.randint(low, high)
    return round(rng.uniform(low, high), 2)


def pick_color(rng, colors, weights):
    """A palette color, weighted (60-30-10 across the palette: see role_weights in abstract.py)."""
    return rng.choices(colors, weights)[0]


def pick_size(rng, short, low, high):
    """A size in pixels: a random share, from `low` to `high`, of the shorter canvas side `short`."""
    return round(rng.uniform(low, high) * short)


def pick_brush_size(rng, short, low=0.01, high=0.04):
    """A brush radius in pixels: a random share of the shorter canvas side, kept within the brush limits (0.5 to 200)."""
    return round(min(200.0, max(0.5, rng.uniform(low, high) * short)), 1)


def pick_tool(rng, weights=TOOL_WEIGHTS):
    """A paint tool, by weight (oils most often by default)."""
    return rng.choices(list(weights), list(weights.values()))[0]


def jitter_color(rng, color, amount):
    """`color` (hex) with each of red, green and blue moved a little, at random.

    `amount` (0 to 1) is how far a channel can move, as a share of its whole range
    (0 to 255), so 0.08 moves each channel by up to about 20.
    """
    r, g, b = (int(color.lstrip("#")[i:i + 2], 16) for i in (0, 2, 4))
    moved = [round(min(max(c + rng.uniform(-1, 1) * amount * 255, 0), 255)) for c in (r, g, b)]
    return "#{:02X}{:02X}{:02X}".format(*moved)


# ---------- the background ----------

def background_args(rng, colors, brush_list):
    """Random values for background.background: its color (any palette color, equally likely),
    the paper surface and its depth, the brush, and a texture seed. Add `width` and `height`."""
    return {"color": rng.choice(colors), "kind": rng.choice(SURFACES), "depth": round(rng.uniform(0.3, 0.6), 2),
            "brush": brushes.random_brush(rng, brush_list), "seed": rng.randrange(2 ** 31)}


def ground_args(rng, colors, brush_list, avoid=None):
    """Random values for ground.ground: a palette color (not `avoid`, the sky color, if the palette
    has another), a random brush and a texture seed. Add `width`, `height` and `horizon`."""
    choices = [c for c in colors if c != avoid] or list(colors)
    return {"color": rng.choice(choices), "brush": brushes.random_brush(rng, brush_list), "seed": rng.randrange(2 ** 31)}


# ---------- shapes ----------

def shape_args(rng, colors, weights, brush_list, short, tuning=None):
    """Random values for one shape (see shapes.py): which kind, the brush, color, size, fill,
    rotation, tool, brush size, and a random variation of the brush's settings.

    Returns a dict: kind ("circle", "square" or "triangle"), brush, color, size, fill, rotation,
    tool, brush_size, settings. The position is not here: place it yourself.
    """
    kind = rng.choice(["circle", "square", "triangle"])
    brush = brushes.random_brush(rng, brush_list)
    return {
        "kind": kind,
        "brush": brush,
        "color": pick_color(rng, colors, weights),
        "size": pick_size(rng, short, 0.08, 0.3),  # 8% to 30% of the shorter canvas side
        "fill": rng.random() < 0.5,
        "rotation": 0 if kind == "circle" else round(rng.uniform(0, 360)),  # a circle looks the same turned
        "tool": pick_tool(rng),
        "brush_size": pick_brush_size(rng, short),  # 1% to 4% of the shorter side
        # A random variation of the brush: how far its settings move is itself random (0 to 0.4).
        "settings": brushes.random_settings(rng, brush, amount=round(rng.uniform(0.0, 0.4), 2), tuning=tuning),
    }


# ---------- clusters ----------

def cluster_marks(rng, brush, color, count, brush_size, variation, color_jitter, tuning=None):
    """`count` random marks for a Cluster (see cluster.py), each a dict of explicit values:

      u, v      where it is in the oval area, from -1 to 1 each way (bunched toward the middle)
      length    its length, as a share of the cluster's size (0.08 to 0.35)
      heading   its direction, in radians (any)
      bend      how far its middle is pushed to the side, as a share of its length (-0.3 to 0.3)
      pressure  three pressures: start, middle, end (light at the ends, firmer in the middle)
      radius    its brush radius in pixels: 0.5 to 1.6 times `brush_size` (limits 0.5 to 200)
      color     `color` moved a little by `color_jitter`
      settings  the brush's settings varied by `variation` (see brushes.random_settings)
    """
    marks = []
    for _ in range(count):
        # A random direction from the middle and a distance that is usually short (a bell
        # curve, capped at the edge), so the marks bunch toward the middle.
        reach = min(1.0, abs(rng.gauss(0, 0.5)))
        theta = rng.uniform(0, math.tau)
        marks.append({
            "u": reach * math.cos(theta), "v": reach * math.sin(theta),
            "length": rng.uniform(0.08, 0.35), "heading": rng.uniform(0, math.tau), "bend": rng.uniform(-0.3, 0.3),
            "pressure": [round(rng.uniform(0.3, 0.8), 2), round(rng.uniform(0.7, 1.0), 2), round(rng.uniform(0.2, 0.7), 2)],
            "radius": round(min(200.0, max(0.5, brush_size * rng.uniform(0.5, 1.6))), 1),
            "color": jitter_color(rng, color, color_jitter),
            "settings": brushes.random_settings(rng, brush, variation, tuning),
        })
    return marks


def cluster_args(rng, colors, weights, brush_list, short, tuning=None):
    """Random values for one Cluster (see cluster.py), including its marks.

    Returns a dict: brush, color, size, rotation, aspect, tool, brush_size, marks. The position
    is not here: place it yourself (Cluster.reach(size, brush_size) says how far it reaches).
    """
    brush = brushes.random_brush(rng, brush_list)
    color = pick_color(rng, colors, weights)
    brush_size = pick_brush_size(rng, short)
    variation = round(rng.uniform(0.1, 0.5), 2)  # how far each mark's brush settings move
    color_jitter = round(rng.uniform(0.03, 0.15), 2)  # how far each mark's color moves
    return {
        "brush": brush,
        "color": color,
        "size": pick_size(rng, short, 0.1, 0.3),  # 10% to 30% of the shorter canvas side
        "rotation": round(rng.uniform(0, 360)),
        "aspect": round(rng.uniform(0.4, 1.0), 2),  # how narrow the oval area is
        "tool": rng.choice(["oils", "oils", "ink", "pencil", "marker"]),  # oils twice: it's the likelier pick
        "brush_size": brush_size,
        "marks": cluster_marks(rng, brush, color, rng.randint(6, 24), brush_size, variation, color_jitter, tuning),
    }


# ---------- paint styles ----------

def paint_settings(rng, tuning=None):
    """Random paint settings for a paint-style mark (the `paint` input of every style in
    paint_styles.py): each drawn from its range in PAINT_RANGES, and `loading` and
    `thickness` from the whole range 0 to 1 (loading leans the way the bias in `tuning` says; see brushes.py)."""
    settings = {name: _draw(rng, low, high) for name, (low, high) in PAINT_RANGES.items()}
    settings["loading"] = brushes.lean(rng, "loading", *FULL_RANGE, tuning)
    settings["thickness"] = round(rng.uniform(*FULL_RANGE), 2)
    return settings


def style_options(rng, name):
    """Random structure for the paint style `name`: its options (hairs, splatter, heading, ...),
    drawn from STYLE_RANGES. These go to the style as keyword arguments."""
    options = {option: _draw(rng, low, high) for option, (low, high) in STYLE_RANGES[name].items()}
    if name == "slash" and rng.random() < 0.5:
        options["heading"] = -options["heading"]  # the sweep can lean either way
    return options


def style_args(rng, colors, weights, brush_list, short, name=None, tuning=None):
    """Random values for one paint-style mark (see paint_styles.py).

    Returns a dict: name (a style from STYLE_RANGES, unless you pass one), brush, color, size,
    rotation, brush_size, paint, seed, and `options` (the style's own structure: send them as
    keyword arguments, `STYLES[name](..., **args["options"])`). The position is not here: place
    it yourself (paint_styles.reach(size) says how far it reaches).
    """
    name = name or rng.choice(list(STYLE_RANGES))
    size = pick_size(rng, short, 0.18, 0.45)  # 18% to 45% of the shorter canvas side
    return {
        "name": name,
        "brush": brushes.random_brush(rng, brush_list),
        "color": pick_color(rng, colors, weights),
        "size": size,
        "rotation": round(rng.uniform(0, 360)),
        "brush_size": round(min(200.0, max(0.5, size * rng.uniform(*BRUSH_SHARE))), 1),
        "paint": paint_settings(rng, tuning),
        "seed": rng.randrange(2 ** 31),  # only for the fine texture inside the mark
        "options": style_options(rng, name),
    }


# ---------- trees ----------

# How much a random tree varies. Each is a (lowest, highest) range; narrow one to make the trees
# alike in that way, widen it for more variety. (Shares are of the tree's height unless said.)
TREE_RANGES = {
    "height": (0.45, 0.85),        # of the shorter canvas side
    "crown_width": (0.8, 1.6),     # of the height
    "crown_height": (0.4, 0.75),   # of the height: a squat crown to a tall one
    "trunk_share": (0.35, 0.7),    # of the height: a short trunk to a tall bare one
    "trunk_width": (0.05, 0.14),   # of the height
    "lean": (-22, 22),             # degrees
    "bow": (-0.15, 0.15),          # trunk bend, of its length
    "crown_shift": (-0.25, 0.3),   # of the crown width
    "lobes": (1, 4),               # how many clumps the crown is made of
    "dabs": (250, 750),            # how many dabs: a sparse crown to a dense one
    "dab_length": (0.04, 0.12),    # of the height
    "dab_width": (0.014, 0.04),    # of the height
    "branches": (3, 8),
    "holes": (1, 5),               # gaps where the sky shows
    "smears": (3, 12),             # long dry strokes dragged out by the wind
    "leaf_colors": (2, 4),         # how many palette colors the crown uses
}


def tree_args(rng, colors, weights, brush_list, short, tuning=None):
    """Random values for one Tree (see tree.py), taken from a palette. How much they vary is TREE_RANGES.

    Returns a dict: brush, height, colors, trunk_width, lean, bow, crown_width, crown_height,
    trunk_share, crown_shift, branches, dabs, paint, seed. The position is not here: place it
    yourself (Tree.reach(height, crown_width, lean, crown_shift, trunk_share) says how far it reaches).

    The palette is turned into the tree's colors: the DARKEST palette color paints the shadow side of
    the trunk and the branches; the lit side of the trunk is one of the lighter half; the crown is
    built from a few palette colors (picked by `weights`, each shifted a little per dab) plus the
    dark one, which is likelier toward the bottom of the crown (the underside is in shadow). The
    dabs come back ordered dark to light, so the lights land on top.

    The shape varies too: the crown is built from 1 to 4 overlapping clumps (lobes) of different
    sizes, so it can be a single ball, a lopsided mass or a few separate clouds; the wind (which side
    the smears and the dabs lean to) is random; and the crown, trunk, lean and density all differ.
    """
    r = TREE_RANGES
    ordered = sorted(colors, key=luminance)
    dark = ordered[0]
    light = rng.choice(ordered[len(ordered) // 2:])
    leaf_colors = [pick_color(rng, colors, weights) for _ in range(_draw(rng, *r["leaf_colors"]))]
    height = pick_size(rng, short, *r["height"])
    crown_width = round(height * rng.uniform(*r["crown_width"]))
    crown_height = _draw(rng, *r["crown_height"])
    trunk_share = _draw(rng, *r["trunk_share"])
    # The trunk must reach up into the crown, or the crown would float above it: its top is at least
    # a tenth of the height above the crown's bottom (no random numbers used).
    trunk_share = round(min(0.9, max(trunk_share, 1 - crown_height + 0.1)), 2)
    lean = round(rng.uniform(*r["lean"]), 1)
    crown_shift = _draw(rng, *r["crown_shift"])
    wind = rng.choice([-1, 1])  # which way the wind blows: the smears and the dabs lean that way
    # Keep the tree small enough to fit the canvas (its reach no more than half the shorter side):
    # narrow the crown first, then shrink the whole tree. No random numbers are used for this.
    while tree.Tree.reach(height, crown_width, lean, crown_shift, trunk_share) > short / 2:
        if crown_width > height * 0.7:
            crown_width = round(crown_width * 0.95)
        else:
            height = round(height * 0.95)
            crown_width = round(crown_width * 0.95)
    trunk_width = round(height * rng.uniform(*r["trunk_width"]))
    brush = brushes.random_brush(rng, brush_list)

    branches = [{"t": round(rng.uniform(0.45, 1.0), 2), "angle": round(rng.uniform(-85, 85)),
                 "length": round(rng.uniform(0.12, 0.35), 2), "bend": round(rng.uniform(-0.25, 0.25), 2),
                 "width": round(trunk_width * rng.uniform(0.12, 0.3), 1)} for _ in range(_draw(rng, *r["branches"]))]

    # The crown's clumps: each (u, v, radius across, radius down, weight) inside the crown's oval. One
    # clump fills most of it; several are smaller and spread out. A bigger clump gets more dabs.
    lobe_count = _draw(rng, *r["lobes"])
    lobes = []
    for _ in range(lobe_count):
        size = rng.uniform(0.9, 1.0) if lobe_count == 1 else rng.uniform(0.35, 0.7)
        cu, cv = (0.0, 0.0) if lobe_count == 1 else (rng.uniform(-0.55, 0.55), rng.uniform(-0.45, 0.35))
        lobes.append((cu, cv, size, size * rng.uniform(0.7, 1.1), size * size))
    # Gaps where the sky shows through the crown: a few round holes (u, v, radius), left unpainted.
    holes = [(rng.uniform(-0.7, 0.7), rng.uniform(-0.7, 0.5), rng.uniform(0.06, 0.14)) for _ in range(_draw(rng, *r["holes"]))]
    dabs = []
    for _ in range(_draw(rng, *r["dabs"])):
        cu, cv, ru, rv, _w = rng.choices(lobes, [lobe[4] for lobe in lobes])[0]
        # A place inside the clump's oval, spread evenly over its area (the square root does that).
        reach, theta = math.sqrt(rng.random()), rng.uniform(0, math.tau)
        u, v = cu + ru * reach * math.cos(theta), cv + rv * reach * math.sin(theta)
        if math.hypot(u, v) > 1.0 or any(math.hypot(u - hu, v - hv) < hr for hu, hv, hr in holes):
            continue
        # The lower in the crown, the likelier a dab is the dark color.
        is_dark = rng.random() < 0.12 + 0.35 * (v + 1) / 2
        color = dark if is_dark else rng.choice(leaf_colors)
        # Dabs lean a little the way the wind blows (heading 0 is right, negative is up).
        heading = rng.uniform(-0.7, 0.3) if wind > 0 else math.pi - rng.uniform(-0.7, 0.3)
        dabs.append({"u": round(u, 2), "v": round(v, 2), "length": round(rng.uniform(*r["dab_length"]), 3),
                     "width": round(height * rng.uniform(*r["dab_width"]), 1), "heading": round(heading, 2),
                     "color": jitter_color(rng, color, 0.05), "dry": round(rng.uniform(0.1, 0.4), 2),
                     "roundness": round(rng.uniform(0.35, 0.6), 2)})
    # A few long, very dry smears dragged outward by the wind, from the crown's downwind edge.
    for _ in range(_draw(rng, *r["smears"])):
        theta = rng.uniform(-math.pi / 2, math.pi / 2)
        u, v = 0.9 * math.cos(theta) * wind, 0.9 * math.sin(theta)
        heading = rng.uniform(-0.3, 0.5) if wind > 0 else math.pi - rng.uniform(-0.3, 0.5)
        dabs.append({"u": round(u, 2), "v": round(v, 2),
                     "length": round(rng.uniform(0.12, 0.3), 3), "width": round(height * rng.uniform(0.02, 0.04), 1),
                     "heading": round(heading, 2),
                     "color": jitter_color(rng, rng.choice([dark] + leaf_colors), 0.05), "dry": round(rng.uniform(0.7, 0.95), 2),
                     "roundness": round(rng.uniform(0.2, 0.45), 2)})
    dabs.sort(key=lambda d: luminance(d["color"]))

    return {"brush": brush, "height": height, "colors": {"light": light, "dark": dark},
            "trunk_width": trunk_width, "lean": lean, "bow": round(rng.uniform(*r["bow"]), 2),
            "crown_width": crown_width, "crown_height": crown_height, "trunk_share": trunk_share,
            "crown_shift": crown_shift, "branches": branches, "dabs": dabs,
            "paint": paint_settings(rng, tuning), "seed": rng.randrange(2 ** 31)}


def recording_colors(rng, recording, colors):
    """The `colors` to send a converted recording (a module in recorded/, see recordings.py): the
    colors picked while recording stay; each color it painted without picking one (its UNPICKED,
    the starting black) becomes a random color from `colors`. {} when every color was picked."""
    return {hex_color: rng.choice(colors) for hex_color in recording.UNPICKED}

