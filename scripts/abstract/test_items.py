"""Tests for the items in scripts/abstract/: the object files (shapes, cluster, paint_styles,
background), the helpers (points, randomizers, brushes) and the conductor (abstract.py).

    python3 scripts/abstract/test_items.py                  # the fast tests: no app, a few seconds
    python3 scripts/abstract/test_items.py --app            # also send real commands through oxide-run
                                                            #   (needs cargo; the first run builds it, so it is slow)
    python3 scripts/abstract/test_items.py -v               # list every test as it runs
    python3 scripts/abstract/test_items.py TestCluster      # one group of tests
    python3 scripts/abstract/test_items.py TestPaintStyles.test_same_seed_same_mark   # one test

The fast tests use a few made-up brushes (STUB_BRUSHES) so they don't need the app. They
check what each item promises: the same inputs give the same output; values sent in arrive
unchanged; everything stays on the canvas; and the rule of the design (see randomizers.py):
the object files do no randomizing except seeded fine texture.

Add a test for a new item as a new class here; add it to the `--app` group if it should be
checked against the real app.
"""

import contextlib
import importlib
import importlib.util
import inspect
import io
import json
import math
import random
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import abstract  # noqa: E402
import abstract2  # noqa: E402
import abstract3  # noqa: E402
import abstract4  # noqa: E402
import background  # noqa: E402
import brushes  # noqa: E402
import cli  # noqa: E402
import cluster  # noqa: E402
import commands  # noqa: E402
import composition  # noqa: E402
import ground  # noqa: E402
import makers  # noqa: E402
import paint_styles  # noqa: E402
import palettes  # noqa: E402
import points  # noqa: E402
import randomizers  # noqa: E402
import recordings  # noqa: E402
import shapes  # noqa: E402
import tree  # noqa: E402

# Whether to also test against the real app (--app). Taken out of argv so unittest doesn't see it.
APP = "--app" in sys.argv
if APP:
    sys.argv.remove("--app")


# ---------- made-up brushes, so most tests don't need the app ----------

def _brush(name, group="Round", bristles=None, **settings):
    """A brush dict shaped like the ones brushes.list_brushes() returns."""
    brush = {"name": name, "group": group, "saved": False, "size": 12.0, "hardness": 0.7, "spacing": 0.05,
             "roundness": 1.0, "angle": 0.0, "rotation": "none", "roughness": 0.0, "streaks": 0.0, "lift_off": 0.0,
             "taper": 0.0, "taper_ends": "both", "tip": "round", "jitter_angle": 0.0, "jitter_size": 0.0,
             "scatter": 0.0, "bristles": bristles}
    brush.update(settings)
    return brush


ROUND = _brush("Round Hard", hardness=0.95)
FLAT = _brush("Bristle Flat", group="Bristle", size=18.0, roundness=0.3, rotation="follow_stroke",
              bristles={"count": 56, "thickness": 0.5, "spread": 0.4, "stiffness": 0.7})
SPONGE = _brush("Sponge", group="Texture", size=26.0, spacing=0.3, tip="sponge", jitter_angle=1.0, jitter_size=0.3, scatter=0.1)
STUB_BRUSHES = [ROUND, FLAT, SPONGE]

PALETTE = ["#C2151B", "#2021A0", "#E2C43F", "#1B1919"]
WEIGHTS = palettes.role_weights(len(PALETTE))


def _commands(items):
    """Only the command dicts (not the '# comment' lines), parsing JSON strings if needed."""
    out = []
    for item in items:
        if isinstance(item, str):
            if item.startswith("#"):
                continue
            item = json.loads(item)
        out.append(item)
    return out


def _strokes(items):
    return [c for c in _commands(items) if c["command"] == "stroke"]


def _body_strokes(items):
    """Strokes that are not splatter flecks (flecks are two-point strokes)."""
    return [c for c in _strokes(items) if len(c["points"]) > 2]


def _all_points(items):
    return [p for c in _strokes(items) for p in c["points"]]


class FakeComposition:
    """The little the placement code needs from composition.Composition."""

    def __init__(self, w=1200, h=900, seed=1):
        self.w, self.h = w, h
        self.comp = composition.Composition(random.Random(seed), w, h)
        self.points, self.rule = self.comp.points, self.comp.rule

    def anchor(self, extent, point=None):
        return self.comp.anchor(extent, point)

    def start_point(self, focus):
        return self.comp.start_point(focus)


# ---------- shapes ----------

class TestShapes(unittest.TestCase):
    def test_same_inputs_same_commands(self):
        for fn in (shapes.circle, shapes.square, shapes.triangle):
            a = fn(FLAT, "#C2151B", 100, 120, 80, fill=True, rotation=30, brush_size=9)
            b = fn(FLAT, "#C2151B", 100, 120, 80, fill=True, rotation=30, brush_size=9)
            self.assertEqual(a, b)

    def test_sets_up_the_brush_then_strokes(self):
        cmds = shapes.circle(FLAT, "#112233", 100, 100, 60, tool="ink", brush_size=7)
        self.assertEqual(cmds[0], {"command": "set_tool", "tool": "ink"})
        self.assertEqual(cmds[1]["command"], "set_brush")
        self.assertEqual((cmds[1]["preset"], cmds[1]["size"]), ("Bristle Flat", 7))
        self.assertEqual(cmds[2], {"command": "set_color", "hex": "#112233"})
        self.assertTrue(all(c["command"] == "stroke" for c in cmds[3:]))

    def test_settings_go_on_top_of_the_brush(self):
        cmds = shapes.square(ROUND, "#000000", 50, 50, 40, settings={"hardness": 0.3, "opacity": 0.5})
        self.assertEqual((cmds[1]["hardness"], cmds[1]["opacity"]), (0.3, 0.5))

    def test_the_brush_size_defaults_to_the_brushs_own(self):
        self.assertEqual(shapes.circle(FLAT, "#000000", 50, 50, 40)[1]["size"], 18.0)

    def test_outline_is_one_closed_stroke(self):
        for fn in (shapes.circle, shapes.square, shapes.triangle):
            strokes = _strokes(fn(ROUND, "#000000", 200, 200, 100))
            self.assertEqual(len(strokes), 1)
            self.assertEqual(strokes[0]["points"][0], strokes[0]["points"][-1])

    def test_fill_adds_rows_under_the_outline(self):
        plain = _strokes(shapes.square(ROUND, "#000000", 200, 200, 100, brush_size=5))
        filled = _strokes(shapes.square(ROUND, "#000000", 200, 200, 100, fill=True, brush_size=5))
        self.assertGreater(len(filled), len(plain) + 5)
        self.assertEqual(filled[-1], plain[0])  # the outline goes last, on top of the fill

    def test_every_point_is_within_reach(self):
        for kind in ("circle", "square", "triangle"):
            for rotation in (0, 17, 45, 90, 200):
                cmds = getattr(shapes, kind)(ROUND, "#000000", 300, 300, 120, fill=True, rotation=rotation, brush_size=8)
                limit = shapes.reach(kind, 120, 8)
                far = max(math.hypot(p[0] - 300, p[1] - 300) for p in _all_points(cmds))
                self.assertLessEqual(far, limit + 0.5, (kind, rotation))

    def test_rotation_turns_a_square(self):
        flat = max(math.hypot(p[0] - 100, p[1] - 100) for p in _all_points(shapes.square(ROUND, "#000000", 100, 100, 80)))
        turned = max(math.hypot(p[0] - 100, p[1] - 100) for p in _all_points(shapes.square(ROUND, "#000000", 100, 100, 80, rotation=45)))
        self.assertAlmostEqual(flat, 40 * math.sqrt(2), delta=0.5)
        self.assertAlmostEqual(turned, 40 * math.sqrt(2), delta=0.5)  # a corner is the farthest point either way
        self.assertNotEqual(shapes.square(ROUND, "#000000", 100, 100, 80), shapes.square(ROUND, "#000000", 100, 100, 80, rotation=45))


# ---------- cluster ----------

def _ring(n=12, radius=9.0):
    """Marks exactly on a circle, pointing along it: no randomness anywhere."""
    return [{"u": math.cos(i * math.tau / n), "v": math.sin(i * math.tau / n), "length": 0.18,
             "heading": i * math.tau / n + math.pi / 2, "bend": 0.0, "pressure": [0.6, 1.0, 0.6],
             "radius": radius, "color": "#1F2A44"} for i in range(n)]


TREE_COLORS = {"light": "#D9B26B", "dark": "#1C2433"}
TREE_BRANCH = {"t": 0.8, "angle": 30, "length": 0.2, "bend": 0.1, "width": 4.0}
TREE_DAB = {"u": 0.2, "v": -0.1, "length": 0.07, "width": 8.0, "heading": -0.3, "color": "#7A8F2B", "dry": 0.2, "roundness": 0.4}


