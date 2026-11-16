"""Turn a .jsonl script into a Python function a conductor can call, like an object.

Paint something in the app with ⏺ Record (or take any .jsonl a conductor wrote), then convert it:

    python3 scripts/abstract/recordings.py ~/Documents/swirl.jsonl

That writes scripts/abstract/recorded/swirl.py, with a function named after the file:

    from recorded import swirl
    cmds += swirl.swirl(x=600, y=450, scale=0.5, rotation=30, colors={"#C2151B": "#2021A0"})

The function returns the recorded commands, moved, scaled, turned and recolored by the values
it is sent. Nothing in it is random, so a conductor can send exact values, or random ones from
randomizers.py. Like an object, it has a `reach(scale)` for placing it.

What the values change:
  x, y       where the middle of the strokes goes (default: where it was painted)
  scale      size; 1 = as painted. Brush sizes and hatch spacing scale too, and paint thickness
             (impasto) by the scale to the 4th power when smaller, so a smaller copy looks like the original
             shrunk (not thicker and heavier).
  rotation   degrees, clockwise, around the middle. The strokes' paths turn; a brush with a fixed
             tip angle keeps its angle (brushes that follow the stroke turn with it anyway).
             Rectangle and ellipse selections become lasso outlines when turned.
  colors     {recorded hex: new hex}: repaint it in other colors (the module's COLORS lists the
             recorded ones). Colors not in the dict stay.
  palette    a list of hex colors (a palette): every recorded color is repainted with the palette
             color of the same lightness, darkest to darkest (palette_swap). `colors` wins over it.
  shade      one hex color: every recorded color is repainted in shades of it, keeping its own
             lightness (shade_swap): a one-color version. It wins over `palette`; `colors` wins over it.
  brush      a brush dict from brushes.list_brushes() (or a brush's name): paint every stroke with
             it, at the recorded sizes and paint settings. Leave it out to use the brushes picked
             while recording (the module lists them in BRUSHES), exactly as they were set.

Colors you picked while recording are the defaults: the module lists them in PICKED. If you
painted without picking a color (in the black oxide-run starts with), UNPICKED says so, and the
conductor fills it from its palette, keeping the picked ones:

    swap = randomizers.recording_colors(rng, swirl, palette_colors)   # {} if every color was picked
    cmds += swirl.swirl(x, y, colors=swap)

Left out when converting, because they belong to the conductor: new_painting, open, set_paper,
export, save, snapshot, the read-only commands (info, describe, list_*), and the layer commands
before the first stroke (they set up the recording's own canvas). Later layer commands are kept
as recorded (select_layer uses the recording's layer numbers).

Added when converting: a recording only writes what changed from how oxide-run starts (oils,
black, each tool's default brush), so the function starts by setting those (START_BRUSHES), and
resets each other tool's brush the first time it switches to it. It paints the same whatever
the conductor painted before it. Black is in COLORS, so `colors={"#000000": ...}` recolors it.

Command line:
    python3 scripts/abstract/recordings.py FILE.jsonl [--name NAME] [--out FOLDER]
A converted file paints a demo of itself: python3 scripts/abstract/recorded/swirl.py --run
"""

import argparse
import colorsys
import keyword
import math
import re
import sys
from datetime import date
from pathlib import Path

import commands
from palettes import luminance

SCRIPT_DIR = Path(__file__).resolve().parent
RECORDED_DIR = SCRIPT_DIR / "recorded"

# Commands a converted function leaves out: they are about the whole painting or about files.
CANVAS_COMMANDS = {"new_painting", "open", "set_paper", "export", "save", "snapshot",
                   "info", "describe", "list_pigments", "list_brushes"}

# Layer commands before the first stroke set up the recording's own canvas (naming its first
# layer, adding the layers it started with); they're left out too.
LAYER_SETUP = {"set_layer", "add_layer", "select_layer"}


def _brush(size, hardness, spacing, grain, opacity, loading, oiliness, size_pressure, opacity_pressure,
           thickness=0.0, **shape):
    """Every set_brush setting of a tool as a fresh oxide-run session has it (see tools.rs, default_state)."""
    return {"size": size, "hardness": hardness, "spacing": spacing, "grain": grain, "opacity": opacity,
            "loading": loading, "oiliness": oiliness, "thickness": thickness,
            "size_pressure": size_pressure, "opacity_pressure": opacity_pressure,
            "roundness": 1.0, "angle": 0.0, "rotation": "none", "roughness": 0.0, "streaks": 0.0, "lift_off": 0.0,
            "taper": 0.0, "taper_ends": "both", "rough_ends": 0.0, "tip": "round", "jitter_angle": 0.0, "jitter_size": 0.0,
            "scatter": 0.0, "bristles": 0, **shape}


# A recording only writes what changed from how oxide-run starts: oils, black paint, and each
# tool's own default brush. A converted function sets those first (for every tool it uses), so it
# paints the same whatever the conductor painted before it.
START_BRUSHES = {
    "oils": _brush(14, 0.6, 0.04, 0.6, 1.0, 0.85, 0.4, 0.6, 0.3, thickness=0.5,
                   roughness=0.35, streaks=0.45, taper=0.2, rough_ends=0.6),
    "ink": _brush(3, 0.95, 0.05, 0.2, 1.0, 1.0, 0.0, 1.0, 0.0),
    "pencil": _brush(1.5, 0.5, 0.1, 0.9, 0.6, 1.0, 0.0, 0.3, 0.8),
    "marker": _brush(12, 0.85, 0.05, 0.3, 0.35, 1.0, 0.0, 0.2, 0.4),
    "airbrush": _brush(40, 0.0, 0.05, 0.1, 0.06, 1.0, 0.0, 0.0, 1.0),
    "eraser": _brush(16, 0.8, 0.08, 0.0, 1.0, 1.0, 0.0, 0.6, 0.3),
    "smudge": _brush(14, 0.5, 0.05, 0.0, 1.0, 0.0, 0.8, 0.4, 0.6),
    "knife": _brush(22, 1.0, 0.03, 0.5, 1.0, 0.9, 0.7, 0.2, 0.0, thickness=0.9,
                    roundness=0.15, rotation="follow_stroke"),
    "hatch": _brush(2, 1.0, 0.05, 0.2, 1.0, 1.0, 0.0, 1.0, 0.0, taper=0.6),
}
START_MODES = {"oils": "paint_mix", "knife": "spread"}  # the tools that have paint modes
START_COLOR = "#000000"


def start_of(tool):
    """The commands that put `tool` back to how oxide-run starts it."""
    cmds = [{"command": "set_tool", "tool": tool}, {"command": "set_brush", **START_BRUSHES[tool]}]
    if tool in START_MODES:
        cmds.append({"command": "set_paint_mode", "mode": START_MODES[tool]})
    return cmds


def with_start(cmds):
    """`cmds` (a recording) made to paint the same after anything: it starts with oils, black and
    no MultiColored brush, and each tool's first set_tool also resets that tool's brush."""
    out = [*start_of("oils"), {"command": "set_multicolor", "colors": []}, {"command": "set_color", "hex": START_COLOR}]
    seen = {"oils"}
    for c in cmds:
        if isinstance(c, dict) and c["command"] == "set_tool" and c["tool"] not in seen and c["tool"] in START_BRUSHES:
            seen.add(c["tool"])
            out += start_of(c["tool"])
        else:
            out.append(c)
    return out


# ---------- what a converted function calls ----------