class TestTree(unittest.TestCase):
    def make(self, **over):
        args = dict(brush=FLAT, x=300, y=500, height=400, colors=TREE_COLORS, trunk_width=30, lean=5, bow=0.05,
                    crown_width=350, branches=[TREE_BRANCH], dabs=[TREE_DAB], seed=1)
        args.update(over)
        return tree.Tree(**args)

    def test_same_inputs_same_commands(self):
        self.assertEqual(self.make().commands(), self.make().commands())
        self.assertNotEqual(self.make().commands(), self.make(seed=2).commands())

    def test_it_paints_exactly_what_it_is_given(self):
        strokes = lambda t: sum(1 for c in _commands(t.commands()) if c["command"] == "stroke")
        # The trunk is three strokes (middle, foot, shadow side); then one per branch and one per dab.
        self.assertEqual(strokes(self.make()), 3 + 1 + 1)
        self.assertEqual(strokes(self.make(branches=[], dabs=[])), 3)
        self.assertEqual(strokes(self.make(dabs=[TREE_DAB] * 10)), 3 + 1 + 10)

    def test_the_commands_are_well_formed_and_use_the_colors_sent(self):
        cmds = _commands(self.make().commands())
        colors = [c["hex"] for c in cmds if c["command"] == "set_color"]
        self.assertEqual(set(colors), {"#D9B26B", "#1C2433", "#7A8F2B"})
        for c in cmds:
            if c["command"] == "stroke":
                self.assertTrue(all(len(p) == 3 for p in c["points"]))

    def test_dabs_are_painted_in_the_order_given(self):
        dabs = [{**TREE_DAB, "color": "#111111"}, {**TREE_DAB, "color": "#EEEEEE"}]
        colors = [c["hex"] for c in _commands(self.make(branches=[], dabs=dabs).commands()) if c["command"] == "set_color"]
        self.assertEqual(colors[-2:], ["#111111", "#EEEEEE"])

    def test_the_lean_moves_the_top_of_the_trunk(self):
        top = lambda t: t._trunk_points()[-1][0]
        self.assertGreater(top(self.make(lean=20, bow=0)), 0)
        self.assertLess(top(self.make(lean=-20, bow=0)), 0)
        self.assertAlmostEqual(top(self.make(lean=0, bow=0)), 0)

    def test_the_crown_shift_moves_the_crown(self):
        self.assertGreater(self.make(crown_shift=0.2)._crown_center()[0], self.make()._crown_center()[0])

    def test_reach_covers_the_tree_and_grows_with_it(self):
        self.assertGreater(tree.Tree.reach(500, 400), tree.Tree.reach(300, 400))
        self.assertGreater(tree.Tree.reach(400, 700), tree.Tree.reach(400, 300))
        self.assertGreater(tree.Tree.reach(400, 350, 25), tree.Tree.reach(400, 350, 0))

    def test_random_trees_come_from_the_palette(self):
        for seed in range(15):
            a = randomizers.tree_args(random.Random(seed), PALETTE, WEIGHTS, STUB_BRUSHES, 900)
            darkest = min(PALETTE, key=palettes.luminance)
            self.assertEqual(a["colors"]["dark"], darkest)
            self.assertIn(a["colors"]["light"], PALETTE)
            self.assertGreater(len(a["dabs"]), 100)
            lum = [palettes.luminance(d["color"]) for d in a["dabs"]]
            self.assertEqual(lum, sorted(lum))  # dark to light, so the lights land on top
            self.assertTrue(all(-1 <= d["u"] <= 1 and -1 <= d["v"] <= 1 for d in a["dabs"]))
            self.assertEqual(tree.Tree(x=500, y=700, **a).commands(), tree.Tree(x=500, y=700, **a).commands())

    def test_random_trees_vary_and_the_trunk_reaches_the_crown(self):
        seen = {key: set() for key in ("height", "crown_height", "trunk_share", "lean", "crown_shift")}
        sides = set()
        for seed in range(60):
            a = randomizers.tree_args(random.Random(seed), PALETTE, WEIGHTS, STUB_BRUSHES, 900)
            for key in seen:
                seen[key].add(a[key])
            self.assertGreaterEqual(a["trunk_share"], 1 - a["crown_height"] + 0.1 - 0.011, seed)  # no floating crown
            smears = [d for d in a["dabs"] if d["dry"] >= 0.7]
            sides.add(sum(d["u"] for d in smears) > 0)  # which side the wind blows them to
        for key, values in seen.items():
            self.assertGreater(len(values), 20, key)  # every one differs from tree to tree
        self.assertEqual(sides, {True, False})  # the wind blows both ways

    def test_make_tree_stays_on_the_canvas(self):
        for seed in range(20):
            comp = FakeComposition(1200, 900, seed)
            rng = random.Random(seed)
            cmds = makers.make_tree(rng, comp, PALETTE, WEIGHTS, STUB_BRUSHES)
            self.assertTrue(cmds[0].startswith("# Tree ("))
            for c in _commands(cmds):
                if c["command"] == "stroke":
                    for x, y, _ in c["points"]:
                        # The dabs and smears may stick out a hair past the edge (strokes are wider than their path).
                        self.assertTrue(-20 <= x <= 1220 and -20 <= y <= 920, (seed, x, y))

    def test_a_tuning_reaches_the_trees_paint(self):
        loading = lambda bias: [randomizers.tree_args(random.Random(i), PALETTE, WEIGHTS, STUB_BRUSHES, 900,
                                                      tuning={"bias": {"loading": bias}})["paint"]["loading"] for i in range(100)]
        self.assertLess(sum(loading(4)) / 100, 0.3)
        self.assertGreater(sum(loading(0.25)) / 100, 0.7)


class TestCluster(unittest.TestCase):
    def test_same_inputs_same_commands(self):
        marks = randomizers.cluster_marks(random.Random(1), FLAT, "#C2151B", 10, 10, 0.3, 0.05)
        a = cluster.Cluster(FLAT, "#C2151B", 100, 100, 120, marks).commands()
        b = cluster.Cluster(FLAT, "#C2151B", 100, 100, 120, marks).commands()
        self.assertEqual(a, b)

    def test_one_stroke_per_mark_with_the_marks_pressures(self):
        marks = _ring(7)
        marks[3]["pressure"] = [0.1, 0.2, 0.3]
        strokes = _strokes(cluster.Cluster(ROUND, "#000000", 200, 200, 100, marks).commands())
        self.assertEqual(len(strokes), 7)
        self.assertTrue(all(len(s["points"]) == 3 for s in strokes))
        self.assertEqual([p[2] for p in strokes[3]["points"]], [0.1, 0.2, 0.3])

    def test_exact_marks_land_exactly_where_sent(self):
        # With no bend, the middle point of each mark is its place in the oval: here, on the circle.
        c = cluster.Cluster(ROUND, "#000000", 300, 250, 200, _ring(12))
        for stroke in _strokes(c.commands()):
            middle = stroke["points"][1]
            self.assertAlmostEqual(math.hypot(middle[0] - 300, middle[1] - 250), 100, delta=0.2)

    def test_marks_use_their_own_color_or_the_clusters(self):
        marks = _ring(2)
        del marks[1]["color"]
        colors = [c["hex"] for c in cluster.Cluster(ROUND, "#ABCDEF", 0, 0, 100, marks).commands() if c["command"] == "set_color"]
        self.assertEqual(colors, ["#1F2A44", "#ABCDEF"])

    def test_a_mark_without_settings_is_fine_and_settings_are_passed_on(self):
        marks = _ring(2)
        marks[0]["settings"] = {"hardness": 0.2}
        brush_cmds = [c for c in cluster.Cluster(ROUND, "#000000", 0, 0, 100, marks).commands() if c["command"] == "set_brush"]
        self.assertEqual(brush_cmds[0]["hardness"], 0.2)
        self.assertNotIn("hardness", brush_cmds[1])

    def test_aspect_squashes_the_oval(self):
        c = cluster.Cluster(ROUND, "#000000", 0, 0, 200, _ring(12), aspect=0.5)
        ys = [s["points"][1][1] for s in _strokes(c.commands())]
        self.assertAlmostEqual(max(ys), 50, delta=0.5)

    def test_reach_and_label(self):
        c = cluster.Cluster(FLAT, "#000000", 0, 0, 100, _ring(5), brush_size=10)
        self.assertEqual(c.extent, cluster.Cluster.reach(100, 10))
        self.assertEqual(c.label, "Cluster (5 marks, brush: Bristle Flat)")

    def test_random_marks_stay_within_reach(self):
        for seed in range(30):
            args = randomizers.cluster_args(random.Random(seed), PALETTE, WEIGHTS, STUB_BRUSHES, 900)
            c = cluster.Cluster(x=500, y=500, **args)
            far = max(math.hypot(p[0] - 500, p[1] - 500) for p in _all_points(c.commands()))
            self.assertLessEqual(far, c.extent, seed)


# ---------- paint styles ----------