def transform(cmds, origin, x=None, y=None, scale=1.0, rotation=0, colors=None, brush=None):
    """A copy of `cmds` moved so `origin` lands on (x, y), scaled by `scale` and turned
    `rotation` degrees clockwise around it, with its colors swapped by `colors`
    ({old hex: new hex}) and, with `brush`, painted with that brush (see with_brush).
    "# comment" strings are kept as they are."""
    if brush is not None:
        cmds = with_brush(cmds, brush)
    ox, oy = origin
    tx, ty = (ox if x is None else x), (oy if y is None else y)
    cos, sin = math.cos(math.radians(rotation)), math.sin(math.radians(rotation))
    turned = rotation % 360 != 0
    swap = {k.upper(): v for k, v in (colors or {}).items()}

    def at(px, py):
        dx, dy = (px - ox) * scale, (py - oy) * scale
        return round(tx + dx * cos - dy * sin, 2), round(ty + dx * sin + dy * cos, 2)

    def point(p):
        return [*at(p[0], p[1]), *p[2:]]

    def color(hex_or_name):
        return swap.get(hex_or_name.upper(), hex_or_name) if isinstance(hex_or_name, str) else hex_or_name

    out = []
    for c in cmds:
        if isinstance(c, str):
            out.append(c)
            continue
        c = dict(c)
        name = c["command"]
        if name == "stroke" and c.get("target") != "palette":
            c["points"] = [point(p) for p in c["points"]]
        elif name == "hatch":
            for key in ("rail_a", "rail_b", "edge"):
                if c.get(key):
                    c[key] = [point(p) for p in c[key]]
            if "spacing" in c:
                c["spacing"] = round(c["spacing"] * scale, 3)
        elif name == "select":
            c = _select(c, at, turned)
        elif name == "pick_color" and c.get("from", "painting") == "painting":
            c["x"], c["y"] = at(c["x"], c["y"])
        elif name == "transform_layer":
            # A move is a direction and a distance: it scales and turns, but isn't moved.
            mx, my = c.get("move_x", 0) * scale, c.get("move_y", 0) * scale
            c["move_x"], c["move_y"] = round(mx * cos - my * sin, 2), round(mx * sin + my * cos, 2)
        elif name == "set_brush":
            if "size" in c:
                c["size"] = round(min(max(c["size"] * scale, 0.5), 200), 3)
            # Thick paint (impasto) is built in fixed height units and lit by its slope from pixel to
            # pixel. Shrunk, the same height over fewer pixels makes steeper, shinier ridges; where
            # strokes pile up the paint reaches the engine's height limit; and the original, seen
            # shrunk, averages its fine ridges away. So a smaller copy looks dark, shiny and heavily
            # loaded unless its paint is much thinner: by the scale to the 4th power, matched by eye
            # against recordings seen shrunk (plant1 at 0.11, falltree1 at 0.39). A bigger copy: by the scale.
            if "thickness" in c:
                c["thickness"] = round(min(c["thickness"] * (scale if scale >= 1 else scale ** 4), 1.0), 5)
        elif name in ("set_color", "palette_add_swatch") and "hex" in c:
            c["hex"] = color(c["hex"])
        elif name == "set_multicolor":
            c["colors"] = [color(h) for h in c["colors"]]
        out.append(c)
    return out


def _select(c, at, turned):
    """A select command moved by `at`. Turned, a rectangle or ellipse becomes a lasso outline."""
    shape = c.get("shape", "rect")
    if shape in ("lasso", "polygon"):
        return {**c, "points": [list(at(*p)) for p in c["points"]]}
    x0, y0, x1, y1 = c["x0"], c["y0"], c["x1"], c["y1"]
    if not turned:
        (ax, ay), (bx, by) = at(x0, y0), at(x1, y1)
        return {**c, "x0": min(ax, bx), "y0": min(ay, by), "x1": max(ax, bx), "y1": max(ay, by)}
    if shape == "ellipse":
        cx, cy, rx, ry = (x0 + x1) / 2, (y0 + y1) / 2, abs(x1 - x0) / 2, abs(y1 - y0) / 2
        outline = [(cx + rx * math.cos(t), cy + ry * math.sin(t)) for t in (i / 48 * math.tau for i in range(48))]
    else:
        outline = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
    keep = {k: v for k, v in c.items() if k not in ("x0", "y0", "x1", "y1", "shape")}
    return {**keep, "shape": "lasso", "points": [list(at(*p)) for p in outline]}