# How many hairline strokes each style makes for hairs=H, drips=D (the quarter off the slash's near end, the drips of the block).
def _expected_hairlines(name, hairs, drips):
    return {"slash": hairs + max(2, hairs // 4), "fan": hairs, "block": hairs + drips,
            "wedge": hairs, "swoosh": hairs, "flame": hairs}[name]


class TestPaintStyles(unittest.TestCase):
    def test_there_are_six_styles(self):
        self.assertEqual(set(paint_styles.STYLES), {"slash", "fan", "block", "wedge", "swoosh", "flame"})

    def test_same_seed_same_mark(self):
        for name, style in paint_styles.STYLES.items():
            with self.subTest(style=name):
                self.assertEqual(style(FLAT, "#D8403A", 200, 200, 240, seed=5), style(FLAT, "#D8403A", 200, 200, 240, seed=5))

    def test_the_seed_changes_only_the_texture(self):
        for name, style in paint_styles.STYLES.items():
            with self.subTest(style=name):
                a, b = style(FLAT, "#D8403A", 200, 200, 240, seed=1), style(FLAT, "#D8403A", 200, 200, 240, seed=2)
                self.assertNotEqual(a, b)
                self.assertEqual(len(a), len(b))  # the structure (how many strokes) doesn't depend on the seed

    def test_paint_only(self):
        for name, style in paint_styles.STYLES.items():
            with self.subTest(style=name):
                cmds = _commands(style(FLAT, "#D8403A", 200, 200, 240, seed=1))
                self.assertEqual({c["tool"] for c in cmds if c["command"] == "set_tool"}, {"oils"})
                self.assertEqual({c["mode"] for c in cmds if c["command"] == "set_paint_mode"}, {"paint"})

    def test_options_are_obeyed(self):
        for name, style in paint_styles.STYLES.items():
            with self.subTest(style=name):
                cmds = _commands(style(FLAT, "#D8403A", 200, 200, 240, seed=1, hairs=9, splatter=13, **({"drips": 4} if name == "block" else {})))
                flecks = [c for c in cmds if c["command"] == "set_brush" and c["preset"] == paint_styles.SPECK_BRUSH]
                hairlines = [c for c in cmds if c["command"] == "set_brush" and c["preset"] == FLAT["name"] and c["bristles"] == 0]
                self.assertEqual(len(flecks), 13)
                self.assertEqual(len(hairlines), _expected_hairlines(name, 9, 4))

    def test_no_splatter_when_asked_for_none(self):
        for name, style in paint_styles.STYLES.items():
            with self.subTest(style=name):
                cmds = _commands(style(FLAT, "#D8403A", 200, 200, 240, seed=1, splatter=0))
                self.assertFalse([c for c in cmds if c["command"] == "set_brush" and c["preset"] == paint_styles.SPECK_BRUSH])

    def test_paint_settings_arrive_with_only_the_texture_wander(self):
        wander = paint_styles.TEXTURE + 0.01
        for name, style in paint_styles.STYLES.items():
            with self.subTest(style=name):
                cmds = _commands(style(FLAT, "#D8403A", 200, 200, 240, seed=1, paint={"loading": 1.0, "thickness": 0.5, "bristles": 100}))
                body = [c for c in cmds if c["command"] == "set_brush" and c.get("bristles", 0) > 0]
                self.assertGreater(len(body), 3)
                for c in body:
                    self.assertGreaterEqual(c["loading"], 1.0 - wander)
                    self.assertAlmostEqual(c["thickness"], 0.5, delta=wander)
                    self.assertTrue(80 <= c["bristles"] <= 120, c["bristles"])  # 100, up to 20% either way

    def test_unsent_paint_settings_use_the_defaults(self):
        cmds = _commands(paint_styles.slash(FLAT, "#D8403A", 200, 200, 240, seed=1))
        body = [c for c in cmds if c["command"] == "set_brush" and c.get("bristles", 0) > 0]
        self.assertAlmostEqual(sum(c["loading"] for c in body) / len(body), paint_styles.PAINT_DEFAULTS["loading"], delta=0.08)

    def test_stays_within_reach(self):
        for name, style in paint_styles.STYLES.items():
            with self.subTest(style=name):
                for seed in range(6):
                    cmds = style(FLAT, "#D8403A", 0, 0, 300, rotation=seed * 61, seed=seed)
                    far = max(math.hypot(p[0], p[1]) for c in _body_strokes(cmds) for p in c["points"])
                    self.assertLessEqual(far, paint_styles.reach(300) * 1.02, (name, seed))

    def test_rotation_turns_the_mark(self):
        for name, style in paint_styles.STYLES.items():
            with self.subTest(style=name):
                self.assertNotEqual(style(FLAT, "#D8403A", 200, 200, 240, seed=1), style(FLAT, "#D8403A", 200, 200, 240, seed=1, rotation=90))

    def test_position_moves_the_mark(self):
        a = _all_points(paint_styles.fan(FLAT, "#D8403A", 100, 100, 200, seed=3))
        b = _all_points(paint_styles.fan(FLAT, "#D8403A", 400, 300, 200, seed=3))
        self.assertAlmostEqual(b[0][0] - a[0][0], 300, delta=0.2)
        self.assertAlmostEqual(b[0][1] - a[0][1], 200, delta=0.2)

    def test_any_brush_works(self):
        for brush in STUB_BRUSHES:
            for name, style in paint_styles.STYLES.items():
                with self.subTest(style=name):
                    self.assertTrue(style(brush, "#D8403A", 200, 200, 200, seed=1))


# ---------- background ----------

class TestBackground(unittest.TestCase):
    def lines(self, **kw):
        args = dict(color="#EFE6D2", width=800, height=600, brush=FLAT, seed=1, kind="linen", depth=0.4)
        args.update(kw)
        return background.background(**args)

    def test_same_inputs_same_lines(self):
        self.assertEqual(self.lines(), self.lines())

    def test_the_values_sent_arrive(self):
        cmds = _commands(self.lines(color="#123456", kind="rough", depth=0.7))
        paper = next(c for c in cmds if c["command"] == "set_paper")
        self.assertEqual((paper["color"], paper["kind"], paper["depth"]), ("#123456", "rough", 0.7))
        self.assertIn({"command": "set_color", "hex": "#123456"}, cmds)
        self.assertEqual(next(c for c in cmds if c["command"] == "set_brush")["preset"], "Bristle Flat")

    def test_the_seed_changes_only_the_wobble(self):
        a, b = self.lines(seed=1), self.lines(seed=2)
        self.assertNotEqual(a, b)
        self.assertEqual(len(a), len(b))

    def test_rows_cover_the_whole_canvas_and_end_off_it(self):
        strokes = _strokes(self.lines())  # a 600 px tall canvas: the brush radius is 600 / 6 = 100
        ys = [s["points"][0][1] for s in strokes]
        self.assertLessEqual(min(ys), 20)  # the first row is at the top
        self.assertGreaterEqual(max(ys) + 100, 600)  # the last row's brush reaches the bottom edge
        self.assertTrue(all(b - a < 2 * 100 for a, b in zip(ys, ys[1:])))  # rows overlap, so no paper shows between
        for s in strokes:
            self.assertLess(s["points"][0][0], 0)
            self.assertGreater(s["points"][-1][0], 800)

    def test_names_the_brush_and_ends_dry(self):
        lines = self.lines()
        self.assertEqual(lines[0], "# Background (brush: Bristle Flat)")
        self.assertEqual(json.loads(lines[-1]), {"command": "dry"})


# ---------- points ----------

class TestPoints(unittest.TestCase):
    W, H = 1200, 900

    def rounded(self, pts):
        return [(round(x), round(y)) for x, y in pts]

    def test_thirds(self):
        self.assertEqual(self.rounded(points.thirds_points(self.W, self.H)), [(400, 300), (800, 300), (400, 600), (800, 600)])

    def test_golden(self):
        self.assertEqual(self.rounded(points.golden_points(self.W, self.H)), [(458, 344), (742, 344), (458, 556), (742, 556)])

    def test_diagonal(self):
        self.assertEqual(self.rounded(points.diagonal_points(self.W, self.H)), [(180, 135), (458, 344), (742, 556), (1020, 765)])

    def test_spiral_winds_in_to_the_eye(self):
        spiral = points.spiral_points(self.W, self.H)
        self.assertEqual(self.rounded(spiral), [(371, 450), (971, 278), (1058, 728), (829, 794), (829, 622)])
        eye = spiral[-1]
        distances = [math.hypot(x - eye[0], y - eye[1]) for x, y in spiral]
        self.assertTrue(all(a > b for a, b in zip(distances, distances[1:])))
        self.assertEqual(len(points.spiral_points(self.W, self.H, 8)), 8)

    def test_every_rule_and_mirror_stays_on_the_canvas(self):
        for name, rule in points.RULES.items():
            for flip_x in (False, True):
                for flip_y in (False, True):
                    for x, y in points.mirror(rule(self.W, self.H), self.W, self.H, flip_x, flip_y):
                        self.assertTrue(0 < x < self.W and 0 < y < self.H, (name, flip_x, flip_y))

    def test_mirror(self):
        self.assertEqual(points.mirror([(100, 200)], 1000, 500, flip_x=True), [(900, 200)])
        self.assertEqual(points.mirror([(100, 200)], 1000, 500, flip_y=True), [(100, 300)])
        self.assertEqual(points.mirror([(100, 200)], 1000, 500), [(100, 200)])

    def test_pick_points_gives_different_points_with_a_diagonal_pair(self):
        strong = points.golden_points(self.W, self.H)
        for seed in range(20):
            first, second = points.pick_points(random.Random(seed), strong, 2)
            self.assertNotEqual(first, second)
            self.assertAlmostEqual(first[0] + second[0], self.W, delta=0.01)  # diagonal opposites mirror through the middle
            self.assertAlmostEqual(first[1] + second[1], self.H, delta=0.01)
        self.assertEqual(len(points.pick_points(random.Random(1), strong, 9)), 4)

    def test_ordered_pick_keeps_the_order(self):
        spiral = points.spiral_points(self.W, self.H)
        self.assertEqual(points.pick_points(random.Random(1), spiral, 3, ordered=True), spiral[:3])

    def test_nudge_stays_within_the_amount(self):
        rng = random.Random(1)
        for _ in range(200):
            x, y = points.nudge(rng, (600, 450), self.W, self.H, amount=0.04)
            self.assertLessEqual(abs(x - 600), 0.04 * self.W + 1e-9)
            self.assertLessEqual(abs(y - 450), 0.04 * self.H + 1e-9)

    def test_fit_keeps_an_object_on_the_canvas(self):
        self.assertEqual(points.fit(20, 880, 100, self.W, self.H), (100, 800))
        self.assertEqual(points.fit(600, 450, 100, self.W, self.H), (600, 450))
        self.assertEqual(points.fit(5, 5, 700, self.W, self.H), (600, 450))  # too big: centered

    def test_golden_sizes_shrink_by_the_golden_ratio(self):
        sizes = points.golden_sizes(300, 4)
        self.assertEqual(sizes[0], 300)
        self.assertAlmostEqual(sizes[1] / sizes[0], 0.618, places=3)


# ---------- randomizers and brushes ----------

class TestRandomizers(unittest.TestCase):
    def test_paint_settings_stay_in_their_ranges(self):
        rng = random.Random(1)
        for _ in range(200):
            s = randomizers.paint_settings(rng)
            for name, (low, high) in randomizers.PAINT_RANGES.items():
                self.assertTrue(low <= s[name] <= high, (name, s[name]))
            self.assertTrue(0 <= s["loading"] <= 1 and 0 <= s["thickness"] <= 1)

    def test_loading_and_thickness_are_totally_random(self):
        draws = [randomizers.paint_settings(random.Random(seed)) for seed in range(300)]
        for name in ("loading", "thickness"):
            values = [d[name] for d in draws]
            self.assertLess(min(values), 0.05)
            self.assertGreater(max(values), 0.95)

    def test_loading_leans_low_by_default(self):
        self.assertGreater(brushes.SETTING_BIAS["loading"], 1)
        for make in (lambda r: brushes.random_settings(r, FLAT, 0.3)["loading"],
                     lambda r: randomizers.paint_settings(r)["loading"]):
            values = sorted(make(random.Random(seed)) for seed in range(400))
            self.assertLess(values[200], 0.5)  # the median is in the lower half of the range
        # A setting with no bias (or bias 1) is even; below 1 leans high; same draw count either way.
        evens = sorted(brushes.lean(random.Random(s), "opacity", 0, 1) for s in range(400))
        self.assertAlmostEqual(evens[200], 0.5, delta=0.1)
        a, b = random.Random(5), random.Random(5)
        brushes.lean(a, "loading", 0, 1)
        brushes.lean(b, "opacity", 0, 1)
        self.assertEqual(a.random(), b.random())

    def test_a_conductors_tuning_reaches_every_kind_of_mark(self):
        pinned = {"weights": {name: 0 for name in brushes.PAINT_SETTINGS}, "bias": {}}
        middles = {name: v[2] for name, v in brushes.PAINT_SETTINGS.items()}
        for seed in range(10):
            rng = random.Random(seed)
            shape = randomizers.shape_args(rng, PALETTE, WEIGHTS, STUB_BRUSHES, 900, tuning=pinned)
            cl = randomizers.cluster_args(rng, PALETTE, WEIGHTS, STUB_BRUSHES, 900, tuning=pinned)
            for settings in [shape["settings"]] + [m["settings"] for m in cl["marks"]]:
                for name, middle in middles.items():
                    if settings:
                        self.assertEqual(settings[name], middle, name)
        # Paint-style loading follows the conductor's bias: high power = low loading, low power = high.
        low = [randomizers.paint_settings(random.Random(i), {"bias": {"loading": 4}})["loading"] for i in range(200)]
        high = [randomizers.paint_settings(random.Random(i), {"bias": {"loading": 0.25}})["loading"] for i in range(200)]
        self.assertLess(sum(low) / 200, 0.3)
        self.assertGreater(sum(high) / 200, 0.7)
        # A tuning is optional everywhere: leaving it out is the same as brushes.py's own tables.
        self.assertEqual(randomizers.paint_settings(random.Random(1)), randomizers.paint_settings(random.Random(1), {}))

    def test_style_options_match_what_each_style_accepts(self):
        for name, style in paint_styles.STYLES.items():
            for seed in range(5):
                a = randomizers.style_args(random.Random(seed), PALETTE, WEIGHTS, STUB_BRUSHES, 900, name=name)
                self.assertEqual(a["name"], name)
                self.assertTrue(style(a["brush"], a["color"], 400, 400, a["size"], rotation=a["rotation"],
                                      brush_size=a["brush_size"], paint=a["paint"], seed=a["seed"], **a["options"]), name)

    def test_the_slash_leans_both_ways(self):
        headings = [randomizers.style_options(random.Random(s), "slash")["heading"] for s in range(60)]
        self.assertTrue(min(headings) < 0 < max(headings))
        self.assertTrue(all(25 <= abs(h) <= 55 for h in headings))

    def test_counts_are_whole_numbers(self):
        o = randomizers.style_options(random.Random(1), "fan")
        self.assertIsInstance(o["hairs"], int)
        self.assertIsInstance(o["splatter"], int)

    def test_every_style_has_ranges(self):
        self.assertEqual(set(randomizers.STYLE_RANGES), set(paint_styles.STYLES))

    def test_shape_args_work_with_the_shapes(self):
        kinds = set()
        for seed in range(40):
            a = randomizers.shape_args(random.Random(seed), PALETTE, WEIGHTS, STUB_BRUSHES, 900)
            kinds.add(a["kind"])
            self.assertIn(a["tool"], randomizers.TOOL_WEIGHTS)
            if a["kind"] == "circle":
                self.assertEqual(a["rotation"], 0)
            self.assertTrue(getattr(shapes, a["kind"])(a["brush"], a["color"], 400, 400, a["size"], fill=a["fill"],
                                                      rotation=a["rotation"], tool=a["tool"], brush_size=a["brush_size"],
                                                      settings=a["settings"]))
        self.assertEqual(kinds, {"circle", "square", "triangle"})

    def test_cluster_args_make_6_to_24_marks_that_work(self):
        counts = set()
        for seed in range(60):
            a = randomizers.cluster_args(random.Random(seed), PALETTE, WEIGHTS, STUB_BRUSHES, 900)
            counts.add(len(a["marks"]))
            self.assertTrue(cluster.Cluster(x=300, y=300, **a).commands())
            for m in a["marks"]:
                self.assertLessEqual(math.hypot(m["u"], m["v"]), 1.0 + 1e-9)
                self.assertTrue(0.08 <= m["length"] <= 0.35 and -0.3 <= m["bend"] <= 0.3)
                self.assertTrue(0.5 <= m["radius"] <= 200)
        self.assertEqual((min(counts), max(counts)), (6, 24))

    def test_background_args_work_with_the_background(self):
        a = randomizers.background_args(random.Random(1), PALETTE, STUB_BRUSHES)
        self.assertIn(a["color"], PALETTE)
        self.assertIn(a["kind"], randomizers.SURFACES)
        self.assertTrue(background.background(width=800, height=600, **a))

    def test_same_rng_same_values(self):
        a = randomizers.style_args(random.Random(9), PALETTE, WEIGHTS, STUB_BRUSHES, 900)
        b = randomizers.style_args(random.Random(9), PALETTE, WEIGHTS, STUB_BRUSHES, 900)
        self.assertEqual(a, b)

    def test_tool_weights_make_oils_the_most_likely(self):
        rng = random.Random(1)
        picks = [randomizers.pick_tool(rng) for _ in range(2000)]
        self.assertGreater(picks.count("oils"), 0.4 * 2000)
        self.assertEqual(set(picks), set(randomizers.TOOL_WEIGHTS))

    def test_jitter_color(self):
        self.assertEqual(randomizers.jitter_color(random.Random(1), "#C2151B", 0), "#C2151B")
        moved = randomizers.jitter_color(random.Random(1), "#C2151B", 0.1)
        self.assertRegex(moved, r"^#[0-9A-F]{6}$")
        self.assertNotEqual(moved, "#C2151B")

    def test_random_settings_for_a_brush(self):
        self.assertEqual(brushes.random_settings(random.Random(1), FLAT, 0), {})
        s = brushes.random_settings(random.Random(1), FLAT, 0.3)
        self.assertIn("bristles", s)  # a bristle brush keeps its hairs
        self.assertNotIn("taper", s)  # a brush that doesn't taper doesn't start to
        self.assertNotIn("scatter", brushes.random_settings(random.Random(1), ROUND, 0.5))
        for name in ("hardness", "roundness", "roughness", "streaks", "lift_off", "opacity", "loading", "thickness"):
            if name in s:
                self.assertTrue(0 <= s[name] <= 1, name)
        self.assertTrue(brushes.random_settings(random.Random(2), SPONGE, 0.5)["scatter"] >= 0)

    def test_setting_weights_limit_the_randomness(self):
        # Weight 0: a paint setting sits at its middle and a shape setting at the brush's own value.
        for seed in range(30):
            s = brushes.random_settings(random.Random(seed), FLAT, 0.5, {"weights": {"oiliness": 0, "opacity": 0, "hardness": 0}})
            self.assertEqual(s["oiliness"], 0.4)
            self.assertEqual(s["opacity"], 0.85)
            self.assertEqual(s["hardness"], FLAT["hardness"])
        # Half weight: stays inside half the way from the middle to each end.
        for seed in range(100):
            s = brushes.random_settings(random.Random(seed), FLAT, 0.3, {"weights": {"loading": 0.5}})
            self.assertTrue(0.475 - 0.001 <= s["loading"] <= 0.875 + 0.001, s["loading"])
        # No weights = the defaults (everything at full weight): same as passing {}.
        self.assertEqual(brushes.random_settings(random.Random(3), FLAT, 0.3),
                         brushes.random_settings(random.Random(3), FLAT, 0.3, {}))
        # A weight changes the value but not how many random draws are used.
        a, b = random.Random(4), random.Random(4)
        brushes.random_settings(a, FLAT, 0.3)
        brushes.random_settings(b, FLAT, 0.3, {"weights": {"grain": 0, "streaks": 0}})
        self.assertEqual(a.random(), b.random())


# ---------- palettes ----------

class TestGround(unittest.TestCase):
    def test_it_paints_only_below_the_horizon_and_to_the_bottom(self):
        cmds = _commands(ground.ground("#556B2F", 800, 600, 250, FLAT, seed=3))
        rows = [c["points"][0][1] for c in cmds if c["command"] == "stroke"]
        self.assertGreaterEqual(min(rows), 250 - 20)  # no row starts well above the horizon
        radius = next(c["size"] for c in cmds if c["command"] == "set_brush")
        self.assertGreaterEqual(max(rows) + radius, 600)  # the last row reaches the bottom
        self.assertEqual(cmds[-1]["command"], "dry")
        self.assertEqual({c["hex"] for c in cmds if c["command"] == "set_color"}, {"#556B2F"})

    def test_same_inputs_same_commands(self):
        self.assertEqual(ground.ground("#556B2F", 800, 600, 250, FLAT, seed=3), ground.ground("#556B2F", 800, 600, 250, FLAT, seed=3))
        self.assertNotEqual(ground.ground("#556B2F", 800, 600, 250, FLAT, seed=3), ground.ground("#556B2F", 800, 600, 250, FLAT, seed=4))

    def test_the_random_ground_avoids_the_sky_color(self):
        for seed in range(30):
            a = randomizers.ground_args(random.Random(seed), PALETTE, STUB_BRUSHES, avoid=PALETTE[0])
            self.assertNotEqual(a["color"], PALETTE[0])
        self.assertEqual(randomizers.ground_args(random.Random(1), ["#112233"], STUB_BRUSHES, avoid="#112233")["color"], "#112233")


class TestPalettes(unittest.TestCase):
    def test_blend_mixes_toward_the_other_color(self):
        self.assertEqual(palettes.blend("#000000", "#FFFFFF", 0.5), "#808080")
        self.assertEqual(palettes.blend("#C2151B", "#2021A0", 0), "#C2151B")
        self.assertEqual(palettes.blend("#C2151B", "#2021A0", 1), "#2021A0")
        self.assertEqual(palettes.blend("#fa0", "#000", 0), "#FFAA00")

    def test_every_palette_is_a_list_of_hex_colors(self):
        everything = palettes.load_palettes()
        self.assertGreater(len(everything), 50)
        for name, colors in everything.items():
            self.assertTrue(colors, name)
            for color in colors:
                self.assertRegex(color, r"^#[0-9A-Fa-f]{6}$", name)

    def test_load_palette_ignores_case_and_keeps_the_files_spelling(self):
        p = palettes.load_palette("LEMONADE")
        self.assertEqual(p["name"], "Lemonade")
        self.assertEqual(p["colors"], palettes.load_palettes()["Lemonade"])

    def test_an_unknown_palette_stops_with_the_names(self):
        with self.assertRaises(SystemExit) as stopped:
            palettes.load_palette("no-such-palette")
        self.assertIn("no palette 'no-such-palette'", str(stopped.exception))

    def test_role_weights_are_60_30_10(self):
        self.assertEqual(palettes.role_weights(1), [1.0])
        self.assertEqual(palettes.role_weights(2), [0.7, 0.3])
        self.assertEqual([round(w, 6) for w in palettes.role_weights(5)], [0.6, 0.1, 0.1, 0.1, 0.1])  # 0.3 / 3 isn't exactly 0.1
        for n in range(1, 13):
            self.assertAlmostEqual(sum(palettes.role_weights(n)), 1.0)
            self.assertEqual(len(palettes.role_weights(n)), n)

    def test_luminance(self):
        self.assertEqual(palettes.luminance("#000000"), 0.0)
        self.assertAlmostEqual(palettes.luminance("#ffffff"), 1.0)
        self.assertAlmostEqual(palettes.luminance("#808080"), 0.5, delta=0.01)
        self.assertAlmostEqual(palettes.luminance("#fff"), 1.0)  # short hex
        self.assertGreater(palettes.luminance("#00ff00"), palettes.luminance("#0000ff"))  # green looks brighter than blue


# ---------- composition ----------

class TestComposition(unittest.TestCase):
    W, H = 1200, 900

    def make(self, seed=1):
        return composition.Composition(random.Random(seed), self.W, self.H)

    def test_same_seed_same_layout(self):
        a, b = self.make(5), self.make(5)
        self.assertEqual((a.rule, a.flip, a.points, a.focal, a.base_angle), (b.rule, b.flip, b.points, b.focal, b.base_angle))
        self.assertNotEqual((a.points, a.focal), (self.make(6).points, self.make(6).focal))

    def test_the_layout_is_one_of_the_rules_and_stays_on_the_canvas(self):
        rules = set()
        for seed in range(60):
            c = self.make(seed)
            rules.add(c.rule)
            self.assertIn(c.rule, points.RULES)
            self.assertTrue(all(0 < x < self.W and 0 < y < self.H for x, y in c.points))
            self.assertTrue(0 < c.focal[0] < self.W and 0 < c.focal[1] < self.H)
        self.assertEqual(rules, set(points.RULES))

    def test_the_focal_point_of_a_spiral_is_near_its_eye(self):
        for seed in range(80):
            c = self.make(seed)
            if c.rule == "spiral":
                eye = c.points[-1]
                self.assertLessEqual(abs(c.focal[0] - eye[0]), 0.04 * self.W + 1e-9)
                self.assertLessEqual(abs(c.focal[1] - eye[1]), 0.04 * self.H + 1e-9)

    def test_anchor_keeps_an_object_on_the_canvas(self):
        for seed in range(30):
            c = self.make(seed)
            for extent in (20, 150, 300, 440):
                x, y = c.anchor(extent)
                self.assertTrue(extent <= x <= self.W - extent and extent <= y <= self.H - extent, (seed, extent, x, y))

    def test_anchor_goes_to_the_point_asked_for(self):
        c = self.make(2)
        for point in c.points:
            x, y = c.anchor(10, point)
            self.assertLessEqual(abs(x - point[0]), 0.04 * self.W + 10)
            self.assertLessEqual(abs(y - point[1]), 0.04 * self.H + 10)

    def test_clamp_keeps_a_point_inside_the_margin(self):
        c = self.make(1)
        self.assertEqual(c.clamp(-50, -50), (c.margin, c.margin))
        self.assertEqual(c.clamp(5000, 5000), (self.W - c.margin, self.H - c.margin))
        self.assertEqual(c.clamp(600, 450), (600, 450))

    def test_start_points_are_inside_the_canvas_and_gather_near_the_focal_point(self):
        c = self.make(3)
        near = [c.start_point(1.0) for _ in range(200)]
        anywhere = [c.start_point(0.0) for _ in range(200)]
        self.assertTrue(all(0 <= x <= self.W and 0 <= y <= self.H for x, y in near + anywhere))
        mean_distance = lambda pts: sum(math.hypot(x - c.focal[0], y - c.focal[1]) for x, y in pts) / len(pts)
        self.assertLess(mean_distance(near), mean_distance(anywhere))

    def test_a_flow_path_starts_where_asked_and_stays_inside_the_margin(self):
        c = self.make(4)
        for against in (False, True):
            path = c.flow_path((600, 450), 300, against=against)
            self.assertEqual(path[0][:2], [600.0, 450.0])
            self.assertTrue(all(0.35 <= p[2] <= 1.0 for p in path))
            self.assertTrue(all(c.margin - 1e-6 <= p[0] <= self.W - c.margin + 1e-6 for p in path))
        self.assertNotEqual(c.flow_path((600, 450), 300), c.flow_path((600, 450), 300, against=True))

    def test_the_flow_changes_smoothly(self):
        c = self.make(7)
        for x, y in ((100, 100), (600, 450), (1000, 800)):
            self.assertAlmostEqual(c.angle_at(x, y), c.angle_at(x + 1, y + 1), delta=0.02)


# ---------- commands ----------

class TestCommands(unittest.TestCase):
    def test_brush_command(self):
        self.assertEqual(commands.brush_command(FLAT, size=40, oiliness=0.3),
                         {"command": "set_brush", "preset": "Bristle Flat", "size": 40, "oiliness": 0.3})

    def test_color_command(self):
        self.assertEqual(commands.color_command("#4F7329"), {"command": "set_color", "hex": "#4F7329"})

    def test_write_script_writes_one_line_per_command(self):
        items = ["# a comment", {"command": "new_painting", "width": 100, "height": 80}, "# another", {"command": "dry"}]
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "script.jsonl"
            commands.write_script(items, path)
            lines = path.read_text().splitlines()
        self.assertEqual(lines[0], "# a comment")
        self.assertEqual([json.loads(line) for line in lines if not line.startswith("#")],
                         [{"command": "new_painting", "width": 100, "height": 80}, {"command": "dry"}])

    def test_finish_exports_then_saves(self):
        self.assertEqual(commands.finish("a.png", "a.oxide"),
                         [{"command": "export", "path": "a.png"}, {"command": "save", "path": "a.oxide"}])

    def test_the_repo_folder_is_where_cargo_runs(self):
        self.assertTrue((commands.REPO / "Cargo.toml").exists())

    def test_read_script_reads_what_write_script_wrote(self):
        items = ["# a comment", {"command": "new_painting", "width": 100, "height": 80}, {"command": "stroke", "points": [[1, 2, 0.5]], "smooth": True}]
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "script.jsonl"
            commands.write_script(items, path)
            self.assertEqual(commands.read_script(path), items)
            path.write_text('{"command": "dry"}\n\nnot json\n')
            with self.assertRaisesRegex(ValueError, "line 3"):
                commands.read_script(path)


# ---------- recordings: .jsonl scripts turned into Python functions ----------

RECORDING = [
    "# Recorded in Oxide. Paint it again with:",
    "#   cargo run -q --release -p oxide-agent --bin oxide-run -- <this file>",
    {"command": "new_painting", "width": 400, "height": 300, "dpi": 200.0, "paper": "canvas"},
    {"command": "set_paper", "gloss": 8.0},
    "# brush: Bristle Flat",
    {"command": "set_brush", "size": 10.0, "loading": 0.5},
    {"command": "set_color", "hex": "#C2151B"},
    {"command": "stroke", "points": [[100.0, 100.0, 0.5], [200.0, 100.0, 0.8]]},
    {"command": "set_multicolor", "colors": ["#C2151B", "#2021A0"]},
    {"command": "hatch", "rail_a": [[100, 150], [200, 150]], "rail_b": [[100, 200], [200, 200]], "spacing": 6.0, "seed": 3},
    {"command": "select", "shape": "rect", "x0": 100, "y0": 100, "x1": 200, "y1": 200, "mode": "replace"},
    {"command": "export", "path": "/tmp/x.png"},
]


class TestRecordings(unittest.TestCase):
    ORIGIN = (150.0, 150.0)

    def test_nothing_sent_changes_nothing(self):
        self.assertEqual(recordings.transform(RECORDING, self.ORIGIN), RECORDING)

    def test_move_puts_the_middle_at_x_y(self):
        moved = recordings.transform(RECORDING, self.ORIGIN, x=250, y=100)
        self.assertEqual(moved[7]["points"], [[200.0, 50.0, 0.5], [300.0, 50.0, 0.8]])
        self.assertEqual(moved[10]["x0"], 200)
        self.assertEqual(recordings.bounds(moved), (200, 50, 300, 150))

    def test_scale_scales_paths_and_brushes(self):
        half = recordings.transform(RECORDING, self.ORIGIN, scale=0.5)
        self.assertEqual(half[7]["points"][0], [125.0, 125.0, 0.5])
        self.assertEqual(half[5]["size"], 5.0)
        self.assertEqual(half[9]["spacing"], 3.0)

    def test_paint_thickness_scales_with_the_size(self):
        # Impasto height is in fixed units and lit pixel to pixel: a smaller copy needs thinner paint
        # to look the same (otherwise it looks heavier and more loaded). Loading etc. stay as recorded.
        cmds = [{"command": "set_brush", "size": 140.0, "loading": 0.22, "length": 0.45, "thickness": 0.25}]
        small = recordings.transform(cmds, self.ORIGIN, scale=0.5)[0]
        self.assertEqual(small, {"command": "set_brush", "size": 70.0, "loading": 0.22, "length": 0.45, "thickness": 0.01562})
        self.assertEqual(recordings.transform(cmds, self.ORIGIN, scale=2)[0]["thickness"], 0.5)   # bigger: by the scale
        self.assertEqual(recordings.transform(cmds, self.ORIGIN, scale=8)[0]["thickness"], 1.0)   # at most 1

    def test_rotation_turns_clockwise_around_the_middle(self):
        turned = recordings.transform(RECORDING, self.ORIGIN, rotation=90)
        # (100, 100) is up-left of the middle; a quarter turn clockwise puts it up-right.
        self.assertEqual(turned[7]["points"][0][:2], [200.0, 100.0])
        self.assertEqual(turned[10]["shape"], "lasso")
        self.assertEqual(len(turned[10]["points"]), 4)

    def test_colors_are_swapped(self):
        swapped = recordings.transform(RECORDING, self.ORIGIN, colors={"#c2151b": "#00FF00"})
        self.assertEqual(swapped[6]["hex"], "#00FF00")
        self.assertEqual(swapped[8]["colors"], ["#00FF00", "#2021A0"])

    def test_reach_covers_the_paint(self):
        self.assertAlmostEqual(recordings.reach(RECORDING, self.ORIGIN), math.hypot(50, 50) + 10)

    def test_it_starts_from_the_hand_oxide_run_starts_with(self):
        cmds = recordings.with_start([{"command": "stroke", "points": [[1, 1]]}, {"command": "set_tool", "tool": "ink"},
                                      {"command": "set_brush", "size": 5}, {"command": "set_tool", "tool": "oils"},
                                      {"command": "set_tool", "tool": "ink"}])
        names = [c["command"] for c in cmds]
        self.assertEqual(names[:5], ["set_tool", "set_brush", "set_paint_mode", "set_multicolor", "set_color"])
        self.assertEqual(cmds[4]["hex"], "#000000")
        # Ink's brush is reset the first time only, before the recording's own set_brush.
        ink = [i for i, c in enumerate(cmds) if c == {"command": "set_tool", "tool": "ink"}]
        self.assertEqual(cmds[ink[0] + 1], {"command": "set_brush", **recordings.START_BRUSHES["ink"]})
        self.assertEqual(cmds[ink[0] + 2], {"command": "set_brush", "size": 5})
        self.assertEqual(cmds[-1], {"command": "set_tool", "tool": "ink"})

    def test_layer_setup_before_the_first_stroke_is_left_out(self):
        recorded = [{"command": "set_layer", "index": 0, "name": "Layer 1"}, {"command": "stroke", "points": [[1, 1], [9, 9]]},
                    {"command": "add_layer", "name": "Later"}]
        text = recordings.to_python(recorded, "x")
        self.assertNotIn("'Layer 1'", text)
        self.assertIn("'Later'", text)
        self.assertIn("layer setup before the first stroke", text)

    def test_picked_colors_and_the_unpicked_starting_black(self):
        stroke = {"command": "stroke", "points": [[1, 1], [9, 9]]}
        red = {"command": "set_color", "hex": "#C2151B"}
        # Only picked colors: they are the defaults, nothing to fill in.
        self.assertEqual(recordings.color_use([red, stroke]), (["#C2151B"], []))
        # Never picked: painted in the starting black.
        self.assertEqual(recordings.color_use([stroke]), ([], ["#000000"]))
        # Black first, then a pick: both.
        self.assertEqual(recordings.color_use([stroke, red, stroke]), (["#C2151B"], ["#000000"]))
        # A set_color to the color in use cleans the brush; it isn't a pick.
        self.assertEqual(recordings.color_use([stroke, {"command": "set_color", "hex": "#000000"}, stroke]), ([], ["#000000"]))
        # The eraser puts no color down; a MultiColored brush is picked colors.
        eraser = [{"command": "set_tool", "tool": "eraser"}, stroke]
        self.assertEqual(recordings.color_use(eraser), ([], []))
        self.assertEqual(recordings.color_use([{"command": "set_multicolor", "colors": ["#111111", "#222222"]}, stroke]),
                         (["#111111", "#222222"], []))
        # A color picked from the canvas is chosen, though its hex isn't known.
        self.assertEqual(recordings.color_use([{"command": "pick_color", "x": 5, "y": 5}, stroke]), ([], []))

    def test_recording_colors_keeps_picked_and_fills_unpicked_from_the_palette(self):
        class Picked:
            UNPICKED = []

        class Unpicked:
            UNPICKED = ["#000000"]

        self.assertEqual(randomizers.recording_colors(random.Random(1), Picked, PALETTE), {})
        swap = randomizers.recording_colors(random.Random(1), Unpicked, PALETTE)
        self.assertEqual(list(swap), ["#000000"])
        self.assertIn(swap["#000000"], PALETTE)
        self.assertEqual(swap, randomizers.recording_colors(random.Random(1), Unpicked, PALETTE))

    def test_palette_swap_matches_colors_by_lightness(self):
        recorded = ["#FFFFFF", "#000000", "#808080"]
        palette = ["#E2C43F", "#1C2433", "#4F7329"]  # light, dark, middle (not in order)
        self.assertEqual(recordings.palette_swap(recorded, palette),
                         {"#000000": "#1C2433", "#808080": "#4F7329", "#FFFFFF": "#E2C43F"})
        # More recorded colors than palette colors: they share, darkest still to darkest.
        swap = recordings.palette_swap(["#000000", "#333333", "#999999", "#FFFFFF"], ["#1C2433", "#E2C43F"])
        self.assertEqual((swap["#000000"], swap["#FFFFFF"]), ("#1C2433", "#E2C43F"))
        self.assertEqual(recordings.palette_swap(["#808080"], palette), {"#808080": "#4F7329"})
        self.assertEqual(recordings.palette_swap(recorded, None), {})

    def test_shade_swap_keeps_each_lightness_in_one_hue(self):
        import colorsys
        def hls(color):
            return colorsys.rgb_to_hls(*recordings._rgb(color))
        recorded = ["#D7712C", "#2A1A10", "#F2C9AE", "#000000", "#FFFFFF"]
        swap = recordings.shade_swap(recorded, "#2E6B8F")
        self.assertEqual(set(swap), set(recorded))
        hue = hls("#2E6B8F")[0]
        for old, new in swap.items():
            self.assertAlmostEqual(hls(new)[1], hls(old)[1], delta=0.01)  # same lightness
            if old not in ("#000000", "#FFFFFF"):
                self.assertAlmostEqual(hls(new)[0], hue, delta=0.01)  # the shade's hue
        self.assertEqual((swap["#000000"], swap["#FFFFFF"]), ("#000000", "#FFFFFF"))
        self.assertEqual(recordings.shade_swap(recorded, None), {})
        # Which wins: colors over shade over palette.
        both = recordings.color_swap(recorded, colors={"#D7712C": "#00FF00"}, palette=PALETTE, shade="#2E6B8F")
        self.assertEqual(both["#D7712C"], "#00FF00")
        self.assertEqual(both["#2A1A10"], swap["#2A1A10"])
        self.assertEqual(recordings.color_swap(recorded), {})

    def test_every_recording_takes_a_shade(self):
        for name in ("plant1", "falltree1", "kg", "mark1", "mark2", "mark3", "mark4"):
            module = importlib.import_module(f"recorded.{name}")
            cmds = getattr(module, name)(shade="#2E6B8F")
            used = [c["hex"] for c in cmds if isinstance(c, dict) and c["command"] == "set_color"]
            used = set(used[1:] if used[0] == "#000000" and "#000000" not in module.COLORS[1:] + module.UNPICKED else used)
            self.assertTrue(used <= set(recordings.shade_swap(module.PICKED + module.UNPICKED, "#2E6B8F").values()), name)

    def test_a_converted_function_takes_a_palette(self):
        with tempfile.TemporaryDirectory() as folder:
            folder = Path(folder)
            commands.write_script(RECORDING, folder / "swirl.jsonl")
            spec = importlib.util.spec_from_file_location("converted_swirl2", recordings.convert(folder / "swirl.jsonl", out=folder))
            module = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(module)
        palette = ["#1C2433", "#E2C43F"]
        cmds = module.swirl(palette=palette)
        used = [c["hex"] for c in cmds if isinstance(c, dict) and c["command"] == "set_color"]
        self.assertEqual(used[0], "#000000")  # the starting black, never painted with: not mapped
        self.assertTrue(set(used[1:]) <= set(palette), used)
        multi = next(c for c in cmds if isinstance(c, dict) and c["command"] == "set_multicolor" and c["colors"])
        self.assertTrue(set(multi["colors"]) <= set(palette))
        # An exact swap wins over the palette.
        cmds = module.swirl(palette=palette, colors={"#C2151B": "#00FF00"})
        self.assertIn({"command": "set_color", "hex": "#00FF00"}, cmds)

    def test_picked_brushes_are_listed_from_the_brush_comments(self):
        self.assertEqual(recordings.picked_brushes(RECORDING), ["Bristle Flat"])
        self.assertEqual(recordings.picked_brushes(["# brush: A", "# brush: B", "# brush: A"]), ["A", "B"])
        self.assertEqual(recordings.picked_brushes([{"command": "stroke", "points": [[1, 1]]}]), [])

    def test_no_brush_sent_paints_as_recorded(self):
        cmds = recordings.with_start(RECORDING[4:])
        self.assertEqual(recordings.transform(cmds, self.ORIGIN), cmds)

    def test_a_brush_sent_replaces_the_recorded_one(self):
        cmds = recordings.with_start([c for c in RECORDING[4:] if isinstance(c, str) or c["command"] != "export"])
        for brush in ("Round Hard", {"name": "Round Hard", "size": 5}):
            out = recordings.transform(cmds, self.ORIGIN, brush=brush, scale=0.5)
            set_brushes = [c for c in out if isinstance(c, dict) and c["command"] == "set_brush"]
            start, *rest = set_brushes
            # The tool's starting brush stays first (so grain and pressure start from oxide-run's defaults)...
            self.assertEqual(start, {"command": "set_brush", **{k: v * 0.5 if k == "size" else round(v * 0.5 ** 4, 5) if k == "thickness" else v for k, v in recordings.START_BRUSHES["oils"].items()}})
            # ...then every set_brush is the new brush, at the recorded size (scaled) and paint settings.
            start_paint = {k: round(recordings.START_BRUSHES["oils"][k] * (0.5 ** 4 if k == "thickness" else 1), 5) for k in recordings.PAINT_SETTINGS}
            self.assertEqual(rest[0], {"command": "set_brush", "preset": "Round Hard", "size": 7.0, **start_paint})
            self.assertEqual(rest[-1], {"command": "set_brush", "preset": "Round Hard", "size": 5.0, "loading": 0.5})
            self.assertIn("# brush: Round Hard", out)
            self.assertNotIn("# brush: Bristle Flat", out)
            # The strokes themselves are the same.
            self.assertEqual([c for c in out if isinstance(c, dict) and c["command"] == "stroke"],
                             [c for c in recordings.transform(cmds, self.ORIGIN, scale=0.5) if isinstance(c, dict) and c["command"] == "stroke"])

    def test_function_names_are_python_names(self):
        self.assertEqual(recordings.function_name("My Swirl 2.jsonl"), "my_swirl_2")
        self.assertEqual(recordings.function_name("2024.jsonl"), "recording_2024")
        self.assertEqual(recordings.function_name("class.jsonl"), "recording_class")

    def test_a_converted_file_gives_back_the_commands(self):
        with tempfile.TemporaryDirectory() as folder:
            folder = Path(folder)
            commands.write_script(RECORDING, folder / "swirl.jsonl")
            path = recordings.convert(folder / "swirl.jsonl", out=folder / "recorded")
            self.assertEqual(path.name, "swirl.py")
            # Load the new file by its path (the real recorded/ package may be imported already).
            spec = importlib.util.spec_from_file_location("converted_swirl", path)
            module = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(module)
            as_painted = module.swirl()
            moved = module.swirl(x=250, y=100, scale=2)
            self.assertEqual(module.SIZE, (400, 300))
            self.assertEqual(module.ORIGIN, self.ORIGIN)
            self.assertEqual(module.COLORS, ["#000000", "#C2151B", "#2021A0"])
            self.assertEqual((module.PICKED, module.UNPICKED), (["#C2151B", "#2021A0"], []))
            self.assertEqual(module.BRUSHES, ["Bristle Flat"])
            self.assertEqual(module.swirl(brush="Round Hard"), recordings.with_brush(module.swirl(), "Round Hard"))
            self.assertAlmostEqual(module.reach(2), 2 * recordings.reach(RECORDING, self.ORIGIN), places=1)
        # The canvas and file commands (and the recording's header) are left out; the rest is as
        # recorded, after the starting hand oxide-run would have had.
        kept = [c for c in RECORDING[4:] if isinstance(c, str) or c["command"] not in ("new_painting", "set_paper", "export")]
        kept = recordings.with_start(kept)
        self.assertEqual(as_painted, kept)
        self.assertEqual(moved, recordings.transform(kept, self.ORIGIN, x=250, y=100, scale=2))


# ---------- who owns what (the layout of the files) ----------

class TestFileLayout(unittest.TestCase):
    """abstract.py is a conductor: it holds only its own painting logic. The shared parts live in modules
    any conductor can use."""

    def test_each_conductor_defines_only_its_own_logic(self):
        allowed = {abstract: {"generate", "main"}, abstract2: {"generate", "main"},
                   abstract3: {"generate", "main", "_as_dict"}, abstract4: {"generate", "main"}}
        for module, names in allowed.items():
            own = {n for n, o in vars(module).items() if (inspect.isfunction(o) or inspect.isclass(o)) and o.__module__ == module.__name__}
            self.assertEqual(own, names, module.__name__)

    def test_the_conductors_share_the_command_line(self):
        for module in (abstract, abstract2, abstract3, abstract4):
            self.assertIs(module.cli, cli, module.__name__)
        self.assertEqual(cli.run.__module__, "cli")

    def test_the_shared_parts_live_in_their_own_modules(self):
        owners = {palettes: ["load_palettes", "load_palette", "role_weights", "luminance", "blend"],
                  composition: ["Composition"],
                  commands: ["brush_command", "color_command", "finish", "write_script", "read_script", "run_script"],
                  recordings: ["transform", "bounds", "reach", "convert", "to_python"],
                  cli: ["run"],
                  makers: ["make_background", "make_paint_mark", "make_shape", "make_cluster", "make_tree", "make_ground"]}
        for module, names in owners.items():
            for name in names:
                self.assertEqual(getattr(module, name).__module__, module.__name__, name)


# ---------- the rule of the design ----------

class TestObjectFilesDoNotRandomize(unittest.TestCase):
    """The object files may use randomness only for fine texture, driven by a seed."""

    def test_no_global_random_calls(self):
        # Replace every module-level random function with one that fails; the objects must not call them.
        names = ["random", "uniform", "choice", "choices", "randint", "randrange", "gauss", "sample", "shuffle", "seed"]
        saved = {n: getattr(random, n) for n in names}

        def boom(*a, **k):
            raise AssertionError("an object file used the global random generator")

        for n in names:
            setattr(random, n, boom)
        try:
            marks = _ring(6)
            for fn in (shapes.circle, shapes.square, shapes.triangle):
                fn(FLAT, "#000000", 100, 100, 80, fill=True, rotation=20)
            cluster.Cluster(FLAT, "#000000", 100, 100, 100, marks).commands()
            for style in paint_styles.STYLES.values():
                style(FLAT, "#000000", 100, 100, 120, seed=1)
            background.background("#CCCCCC", 400, 300, FLAT, seed=1)
            ground.ground("#556B2F", 400, 300, 150, FLAT, seed=1)
            tree.Tree(FLAT, 300, 500, 400, TREE_COLORS, 30, 5, 0.05, 350, [TREE_BRANCH], [TREE_DAB], seed=1).commands()
        finally:
            for n, f in saved.items():
                setattr(random, n, f)

    def test_shapes_and_cluster_do_not_even_import_random(self):
        for module in (shapes, cluster, tree, recordings):
            self.assertFalse(hasattr(module, "random"), module.__name__)

    def test_object_files_do_not_import_the_randomizers(self):
        for module in (shapes, cluster, paint_styles, background, tree, ground):
            self.assertFalse(hasattr(module, "randomizers"), module.__name__)

    def test_the_randomizing_functions_are_not_in_the_object_files(self):
        for module in (shapes, cluster, paint_styles, background, tree):
            for name in ("random_shape", "random_cluster", "random_paint_style"):
                self.assertFalse(hasattr(module, name), (module.__name__, name))


# ---------- the conductor ----------

class TestConductor(unittest.TestCase):
    W, H = 1200, 900

    def painting(self, seed, width=None, height=None):
        return abstract.generate({"name": "test", "colors": PALETTE}, seed, width or self.W, height or self.H,
                                 "x.png", "x.oxide", STUB_BRUSHES)

    def test_same_seed_same_painting(self):
        self.assertEqual(self.painting(11), self.painting(11))
        self.assertNotEqual(self.painting(11), self.painting(12))

    def test_has_its_parts_in_order(self):
        cmds = self.painting(3)
        comments = [c for c in cmds if isinstance(c, str) and c.startswith("# ")]
        self.assertTrue(comments[0].startswith("# Abstract: palette test, seed 3, composition "))
        self.assertTrue(comments[1].startswith("# Background (brush: "))
        self.assertEqual(cmds[1]["command"], "new_painting")
        self.assertEqual([c["command"] for c in _commands(cmds[-2:])], ["export", "save"])

    def test_the_number_of_paint_marks_is_in_range_and_styles_dont_repeat_when_few(self):
        low, high = abstract.MARK_COUNT
        seen = set()
        for seed in range(60):
            cmds = self.painting(seed)
            header = next(c for c in cmds if isinstance(c, str) and c.startswith("# Paint marks"))
            count = int(header.split("(")[1].rstrip(")"))
            self.assertTrue(low <= count <= high, seed)
            seen.add(count)
            names = [c.split("(")[1].split(",")[0] for c in cmds if isinstance(c, str) and c.startswith("# Paint mark (")]
            self.assertEqual(len(names), count)
            if count <= len(paint_styles.STYLES):
                self.assertEqual(len(set(names)), count, seed)
        self.assertEqual(seen, set(range(low, high + 1)))

    def test_everything_stays_on_the_canvas_except_splatter(self):
        # The background rows start and end off the canvas on purpose, so look at what goes on top of it.
        for seed in range(25):
            cmds = self.painting(seed)
            top = next(i for i, c in enumerate(cmds) if isinstance(c, str) and c.startswith("# Paint marks"))
            for c in _body_strokes(cmds[top:]):
                for x, y, *_ in c["points"]:
                    self.assertTrue(-1 <= x <= self.W + 1 and -1 <= y <= self.H + 1, (seed, x, y))

    def test_every_command_is_well_formed(self):
        for seed in range(10):
            for c in _commands(self.painting(seed)):
                if c["command"] == "set_brush":
                    self.assertIn("preset", c)
                    self.assertTrue(0.5 <= c["size"] <= 200, c)
                if c["command"] == "set_color":
                    self.assertRegex(c["hex"], r"^#[0-9A-Fa-f]{6}$")
                if c["command"] == "stroke":
                    self.assertGreaterEqual(len(c["points"]), 2)
                    self.assertTrue(all(math.isfinite(v) for p in c["points"] for v in p))

    def test_works_on_other_canvas_sizes(self):
        for w, h in ((600, 600), (1600, 600), (500, 900)):
            self.assertTrue(self.painting(1, w, h))

    def test_make_functions_send_the_values_to_the_objects(self):
        comp = FakeComposition()
        rng = random.Random(4)
        shape = makers.make_shape(rng, comp, PALETTE, WEIGHTS, STUB_BRUSHES)
        self.assertTrue(shape[0].startswith("# Shape ("))
        group = makers.make_cluster(rng, comp, PALETTE, WEIGHTS, STUB_BRUSHES, chance=1.0)
        self.assertTrue(group[0].startswith("# Cluster ("))
        self.assertEqual(makers.make_cluster(rng, comp, PALETTE, WEIGHTS, STUB_BRUSHES, chance=0.0), [])
        mark = makers.make_paint_mark(rng, comp, PALETTE, WEIGHTS, STUB_BRUSHES, style="fan")
        self.assertTrue(mark[0].startswith("# Paint mark (fan, "))
        self.assertEqual(makers.make_paint_mark(rng, comp, PALETTE, WEIGHTS, STUB_BRUSHES, style="fan", point=comp.points[0])[0][:19],
                         "# Paint mark (fan, ")


# ---------- the second conductor ----------

class TestAbstract2(unittest.TestCase):
    W, H = 1200, 900

    # The conductor's switches are Kevin's to edit while experimenting, so the tests pin them to
    # known values and put his back afterwards.
    PINNED = {"PLANT_COUNT": 12, "PLANT_SIZE": 0.3}

    def setUp(self):
        self.saved = {name: getattr(abstract2, name) for name in self.PINNED}
        for name, value in self.PINNED.items():
            setattr(abstract2, name, value)

    def tearDown(self):
        for name, value in self.saved.items():
            setattr(abstract2, name, value)

    def painting(self, seed, width=None, height=None):
        return abstract2.generate({"name": "test", "colors": PALETTE}, seed, width or self.W, height or self.H,
                                  "x.png", "x.oxide", STUB_BRUSHES)

    def plants(self, cmds):
        """Each plant's own commands (from its "# Plant at" line to its dry)."""
        starts = [i for i, c in enumerate(cmds) if isinstance(c, str) and c.startswith("# Plant at (")]
        return [cmds[i + 1:cmds.index({"command": "dry"}, i)] for i in starts]

    def test_same_seed_same_painting(self):
        self.assertEqual(self.painting(8), self.painting(8))
        self.assertNotEqual(self.painting(8), self.painting(9))

    def test_the_background_then_plant_count_plants_then_export_and_save(self):
        cmds = self.painting(4)
        self.assertEqual(cmds[0], "# Abstract2: palette test, seed 4, 12 plants")
        layer = cmds.index({"command": "add_layer", "name": "Plants"})
        self.assertEqual(cmds[layer - 1], "# Plants (12)")
        self.assertTrue(any(c["command"] == "stroke" for c in _commands(cmds[:layer])), "a background first")
        self.assertEqual(len(self.plants(cmds)), 12)
        self.assertEqual([c["command"] for c in _commands(cmds[-2:])], ["export", "save"])
        abstract2.PLANT_COUNT = 3
        self.assertEqual(len(self.plants(self.painting(4))), 3)

    def test_each_plant_is_plant1_as_recorded_at_one_size(self):
        cmds = self.painting(5)
        scale = abstract2.PLANT_SIZE * min(self.W, self.H) / 2 / abstract2.plant1.reach()
        for plant in self.plants(cmds):
            x0, y0, x1, y1 = recordings.bounds(plant)
            x, y = (x0 + x1) / 2, (y0 + y1) / 2
            self.assertEqual(plant, abstract2.plant1.plant1(x=round(x, 1), y=round(y, 1), scale=scale))

    def test_the_plants_go_anywhere_and_stay_on_the_canvas(self):
        middles = []
        for seed in range(25):
            for plant in self.plants(self.painting(seed)):
                x0, y0, x1, y1 = recordings.bounds(plant)
                self.assertTrue(0 <= x0 and x1 <= self.W and 0 <= y0 and y1 <= self.H, (seed, x0, y0, x1, y1))
                middles.append(((x0 + x1) / 2, (y0 + y1) / 2))
        # Spread over the canvas: plants turn up in every quarter of it.
        quarters = {(x > self.W / 2, y > self.H / 2) for x, y in middles}
        self.assertEqual(len(quarters), 4)

    def test_the_fall_trees_get_their_own_size_and_stay_on_the_canvas(self):
        # Each recording is sized from its own reach (falltree1 is much smaller than plant1; with
        # plant1's scale it came out about 40 px across, as a thick lump).
        scale = abstract2.PLANT_SIZE * min(self.W, self.H) / 2 / abstract2.falltree1.reach()
        for seed in range(10):
            cmds = self.painting(seed)
            starts = [i for i, c in enumerate(cmds) if isinstance(c, str) and c.startswith("# falltree at (")]
            self.assertEqual(len(starts), abstract2.PLANT_COUNT)
            for i in starts:
                tree = cmds[i + 1:cmds.index({"command": "dry"}, i)]
                x0, y0, x1, y1 = recordings.bounds(tree)
                self.assertTrue(0 <= x0 and x1 <= self.W and 0 <= y0 and y1 <= self.H, (seed, x0, y0, x1, y1))
                self.assertEqual(tree, abstract2.falltree1.falltree1(x=round((x0 + x1) / 2, 1), y=round((y0 + y1) / 2, 1), scale=scale))

    def test_a_plant_bigger_than_the_canvas_goes_in_the_middle(self):
        abstract2.PLANT_SIZE = 3
        for plant in self.plants(self.painting(1, 400, 300)):
            x0, y0, x1, y1 = recordings.bounds(plant)
            self.assertAlmostEqual((x0 + x1) / 2, 200, delta=1)
            self.assertAlmostEqual((y0 + y1) / 2, 150, delta=1)

    def test_the_command_line_lists_palettes(self):
        for script in ("abstract.py", "abstract2.py"):
            out = subprocess.run([sys.executable, str(Path(__file__).resolve().parent / script), "--list-palettes"],
                                 capture_output=True, text=True)
            self.assertEqual(out.returncode, 0, script)
            self.assertEqual(len(out.stdout.splitlines()), len(palettes.load_palettes()), script)


class TestAbstract4(unittest.TestCase):
    W, H = 1200, 900

    # The conductor's switches are Kevin's to edit while experimenting, so the tests pin them.
    PINNED = {"MARK_COUNT": 8, "MARK_SIZE": 0.5}

    def setUp(self):
        self.saved = {name: getattr(abstract4, name) for name in self.PINNED}
        for name, value in self.PINNED.items():
            setattr(abstract4, name, value)

    def tearDown(self):
        for name, value in self.saved.items():
            setattr(abstract4, name, value)

    def painting(self, seed, width=None, height=None):
        return abstract4.generate({"name": "test", "colors": PALETTE}, seed, width or self.W, height or self.H,
                                  "x.png", "x.oxide", STUB_BRUSHES)

    def marks(self, cmds):
        """Each mark's own commands (from its "# Mark at" line to its dry)."""
        starts = [i for i, c in enumerate(cmds) if isinstance(c, str) and c.startswith("# Mark at (")]
        return [cmds[i + 1:cmds.index({"command": "dry"}, i)] for i in starts]

    def test_same_seed_same_painting(self):
        self.assertEqual(self.painting(8), self.painting(8))
        self.assertNotEqual(self.painting(8), self.painting(9))

    def test_the_comment_names_each_marks_shade(self):
        scale = abstract4.MARK_SIZE * min(self.W, self.H) / 2 / abstract4.mark1.reach()
        cmds = self.painting(3)
        for i, c in enumerate(cmds):
            if isinstance(c, str) and c.startswith("# Mark at ("):
                shade = c.rsplit(" ", 1)[1]
                self.assertIn(shade, PALETTE)
                mark = cmds[i + 1:cmds.index({"command": "dry"}, i)]
                x0, y0, x1, y1 = recordings.bounds(mark)
                self.assertEqual(mark, abstract4.mark1.mark1(x=round((x0 + x1) / 2, 1), y=round((y0 + y1) / 2, 1), scale=scale, shade=shade))

    def test_the_background_then_mark_count_marks_then_export_and_save(self):
        cmds = self.painting(4)
        self.assertEqual(cmds[0], "# Abstract4: palette test, seed 4, 8 marks")
        layer = cmds.index({"command": "add_layer", "name": "Marks"})
        self.assertTrue(any(c["command"] == "stroke" for c in _commands(cmds[:layer])), "a background first")
        self.assertEqual(len(self.marks(cmds)), 8)
        self.assertEqual([c["command"] for c in _commands(cmds[-2:])], ["export", "save"])
        abstract4.MARK_COUNT = 3
        self.assertEqual(len(self.marks(self.painting(4))), 3)

    def test_each_mark_is_mark1_in_shades_of_a_palette_color_at_one_size_on_the_canvas(self):
        scale = abstract4.MARK_SIZE * min(self.W, self.H) / 2 / abstract4.mark1.reach()
        for seed in range(10):
            for mark in self.marks(self.painting(seed)):
                x0, y0, x1, y1 = recordings.bounds(mark)
                self.assertTrue(0 <= x0 and x1 <= self.W and 0 <= y0 and y1 <= self.H, (seed, x0, y0, x1, y1))
                self.assertIn(mark, [abstract4.mark1.mark1(x=round((x0 + x1) / 2, 1), y=round((y0 + y1) / 2, 1), scale=scale, shade=shade)
                                     for shade in PALETTE])

    def test_the_command_line_lists_palettes(self):
        out = subprocess.run([sys.executable, str(Path(__file__).resolve().parent / "abstract4.py"), "--list-palettes"],
                             capture_output=True, text=True)
        self.assertEqual(out.returncode, 0)
        self.assertEqual(len(out.stdout.splitlines()), len(palettes.load_palettes()))


# ---------- against the real app (only with --app) ----------

class TestAbstract3(unittest.TestCase):
    W, H = 1200, 900

    def setUp(self):
        self.saved = {name: getattr(abstract3, name) for name in ("TREE_COUNT", "HORIZON", "BRUSH_TUNING")}
        abstract3.TREE_COUNT, abstract3.HORIZON = (8, 30), (0.4, 0.6)

    def tearDown(self):
        for name, value in self.saved.items():
            setattr(abstract3, name, value)

    def painting(self, seed, palette=None):
        return abstract3.generate({"name": "test", "colors": palette or PALETTE}, seed, self.W, self.H, "x.png", "x.oxide", STUB_BRUSHES)

    def trees(self, cmds):
        return [c for c in cmds if isinstance(c, str) and c.startswith("# Tree (")]

    def test_same_seed_same_painting(self):
        self.assertEqual(self.painting(4), self.painting(4))
        self.assertNotEqual(self.painting(4), self.painting(5))

    def test_the_number_of_trees_is_random_in_range_and_the_header_says_so(self):
        seen = set()
        for seed in range(60):
            cmds = self.painting(seed)
            n = len(self.trees(cmds))
            low, high = abstract3.TREE_COUNT
            self.assertTrue(low <= n <= high, seed)
            self.assertIn(f"{n} trees", cmds[0])
            self.assertIn(f"# Trees ({n})", cmds)
            seen.add(n)
        self.assertGreater(len(seen), 10)

    def test_sky_then_ground_then_trees_then_export_and_save(self):
        cmds = self.painting(2)
        layers = [c["name"] for c in _commands(cmds) if c["command"] == "add_layer"]
        self.assertEqual(layers, ["Ground", "Trees"])
        names = [c for c in cmds if isinstance(c, str)]
        self.assertLess(names.index(next(n for n in names if n.startswith("# Background"))), names.index(next(n for n in names if n.startswith("# Ground"))))
        self.assertLess(names.index(next(n for n in names if n.startswith("# Ground"))), names.index(next(n for n in names if n.startswith("# Trees ("))))
        last = _commands(cmds)[-2:]
        self.assertEqual([c["command"] for c in last], ["export", "save"])

    def test_the_ground_is_not_the_sky_color(self):
        for seed in range(20):
            cmds = _commands(self.painting(seed))
            sky = next(c["color"] for c in cmds if c["command"] == "set_paper")
            ground_color = next(c["hex"] for i, c in enumerate(cmds) if c["command"] == "set_color" and
                                any(d["command"] == "add_layer" and d["name"] == "Ground" for d in cmds[:i]))
            self.assertNotEqual(ground_color, sky, seed)

    def test_far_trees_are_hazed_toward_the_sky_and_come_first(self):
        # With a black and white palette, a far tree's colors are grey (mixed toward the sky), the nearest are black or white.
        cmds = self.painting(9, ["#000000", "#FFFFFF"])
        colors = [c["hex"] for c in _commands(cmds) if c["command"] == "set_color"]
        tree_part = colors[colors.index(next(c for c in colors)):]
        self.assertTrue(any(h not in ("#000000", "#FFFFFF") for h in tree_part))

    def test_everything_is_well_formed(self):
        for c in _commands(self.painting(6)):
            self.assertIn("command", c)

    def test_works_on_other_canvas_sizes(self):
        cmds = abstract3.generate({"name": "t", "colors": PALETTE}, 3, 600, 800, "x.png", "x.oxide", STUB_BRUSHES)
        self.assertEqual(_commands(cmds)[0], {"command": "new_painting", "width": 600, "height": 800})


@unittest.skipUnless(APP, "pass --app to test against the real app (needs cargo)")
class TestAgainstTheApp(unittest.TestCase):
    """Sends real commands through oxide-run and checks that the app accepts every one."""

    @classmethod
    def setUpClass(cls):
        cls.brush_list = brushes.list_brushes()

    def run_commands(self, items, width=900, height=700):
        lines = [json.dumps({"command": "new_painting", "width": width, "height": height})]
        lines += [i if isinstance(i, str) else json.dumps(i) for i in items]
        result = subprocess.run(["cargo", "run", "-q", "--release", "-p", "oxide-agent", "--bin", "oxide-run"],
                                input="\n".join(lines) + "\n", cwd=brushes.REPO, capture_output=True, text=True)
        failed = [line for line in result.stdout.splitlines() if '"ok":false' in line]
        self.assertEqual(failed, [], failed[:2])

    def test_the_starting_brushes_are_oxide_runs_defaults(self):
        # A fresh session, per tool: `info` shows the brush, which must match START_BRUSHES.
        for tool, brush in recordings.START_BRUSHES.items():
            lines = json.dumps({"command": "set_tool", "tool": tool}) + "\n" + json.dumps({"command": "info"}) + "\n"
            result = subprocess.run(["cargo", "run", "-q", "--release", "-p", "oxide-agent", "--bin", "oxide-run"],
                                    input=lines, cwd=brushes.REPO, capture_output=True, text=True)
            info = json.loads(result.stdout.splitlines()[-1])
            info = info.get("result", info)
            got = info["brush"]
            for key in ("size", "opacity", "hardness", "spacing", "grain", "loading", "oiliness", "length", "thickness",
                        "roundness", "roughness", "streaks", "lift_off", "taper"):
                self.assertAlmostEqual(got[key], brush[key], places=4, msg=f"{tool} {key}")

    def test_a_recording_with_another_brush(self):
        # plant1 as recorded, then with every brush in the app's list in turn (a few strokes of it).
        from recorded import plant1
        cmds = plant1.plant1(x=450, y=350, scale=0.2)
        strokes = [i for i, c in enumerate(cmds) if isinstance(c, dict) and c["command"] == "stroke"]
        short = cmds[:strokes[3] + 1]
        for brush in self.brush_list[::6]:
            cmds += recordings.transform(short, plant1.ORIGIN, x=450, y=350, scale=0.2, brush=brush)
        self.run_commands(cmds)

    def test_the_brush_list_is_real(self):
        self.assertGreaterEqual(len(self.brush_list), 30)
        self.assertTrue({"Round Hard", "Bristle Flat"} <= {b["name"] for b in self.brush_list})

    def test_shapes(self):
        by_name = {b["name"]: b for b in self.brush_list}
        cmds = []
        for kind in ("circle", "square", "triangle"):
            cmds += getattr(shapes, kind)(by_name["Round Hard"], "#C2151B", 400, 300, 200, fill=True, rotation=20, tool="oils", brush_size=10)
        self.run_commands(cmds)

    def test_every_tool_works_for_a_shape(self):
        by_name = {b["name"]: b for b in self.brush_list}
        cmds = []
        for tool in randomizers.TOOL_WEIGHTS:
            cmds += shapes.circle(by_name["Round Hard"], "#2021A0", 400, 300, 150, fill=True, tool=tool, brush_size=8)
        self.run_commands(cmds)

    def test_a_cluster_with_random_and_with_exact_marks(self):
        by_name = {b["name"]: b for b in self.brush_list}
        args = randomizers.cluster_args(random.Random(2), PALETTE, WEIGHTS, self.brush_list, 700)
        self.run_commands(cluster.Cluster(x=400, y=300, **args).commands())
        self.run_commands(cluster.Cluster(by_name["Round Hard"], "#1F2A44", 400, 300, 250, _ring(12), brush_size=9).commands())

    def test_every_style_with_several_brushes(self):
        rng = random.Random(3)
        cmds = []
        for brush in rng.sample(self.brush_list, 6):
            for name, style in paint_styles.STYLES.items():
                cmds += style(brush, "#D8403A", 450, 350, 300, rotation=30, seed=1)
        self.run_commands(cmds)

    def test_random_styles_with_random_paint(self):
        rng = random.Random(4)
        cmds = []
        for _ in range(40):
            a = randomizers.style_args(rng, PALETTE, WEIGHTS, self.brush_list, 700)
            cmds += paint_styles.STYLES[a["name"]](a["brush"], a["color"], 450, 350, a["size"], rotation=a["rotation"],
                                                   brush_size=a["brush_size"], paint=a["paint"], seed=a["seed"], **a["options"])
        self.run_commands(cmds)

    def test_the_background_with_every_surface(self):
        by_name = {b["name"]: b for b in self.brush_list}
        for kind in randomizers.SURFACES:
            self.run_commands(background.background("#EFE6D2", 600, 400, by_name["Bristle Flat"], seed=1, kind=kind, depth=0.5), 600, 400)

    def test_run_script_reports_success_and_failure(self):
        good = [{"command": "new_painting", "width": 200, "height": 150}, {"command": "set_color", "hex": "#C2151B"},
                {"command": "stroke", "points": [[20, 20], [150, 100]]}]
        bad = [{"command": "new_painting", "width": 200, "height": 150}, {"command": "no_such_command"}]
        with tempfile.TemporaryDirectory() as folder:
            for name, items, expected in (("good.jsonl", good, True), ("bad.jsonl", bad, False)):
                path = Path(folder) / name
                commands.write_script(items, path)
                with contextlib.redirect_stderr(io.StringIO()):  # run_script prints what failed; keep the test output quiet
                    self.assertEqual(commands.run_script(path.resolve()), expected, name)

    def test_whole_paintings_from_the_second_conductor(self):
        pal = palettes.load_palette("swan")
        cmds = []
        for seed in range(8):
            cmds += abstract2.generate(pal, seed, 900, 700, "unused.png", "unused.oxide", self.brush_list)[:-2]  # without the export and save
        self.run_commands(cmds)

    def test_the_command_line_writes_a_script_named_for_its_conductor(self):
        here = Path(__file__).resolve().parent
        with tempfile.TemporaryDirectory() as folder:
            for script, prefix in (("abstract.py", "abstract-"), ("abstract2.py", "abstract2-")):
                result = subprocess.run([sys.executable, str(here / script), "--palette", "swan", "--seed", "1", "--out", folder],
                                        capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stderr)
                written = Path(folder) / f"{prefix}swan-1.jsonl"
                self.assertTrue(written.exists(), written)
                self.assertTrue(written.read_text().startswith("# Abstract"), script)

    def test_whole_paintings(self):
        pal = palettes.load_palette("swan")
        cmds = []
        for seed in range(12):
            cmds += abstract.generate(pal, seed, 900, 700, "unused.png", "unused.oxide", self.brush_list)[:-2]  # without the export and save
        self.run_commands(cmds)


if __name__ == "__main__":
    unittest.main()