# The set_brush settings that are about the paint, not the brush's shape: they stay as recorded
# when a conductor sends another brush (a preset sets the shape only).
PAINT_SETTINGS = ("opacity", "loading", "oiliness", "thickness")


def with_brush(cmds, brush):
    """`cmds` painted with `brush` (a brush dict from brushes.list_brushes(), or a brush's name)
    instead of the brushes it was recorded with.

    Every set_brush becomes `preset: <brush>` plus the recorded size and paint settings
    (opacity, loading, oiliness, thickness); the recorded shape settings (hardness,
    roundness, bristles, ...) are dropped, as they'd undo the brush. A tool's starting brush
    (START_BRUSHES) is kept in front of it, so settings the preset doesn't set (grain, pressure)
    start from oxide-run's defaults, not from whatever the conductor painted last."""
    name = brush["name"] if isinstance(brush, dict) else brush
    sizes = {tool: b["size"] for tool, b in START_BRUSHES.items()}
    tool, out = "oils", []
    for c in cmds:
        if isinstance(c, dict) and c["command"] == "set_tool":
            tool = c["tool"]
        if not (isinstance(c, dict) and c["command"] == "set_brush"):
            # A comment naming the recorded brush would now be wrong.
            out.append(f"# brush: {name}" if isinstance(c, str) and c.startswith("# brush:") else c)
            continue
        sizes[tool] = c.get("size", sizes.get(tool, 10))
        if c == {"command": "set_brush", **START_BRUSHES.get(tool, {})}:
            out.append(c)
        out.append({"command": "set_brush", "preset": name, "size": sizes[tool],
                    **{k: c[k] for k in PAINT_SETTINGS if k in c}})
    return out


def picked_brushes(cmds):
    """The brushes picked while recording, by name, in order: the app writes a "# brush: <name>"
    comment each time you pick one (the set_brush after it holds its exact settings)."""
    names = []
    for c in cmds:
        if isinstance(c, str) and c.startswith("# brush:"):
            name = c[len("# brush:"):].strip()
            if name not in names:
                names.append(name)
    return names


# The tools whose strokes put the current color down (eraser and smudge don't).
COLOR_TOOLS = {"oils", "ink", "pencil", "marker", "airbrush", "knife", "hatch"}


def color_use(cmds):
    """Which colors a recording picked, and which it painted with without picking them.

    Returns (picked, unpicked): `picked` is every color the recording chose (set_color,
    set_multicolor); `unpicked` is [START_COLOR] if some stroke was painted in the color
    oxide-run starts with before any was picked, else []. A set_color to the color already in
    use isn't a pick (the recorder writes one to clean a dirty brush), so painting in the
    starting black and never choosing a color counts as unpicked."""
    current, chosen, multicolor, tool = START_COLOR.upper(), False, False, "oils"
    picked, unpicked = [], []

    def pick(h):
        if h.upper() not in (x.upper() for x in picked):
            picked.append(h)

    for c in cmds:
        if isinstance(c, str):
            continue
        name = c["command"]
        if name == "set_tool":
            tool = c["tool"]
        elif name == "set_color":
            if "hex" not in c or c["hex"].upper() != current:
                current, chosen = c.get("hex", "").upper(), True
                if "hex" in c:
                    pick(c["hex"])
        elif name in ("pick_color", "palette_pick"):
            current, chosen = "", True  # a color taken from the canvas or palette: chosen, but not known here
        elif name == "set_multicolor":
            multicolor = bool(c["colors"])
            for h in c["colors"]:
                pick(h)
        elif name in ("stroke", "hatch") and tool in COLOR_TOOLS and not chosen and not multicolor:
            unpicked = [START_COLOR]
    return picked, unpicked


def palette_swap(recorded, palette):
    """The `colors` dict that repaints the `recorded` colors with `palette` (a list of hex colors),
    matched by lightness: both are sorted dark to light, and each recorded color gets the palette
    color at the same place along that line (the darkest gets the darkest, the lightest the
    lightest). Several recorded colors can share one palette color when the palette is shorter.
    Nothing random: the same colors and palette always give the same dict."""
    if not palette or not recorded:
        return {}
    shades = sorted(palette, key=luminance)
    ranked = sorted(recorded, key=luminance)
    if len(ranked) == 1:
        return {ranked[0]: shades[len(shades) // 2]}
    return {color: shades[round(i * (len(shades) - 1) / (len(ranked) - 1))] for i, color in enumerate(ranked)}


def shade_swap(recorded, shade):
    """The `colors` dict that repaints the `recorded` colors in shades of one color, `shade` (a hex
    color): each keeps its own lightness and takes the hue and saturation of `shade`, so dark
    stays dark and light stays light (black and white stay black and white). {} without a shade.
    Nothing random: the same colors and shade always give the same dict."""
    if not shade:
        return {}
    hue, _, saturation = colorsys.rgb_to_hls(*_rgb(shade))
    out = {}
    for color in recorded:
        _, lightness, _ = colorsys.rgb_to_hls(*_rgb(color))
        r, g, b = colorsys.hls_to_rgb(hue, lightness, saturation)
        out[color] = "#{:02X}{:02X}{:02X}".format(*(round(v * 255) for v in (r, g, b)))
    return out


def color_swap(recorded, colors=None, palette=None, shade=None):
    """The colors a converted recording paints with: `palette` (palette_swap), then `shade`
    (shade_swap) over it, then `colors` (exact swaps) over both. {} = as recorded."""
    return {**palette_swap(recorded, palette), **shade_swap(recorded, shade), **(colors or {})}


def _rgb(color):
    """A hex color as three numbers from 0 to 1."""
    h = color.lstrip("#")
    return tuple(int(h[i:i + 2], 16) / 255 for i in (0, 2, 4))


# ---------- measuring a recording ----------

def positions(cmds):
    """Every canvas position the commands paint or select at (stroke points, hatch rails, selections)."""
    found = []
    for c in cmds:
        if isinstance(c, str):
            continue
        name = c["command"]
        if name == "stroke" and c.get("target") != "palette":
            found += [(p[0], p[1]) for p in c["points"]]
        elif name == "hatch":
            for key in ("rail_a", "rail_b", "edge"):
                found += [(p[0], p[1]) for p in c.get(key) or []]
        elif name == "select":
            if c.get("points"):
                found += [tuple(p) for p in c["points"]]
            elif "x0" in c:
                found += [(c["x0"], c["y0"]), (c["x1"], c["y1"])]
    return found


def bounds(cmds):
    """The box around everything the commands paint: (x0, y0, x1, y1), or None if they paint nothing."""
    found = positions(cmds)
    if not found:
        return None
    xs, ys = [p[0] for p in found], [p[1] for p in found]
    return min(xs), min(ys), max(xs), max(ys)


def reach(cmds, origin):
    """How far the paint reaches from `origin`, in pixels: for each stroke, its farthest point
    plus the brush radius it is painted with (each tool keeps its own size, starting at
    oxide-run's default)."""
    ox, oy = origin
    sizes = {tool: brush["size"] for tool, brush in START_BRUSHES.items()}
    tool, far = "oils", 0.0
    for c in cmds:
        if isinstance(c, str):
            continue
        if c["command"] == "set_tool":
            tool = c["tool"]
        elif c["command"] == "set_brush" and "size" in c:
            sizes[tool] = c["size"]
        elif c["command"] in ("stroke", "hatch", "select"):
            here = max((math.hypot(px - ox, py - oy) for px, py in positions([c])), default=0)
            far = max(far, here + (sizes.get(tool, 0) if c["command"] != "select" else 0))
    return far


# ---------- converting ----------

def function_name(path):
    """A Python name from a file name: "My Swirl 2.jsonl" -> "my_swirl_2"."""
    name = re.sub(r"\W+", "_", Path(path).stem.lower()).strip("_") or "recording"
    if name[0].isdigit() or keyword.iskeyword(name):
        name = "recording_" + name
    return name


def to_python(cmds, name, source="a recording", out=RECORDED_DIR):
    """The text of a Python module with one function, `name`, that paints `cmds`."""
    canvas = next((c for c in cmds if isinstance(c, dict) and c["command"] == "new_painting"), None)
    dropped = sorted({c["command"] for c in cmds if isinstance(c, dict) and c["command"] in CANVAS_COMMANDS})
    kept = [c for c in cmds if isinstance(c, str) or c["command"] not in CANVAS_COMMANDS]
    first = next((i for i, c in enumerate(kept) if isinstance(c, dict) and c["command"] in ("stroke", "hatch")), len(kept))
    setup = [c for c in kept[:first] if isinstance(c, dict) and c["command"] in LAYER_SETUP]
    dropped = sorted(set(dropped) | ({"layer setup before the first stroke"} if setup else set()))
    kept = [c for c in kept if not any(c is d for d in setup)]
    picked, unpicked = color_use(kept)
    brush_names = picked_brushes(kept)
    kept = with_start(kept)
    # The comments oxide-run's own header puts at the top of a recording aren't useful here.
    kept = [c for c in kept if not (isinstance(c, str) and ("Paint it again with" in c or "oxide-run -- <this file>" in c))]
    box = bounds(kept)
    origin = (round((box[0] + box[2]) / 2, 2), round((box[1] + box[3]) / 2, 2)) if box else (0.0, 0.0)
    size = (canvas["width"], canvas["height"]) if canvas else (1200, 900)
    colors = []
    for c in kept:
        if isinstance(c, str):
            continue
        used = c.get("colors", []) if c["command"] == "set_multicolor" else [c.get("hex")] if "hex" in c else []
        colors += [h for h in used if h.upper() not in (x.upper() for x in colors)]

    strokes = sum(1 for c in kept if isinstance(c, dict) and c["command"] in ("stroke", "hatch"))
    where = (f"Its strokes cover x {box[0]:.0f}–{box[2]:.0f}, y {box[1]:.0f}–{box[3]:.0f}; the middle is "
             f"({origin[0]:.0f}, {origin[1]:.0f}) and it reaches {reach(kept, origin):.0f} px from there (at scale 1)."
             if box else "It paints nothing on the canvas.")
    body = "\n".join(f"        {c!r}," for c in kept)
    # Converted somewhere other than scripts/abstract/recorded: also look where recordings.py is.
    elsewhere = Path(out).expanduser().resolve() != RECORDED_DIR.resolve()
    extra_note = ", or where it was converted" if elsewhere else ""
    extra_path = f", {str(SCRIPT_DIR)!r}" if elsewhere else ""
    return f'''"""{name}: converted from {source} by recordings.py on {date.today()}.

    from recorded import {name}
    cmds += {name}.{name}(x=600, y=450, scale=0.5, rotation=30)
    cmds += {name}.{name}(x=600, y=450, palette=["#1C2433", "#4F7329", "#E2C43F"])   # in a palette's colors
    cmds += {name}.{name}(x=600, y=450, brush="Round Hard")                             # with another brush
    cmds += {name}.{name}(x=600, y=450, shade="#2E6B8F")                               # in shades of one color

{strokes} strokes, recorded on a {size[0]} × {size[1]} canvas. {where}
Left out (they belong to the conductor): {", ".join(dropped) or "nothing"}.
What x, y, scale, rotation and colors do: see recordings.py.

Paint a demo of it: python3 scripts/abstract/recorded/{name}.py --run
"""

import sys
from pathlib import Path

# recordings.py is in scripts/abstract: the folder above this one{extra_note}.
sys.path[:0] = [str(Path(__file__).resolve().parent.parent){extra_path}]
import recordings  # noqa: E402

ORIGIN = {origin!r}  # the middle of the strokes, where they were painted
REACH = {round(reach(kept, origin), 1)!r}  # how far the paint reaches from ORIGIN, at scale 1
SIZE = {size!r}  # the canvas it was recorded on
COLORS = {colors!r}  # the colors it uses (keys for `colors`)
PICKED = {picked!r}  # the colors picked while recording: painted as they are unless `colors` swaps them
BRUSHES = {brush_names!r}  # the brushes picked while recording: painted with them unless `brush` is sent
UNPICKED = {unpicked!r}  # painted in the starting color without picking one: a conductor fills these from its palette


def {name}(x=None, y=None, scale=1.0, rotation=0, colors=None, palette=None, shade=None, brush=None):
    """The commands that paint it, with its middle at (x, y) (default: where it was painted),
    `scale` times as big, turned `rotation` degrees clockwise.

    Colors: with neither `colors` nor `palette` it paints the colors as recorded.
    `palette` (a list of hex colors) repaints every recorded color with the palette color at the
    same lightness (darkest to darkest, lightest to lightest; see recordings.palette_swap).
    `shade` (one hex color) repaints every recorded color in shades of it, each keeping its own
    lightness (see recordings.shade_swap); it wins over `palette`.
    `colors` ({{recorded hex: new hex}}) swaps exact colors, and wins over both for those.
    randomizers.recording_colors gives a `colors` that keeps the PICKED ones and fills the
    UNPICKED ones from a palette.

    Brush: with no `brush` it paints with the brushes picked while recording (BRUSHES), exactly
    as they were set. `brush` (a brush dict from brushes.list_brushes(), or a brush's name)
    paints every stroke with that brush instead, at the recorded sizes and paint settings."""
    cmds = [
{body}
    ]
    swap = recordings.color_swap(PICKED + UNPICKED, colors, palette, shade)  # the colors it paints with
    return recordings.transform(cmds, ORIGIN, x=x, y=y, scale=scale, rotation=rotation, colors=swap, brush=brush)


def reach(scale=1.0):
    """How far it reaches from its middle at this scale, in pixels (to place it on the canvas)."""
    return REACH * scale


if __name__ == "__main__":
    recordings.demo({name}, SIZE, __file__)
'''


def convert(jsonl_path, name=None, out=RECORDED_DIR):
    """Convert the .jsonl at `jsonl_path` into `out`/<name>.py and return that path."""
    jsonl_path = Path(jsonl_path).expanduser()
    name = name or function_name(jsonl_path)
    out = Path(out).expanduser()
    out.mkdir(parents=True, exist_ok=True)
    package = out / "__init__.py"
    if not package.exists():
        package.write_text('"""Recordings converted to Python by recordings.py (one module each)."""\n')
    path = out / f"{name}.py"
    path.write_text(to_python(commands.read_script(jsonl_path), name, jsonl_path.name, out))
    return path


def demo(paint, size, module_file):
    """The command line of a converted file: paint it once, as recorded, with --run."""
    ap = argparse.ArgumentParser(description=f"Paint {paint.__name__} on its own.")
    ap.add_argument("--out", default=".", help="folder for the .jsonl and .png files")
    ap.add_argument("--run", action="store_true", help="paint it with oxide-run too")
    args = ap.parse_args()
    out = Path(args.out).expanduser()
    out.mkdir(parents=True, exist_ok=True)
    stem = Path(module_file).stem
    png, script = (out / f"{stem}.png").resolve(), out / f"{stem}-demo.jsonl"
    cmds = [{"command": "new_painting", "width": size[0], "height": size[1]}] + paint() + [{"command": "export", "path": str(png)}]
    commands.write_script(cmds, script)
    print(script)
    if args.run:
        if commands.run_script(script.resolve()):
            print(f"  painted {png}")
        else:
            sys.exit(1)


if __name__ == "__main__":
    ap = argparse.ArgumentParser(description="Convert a .jsonl script into a Python function (see the top of recordings.py).")
    ap.add_argument("jsonl", help="the .jsonl file (recorded in the app, or written by a conductor)")
    ap.add_argument("--name", help="the function and file name (default: from the file name)")
    ap.add_argument("--out", default=str(RECORDED_DIR), help="folder for the .py file (default: scripts/abstract/recorded)")
    args = ap.parse_args()
    print(convert(args.jsonl, args.name, args.out))
