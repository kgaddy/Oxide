use super::*;
use crate::{Brush, DabMode, PaintMode, Stroke};

const RED: Color = Color::rgb(1.0, 0.0, 0.0);

fn paint_dot(doc: &mut Document, x: f32, y: f32, color: Color) {
    doc.begin_stroke();
    let brush = Brush { color, ..Brush::default() };
    Stroke::begin(doc.active_canvas(), &brush, DabMode::Paint, x, y);
    doc.end_stroke();
}

#[test]
fn new_document_shows_paper() {
    let mut doc = Document::new(8, 8, Color::WHITE);
    assert_eq!(doc.composite_pixel(3, 3), Some(Color::WHITE));
    assert_eq!(doc.take_dirty(), Some(doc.bounds()));
    assert_eq!(doc.take_dirty(), None);
}

#[test]
fn average_sample_mixes_neighbors() {
    let mut doc = Document::new(20, 20, Color::WHITE);
    // Black on the left half, white paper on the right.
    doc.active_canvas().set_pixels(
        (0..20 * 20).map(|i| if i % 20 < 10 { [0.0, 0.0, 0.0, 1.0] } else { [0.0; 4] }).collect(),
        None,
    );
    assert_eq!(doc.composite_average(9, 5, 1), doc.composite_pixel(9, 5), "size 1 is one pixel");
    // 3x3 on the last black column (x = 9) covers x = 8..=10: 2 black, 1 white.
    let c = doc.composite_average(9, 5, 3).unwrap();
    assert!((c.r - 1.0 / 3.0).abs() < 1e-5, "2/3 black + 1/3 white, got {}", c.r);
    // At the corner the square is cut off, not padded.
    assert_eq!(doc.composite_average(0, 0, 5), Some(Color::BLACK));
    assert_eq!(doc.composite_average(20, 0, 3), None, "outside the painting");
}

#[test]
fn hidden_layer_does_not_show() {
    let mut doc = Document::new(20, 20, Color::WHITE);
    paint_dot(&mut doc, 10.0, 10.0, Color::BLACK);
    assert_eq!(doc.composite_pixel(10, 10), Some(Color::BLACK));
    doc.toggle_visible(0);
    assert_eq!(doc.composite_pixel(10, 10), Some(Color::WHITE));
}

#[test]
fn top_layer_covers_bottom_layer() {
    let mut doc = Document::new(20, 20, Color::WHITE);
    paint_dot(&mut doc, 10.0, 10.0, Color::BLACK);
    doc.add_layer();
    paint_dot(&mut doc, 10.0, 10.0, RED);
    assert_eq!(doc.composite_pixel(10, 10), Some(RED));

    // Move red below black: black wins.
    doc.move_active_layer(-1);
    assert_eq!(doc.composite_pixel(10, 10), Some(Color::BLACK));
}

#[test]
fn erasing_reveals_paper() {
    let mut doc = Document::new(20, 20, Color::WHITE);
    let brush = Brush::default();
    doc.active_canvas().dab(10.0, 10.0, &brush, DabMode::Paint);
    doc.active_canvas().dab(10.0, 10.0, &brush, DabMode::Erase);
    assert_eq!(doc.composite_pixel(10, 10), Some(Color::WHITE));
}

#[test]
fn cannot_delete_last_layer() {
    let mut doc = Document::new(4, 4, Color::WHITE);
    doc.delete_active_layer();
    assert_eq!(doc.layers().len(), 1);
}

#[test]
fn undo_and_redo_a_stroke() {
    let mut doc = Document::new(20, 20, Color::WHITE);
    paint_dot(&mut doc, 10.0, 10.0, Color::BLACK);
    assert_eq!(doc.undo_description(), Some("Paint"));

    doc.undo();
    assert_eq!(doc.composite_pixel(10, 10), Some(Color::WHITE));
    assert_eq!(doc.undo_description(), None);

    doc.redo();
    assert_eq!(doc.composite_pixel(10, 10), Some(Color::BLACK));
}

#[test]
fn undo_steps_back_one_stroke_at_a_time() {
    let mut doc = Document::new(30, 20, Color::WHITE);
    paint_dot(&mut doc, 5.0, 10.0, Color::BLACK);
    paint_dot(&mut doc, 25.0, 10.0, RED);
    doc.undo();
    assert_eq!(doc.composite_pixel(25, 10), Some(Color::WHITE));
    assert_eq!(doc.composite_pixel(5, 10), Some(Color::BLACK));
}

#[test]
fn new_edit_clears_redo() {
    let mut doc = Document::new(20, 20, Color::WHITE);
    paint_dot(&mut doc, 10.0, 10.0, Color::BLACK);
    doc.undo();
    paint_dot(&mut doc, 5.0, 5.0, RED);
    assert_eq!(doc.redo_description(), None);
}

#[test]
fn undo_layer_delete_brings_it_back_with_its_paint() {
    let mut doc = Document::new(20, 20, Color::WHITE);
    doc.add_layer();
    paint_dot(&mut doc, 10.0, 10.0, RED);
    doc.delete_active_layer();
    assert_eq!(doc.layers().len(), 1);
    assert_eq!(doc.composite_pixel(10, 10), Some(Color::WHITE));

    doc.undo();
    assert_eq!(doc.layers().len(), 2);
    assert_eq!(doc.composite_pixel(10, 10), Some(RED));

    doc.redo();
    assert_eq!(doc.layers().len(), 1);
}

#[test]
fn undo_add_move_rename_visibility() {
    let mut doc = Document::new(10, 10, Color::WHITE);
    doc.add_layer();
    doc.rename_layer(1, "Ink".into());
    doc.move_active_layer(-1);
    doc.toggle_visible(0);
    assert_eq!(doc.layers()[0].name, "Ink");
    assert!(!doc.layers()[0].visible);

    doc.undo(); // visibility
    assert!(doc.layers()[0].visible);
    doc.undo(); // move
    assert_eq!(doc.layers()[1].name, "Ink");
    doc.undo(); // rename
    assert_eq!(doc.layers()[1].name, "Layer 2");
    doc.undo(); // add
    assert_eq!(doc.layers().len(), 1);
}

#[test]
fn slider_drag_is_one_undo_step() {
    let mut doc = Document::new(10, 10, Color::WHITE);
    for step in 1..=10 {
        doc.set_opacity(0, 1.0 - step as f32 * 0.05);
    }
    doc.seal_history();
    assert_eq!(doc.layers()[0].opacity, 0.5);
    doc.undo();
    assert_eq!(doc.layers()[0].opacity, 1.0);
    assert_eq!(doc.undo_description(), None);
}

fn paint_wet_dot(doc: &mut Document, x: f32, y: f32) {
    doc.begin_stroke();
    let oily = Brush { oiliness: 0.5, loading: 0.8, ..Brush::default() };
    let mut stroke = Stroke::begin(doc.active_canvas(), &oily, DabMode::Paint, x, y);
    stroke.finish(doc.active_canvas(), &oily);
    doc.end_stroke();
}

#[test]
fn dry_layer_locks_paint_and_can_be_undone() {
    let mut doc = Document::new(20, 20, Color::WHITE);
    paint_wet_dot(&mut doc, 10.0, 10.0);
    assert!(doc.layers()[0].canvas.has_wet_paint());
    let looks = doc.composite_pixel(10, 10).unwrap();

    doc.dry_layer();
    assert!(!doc.layers()[0].canvas.has_wet_paint());
    let after = doc.composite_pixel(10, 10).unwrap();
    assert!((after.r - looks.r).abs() < 1e-5, "drying must not change how it looks: {looks:?} -> {after:?}");
    assert_eq!(doc.undo_description(), Some("Dry layer"));

    doc.undo();
    assert!(doc.layers()[0].canvas.has_wet_paint());
    doc.redo();
    assert!(!doc.layers()[0].canvas.has_wet_paint());

    doc.rewet_layer();
    assert!(doc.layers()[0].canvas.has_wet_paint());
}

#[test]
fn undoing_a_stroke_restores_wetness() {
    let mut doc = Document::new(20, 20, Color::WHITE);
    paint_wet_dot(&mut doc, 10.0, 10.0);
    doc.undo();
    assert!(!doc.layers()[0].canvas.has_wet_paint());
    doc.redo();
    assert!(doc.layers()[0].canvas.has_wet_paint());
}

#[test]
fn drying_already_dry_paint_is_not_an_undo_step() {
    let mut doc = Document::new(20, 20, Color::WHITE);
    paint_dot(&mut doc, 10.0, 10.0, Color::BLACK); // dry medium
    doc.dry_all();
    assert_eq!(doc.undo_description(), Some("Paint"));
}

#[test]
fn surface_change_is_undoable_and_lights_the_paper() {
    use crate::PaperKind;
    let mut doc = Document::new(64, 64, Color::WHITE);
    let flat = doc.composite_rgba8(doc.bounds());
    assert!(flat.iter().all(|&b| b == 255), "smooth paper is plain white");

    doc.set_surface(PaperSettings { kind: PaperKind::Canvas, depth: 0.6, ..PaperSettings::default() });
    doc.seal_history();
    let textured = doc.composite_rgba8(doc.bounds());
    assert!(textured.iter().any(|&b| b < 240), "canvas weave should show as light and shadow");
    assert_eq!(doc.undo_description(), Some("Paper texture"));

    doc.undo();
    assert_eq!(doc.surface().settings(), PaperSettings::flat());
}

#[test]
fn grainy_brush_skips_valleys_on_rough_paper() {
    use crate::{DabStyle, PaperKind, Wetness};
    let rough = PaperSettings { kind: PaperKind::Rough, depth: 1.0, ..PaperSettings::default() };
    let mut doc = Document::with_surface(128, 128, Color::WHITE, rough);
    let pencil = Brush { radius: 60.0, hardness: 1.0, grain: 1.0, ..Brush::default() };
    let light_touch =
        DabStyle { color: Color::BLACK, opacity: 1.0, mode: DabMode::Paint, wetness: Wetness::Dry, texture: None, reach: 0.4, pigment: None, relief: None, bands: None };
    doc.active_canvas().dab_with(64.0, 64.0, &pencil, &light_touch);
    let covered = (0..128 * 128)
        .filter(|&i| doc.layers()[0].canvas.dry_pixels()[i][3] > 0.5)
        .count();
    // Some of the area gets paint (peaks), some doesn't (valleys).
    let disc = (std::f32::consts::PI * 60.0 * 60.0) as usize;
    assert!(covered > disc / 10 && covered < disc * 9 / 10, "covered {covered} of ~{disc}");
}

#[test]
fn export_without_paper_is_transparent_where_unpainted() {
    let mut doc = Document::new(20, 20, Color::WHITE);
    paint_dot(&mut doc, 10.0, 10.0, Color::rgb(1.0, 0.0, 0.0));
    let with_paper = doc.composite_rgba_f32(doc.bounds(), true);
    let without = doc.composite_rgba_f32(doc.bounds(), false);
    assert_eq!(with_paper[0], [1.0, 1.0, 1.0, 1.0], "corner shows the paper");
    assert_eq!(without[0][3], 0.0, "corner is transparent without paper");
    assert_eq!(without[10 * 20 + 10], [1.0, 0.0, 0.0, 1.0], "paint stays");
}

#[test]
fn duplicate_merge_and_clear_are_undoable() {
    let mut doc = Document::new(20, 20, Color::WHITE);
    paint_dot(&mut doc, 6.0, 10.0, Color::BLACK);
    doc.duplicate_active_layer();
    assert_eq!(doc.layers().len(), 2);
    assert_eq!(doc.layers()[1].name, "Layer 1 copy");
    assert_eq!(doc.undo_description(), Some("Duplicate layer"));

    doc.add_layer();
    paint_dot(&mut doc, 14.0, 10.0, RED);
    let looks = (doc.composite_pixel(6, 10), doc.composite_pixel(14, 10));
    assert!(doc.merge_down());
    assert_eq!(doc.layers().len(), 2);
    assert_eq!((doc.composite_pixel(6, 10), doc.composite_pixel(14, 10)), looks, "merging doesn't change the picture");
    doc.undo();
    assert_eq!(doc.layers().len(), 3, "undo brings the merged layer back");
    doc.redo();
    assert_eq!(doc.layers().len(), 2);

    assert!(doc.merge_visible());
    assert_eq!(doc.layers().len(), 1);
    assert_eq!((doc.composite_pixel(6, 10), doc.composite_pixel(14, 10)), looks);
    doc.undo();
    assert_eq!(doc.layers().len(), 2);

    assert!(doc.clear_active_layer());
    doc.undo();
    assert_eq!(doc.composite_pixel(14, 10), Some(RED), "clear is undoable");
}

#[test]
fn transform_previews_applies_and_undoes() {
    let mut doc = Document::new(40, 20, Color::WHITE);
    paint_dot(&mut doc, 10.0, 10.0, RED);
    assert_eq!(doc.begin_transform().map(|b| b.x0 < 10 && b.x1 > 10), Some(true));
    doc.preview_transform(&Transform { dx: 20.0, ..Transform::default() });
    assert_eq!(doc.composite_pixel(30, 10), Some(RED), "moved");
    assert!(doc.cancel_transform());
    assert_eq!(doc.composite_pixel(10, 10), Some(RED), "cancel puts it back");

    doc.begin_transform();
    doc.preview_transform(&Transform { dx: 20.0, ..Transform::default() });
    assert!(doc.apply_transform());
    assert_eq!(doc.undo_description(), Some("Transform"));
    doc.undo();
    assert_eq!(doc.composite_pixel(10, 10), Some(RED));
    assert_ne!(doc.composite_pixel(30, 10), Some(RED));

    // Undo during a transform cancels it rather than undoing the step before.
    doc.redo();
    doc.begin_transform();
    doc.preview_transform(&Transform { dy: 5.0, ..Transform::default() });
    doc.undo();
    assert!(!doc.is_transforming());
    assert_eq!(doc.composite_pixel(30, 10), Some(RED));

    doc.add_layer();
    assert!(doc.transform_layer(Transform::default(), None).is_err(), "empty layer");
}

#[test]
fn impasto_is_lit_undoable_and_moves_with_transform() {
    let mut doc = Document::new(80, 40, Color::WHITE);
    let oil = Brush { radius: 6.0, impasto: 1.0, loading: 1.0, wet_medium: true, color: RED, ..Brush::default() };
    doc.begin_stroke();
    let mut stroke = Stroke::begin(doc.active_canvas(), &oil, DabMode::Paint, 10.0, 20.0);
    stroke.line_to(doc.active_canvas(), &oil, 40.0, 20.0);
    stroke.finish(doc.active_canvas(), &oil);
    doc.end_stroke();
    assert!(doc.layers()[0].canvas.relief()[20 * 80 + 25] > 0.3);

    // Lit: the edge facing the top-left light is brighter than the far edge.
    let lit = doc.composite_rgba8(doc.bounds());
    let red_at = |bytes: &[u8], x: usize, y: usize| bytes[(y * 80 + x) * 4] as i32 + bytes[(y * 80 + x) * 4 + 1] as i32;
    // Rows 15 and 24 are the same distance from the stroke's center (y = 20).
    let (near, far) = (red_at(&lit, 25, 15), red_at(&lit, 25, 24));
    assert!(near > far + 10, "lit side {near} vs shadow side {far}");
    // Impasto depth 0: flat again.
    doc.set_surface(PaperSettings { impasto_depth: 0.0, ..doc.surface().settings() });
    let flat = doc.composite_rgba8(doc.bounds());
    assert_eq!(red_at(&flat, 25, 15), red_at(&flat, 25, 24));
    doc.undo();

    // Undoing the stroke removes the thickness too.
    doc.undo();
    assert!(doc.layers()[0].canvas.relief().iter().all(|&h| h == 0.0));
    doc.redo();
    assert!(doc.layers()[0].canvas.relief()[20 * 80 + 25] > 0.3);

    // Transform moves the relief with the paint.
    doc.transform_layer(Transform { dx: 30.0, ..Transform::default() }, None).unwrap();
    assert!(doc.layers()[0].canvas.relief()[20 * 80 + 55] > 0.3);
    assert_eq!(doc.layers()[0].canvas.relief()[20 * 80 + 15], 0.0);
}

#[test]
fn a_stroke_plows_through_wet_impasto_but_not_dried_impasto() {
    let oil = Brush { radius: 8.0, impasto: 1.0, loading: 1.0, wet_medium: true, ..Brush::default() };
    let crossing = Brush { impasto: 0.3, color: RED, ..oil.clone() };
    let paint = |doc: &mut Document, brush: &Brush, from: (f32, f32), to: (f32, f32)| {
        doc.begin_stroke();
        let mut stroke = Stroke::begin(doc.active_canvas(), brush, DabMode::Paint, from.0, from.1);
        stroke.line_to(doc.active_canvas(), brush, to.0, to.1);
        stroke.finish(doc.active_canvas(), brush);
        doc.end_stroke();
    };
    // A thick horizontal stroke, then a thinner vertical one across it.
    let setup = |dry_first: bool| {
        let mut doc = Document::new(100, 100, Color::WHITE);
        paint(&mut doc, &oil, (10.0, 50.0), (90.0, 50.0));
        if dry_first {
            doc.dry_layer();
        }
        let before: Vec<f32> = doc.layers()[0].canvas.relief().to_vec();
        paint(&mut doc, &crossing, (50.0, 10.0), (50.0, 90.0));
        (before, doc.layers()[0].canvas.relief().to_vec())
    };
    let at = |r: &[f32], x: usize, y: usize| r[y * 100 + x];

    let (before, after) = setup(false);
    assert!(at(&after, 50, 50) < at(&before, 50, 50) - 0.15, "wet: the crossing stroke scraped a furrow ({} → {})", at(&before, 50, 50), at(&after, 50, 50));
    let ridge = (40..47).map(|x| at(&after, x, 50) - at(&before, x, 50)).fold(f32::MIN, f32::max);
    assert!(ridge > 0.05, "wet: paint pushed up along the crossing stroke's side ({ridge})");

    let (before, after) = setup(true);
    assert!(at(&after, 50, 50) >= at(&before, 50, 50), "dried: nothing scraped away; new paint sits on top");
    assert!(at(&after, 50, 50) > at(&before, 50, 50) + 0.1, "dried: the new paint piles up on the old");
}

#[test]
fn palette_knife_spreads_flat_and_scrapes_only_wet_paint() {
    let knife = Brush {
        radius: 16.0,
        roundness: 0.15,
        hardness: 1.0,
        spacing: 0.03,
        rotation: crate::Rotation::FollowStroke,
        loading: 1.0,
        oiliness: 0.7,
        impasto: 0.9,
        wet_medium: true,
        knife: true,
        color: RED,
        ..Brush::default()
    };
    let stroke = |doc: &mut Document, brush: &Brush, from: (f32, f32), to: (f32, f32)| {
        doc.begin_stroke();
        let mut s = Stroke::begin(doc.active_canvas(), brush, DabMode::Paint, from.0, from.1);
        s.line_to(doc.active_canvas(), brush, to.0, to.1);
        s.finish(doc.active_canvas(), brush);
        doc.end_stroke();
    };
    // Spreading: a flat-topped band (the thickness hardly varies across it).
    let mut doc = Document::new(120, 60, Color::WHITE);
    stroke(&mut doc, &knife, (10.0, 30.0), (110.0, 30.0));
    let relief = |y: usize| doc.layers()[0].canvas.relief()[y * 120 + 60];
    let middle: Vec<f32> = (24..37).map(relief).collect();
    let (lo, hi) = middle.iter().fold((f32::MAX, 0.0f32), |(lo, hi), &h| (lo.min(h), hi.max(h)));
    assert!(lo > 0.6 && hi - lo < 0.05, "flat top: {middle:?}");
    // The paint squeezed out at the blade's edges stands a little higher.
    assert!(relief(22).max(relief(23)) > hi && relief(38).max(relief(37)) > hi, "edge ridges");

    // Scraping: wet paint comes off, dried paint stays.
    let scraper = Brush { loading: 0.0, paint_mode: PaintMode::Blend, ..knife.clone() };
    let paint = Brush { radius: 12.0, loading: 1.0, wet_medium: true, color: RED, ..Brush::default() };
    let mut doc = Document::new(120, 80, Color::WHITE);
    stroke(&mut doc, &paint, (10.0, 20.0), (110.0, 20.0));
    doc.dry_layer();
    stroke(&mut doc, &paint, (10.0, 60.0), (110.0, 60.0));
    stroke(&mut doc, &scraper, (60.0, 2.0), (60.0, 78.0));
    let alpha = |doc: &Document, y: usize| doc.layers()[0].canvas.pixel(60, y).unwrap().a;
    assert!(alpha(&doc, 60) < 0.5, "wet paint scraped off ({})", alpha(&doc, 60));
    assert!(alpha(&doc, 20) > 0.95, "dried paint stays ({})", alpha(&doc, 20));
}

#[test]
fn thick_paint_casts_a_shadow_away_from_the_light() {
    let mut doc = Document::new(100, 100, Color::WHITE);
    doc.set_surface(PaperSettings { light_height: 20.0, shadows: 1.0, ..PaperSettings::flat() });
    let blob = Brush { radius: 10.0, impasto: 1.0, loading: 1.0, wet_medium: true, color: RED, ..Brush::default() };
    doc.begin_stroke();
    let mut stroke = Stroke::begin(doc.active_canvas(), &blob, DabMode::Paint, 50.0, 50.0);
    stroke.finish(doc.active_canvas(), &blob);
    doc.end_stroke();
    // The light is at the top-left (225°): the shadow falls down-right, on bare paper.
    let paper = |doc: &Document, x: usize, y: usize| doc.composite_rgba8(Region::new(x, y, x + 1, y + 1))[1];
    let (lit_side, shadow_side) = (paper(&doc, 39, 39), paper(&doc, 62, 62));
    assert!(shadow_side + 15 < lit_side, "shadow {shadow_side} vs lit {lit_side}");
    doc.set_surface(PaperSettings { shadows: 0.0, ..doc.surface().settings() });
    assert_eq!(paper(&doc, 62, 62), paper(&doc, 39, 39), "no shadows at 0");
}

#[test]
fn each_layer_can_have_its_own_impasto_depth() {
    let mut doc = Document::new(80, 40, Color::WHITE);
    // Thin enough that stronger relief isn't already at the lighting's limits.
    let oil = Brush { radius: 6.0, impasto: 0.15, loading: 1.0, wet_medium: true, color: RED, ..Brush::default() };
    doc.begin_stroke();
    let mut stroke = Stroke::begin(doc.active_canvas(), &oil, DabMode::Paint, 10.0, 20.0);
    stroke.line_to(doc.active_canvas(), &oil, 70.0, 20.0);
    stroke.finish(doc.active_canvas(), &oil);
    doc.end_stroke();
    let edge = |doc: &Document| {
        let px = doc.composite_rgba8(doc.bounds());
        let at = |y: usize| px[(y * 80 + 40) * 4] as i32 + px[(y * 80 + 40) * 4 + 1] as i32;
        (at(15), at(24)) // lit edge, shadow edge (same distance from the middle)
    };
    let (lit, shade) = edge(&doc);
    assert!(lit > shade + 10, "lit with the painting's depth");

    // This layer: flat (depth 0), whatever the painting's setting.
    doc.set_layer_impasto(0, Some(0.0));
    assert_eq!(doc.layer_impasto(0), 0.0);
    let (lit0, shade0) = edge(&doc);
    assert_eq!(lit0, shade0, "a layer with impasto depth 0 looks flat");
    // Deeper than the painting: stronger relief.
    doc.set_layer_impasto(0, Some(1.0));
    let (lit1, shade1) = edge(&doc);
    assert!(lit1 - shade1 > lit - shade, "deeper layer, stronger relief");
    // Both slider moves were one undo step, back to the painting's depth.
    assert_eq!(doc.undo_description(), Some("Layer impasto depth"));
    doc.undo();
    assert_eq!(doc.layers()[0].impasto, None);
    assert_eq!(edge(&doc), (lit, shade));
}

#[test]
fn layer_groups_composite_as_one_picture_and_keep_together() {
    let mut doc = Document::new(20, 20, Color::WHITE);
    paint_dot(&mut doc, 10.0, 10.0, Color::BLACK); // layer 0
    doc.add_layer();
    paint_dot(&mut doc, 10.0, 10.0, RED); // layer 1, exactly over it
    let gid = doc.group_active_layer().unwrap();
    assert_eq!(doc.groups()[0].name, "Group 1");
    // Layer 0 moves up against the group: it joins it (no swap).
    doc.set_active(0);
    doc.move_active_layer(1);
    assert_eq!(doc.layers()[0].group, Some(gid));
    assert_eq!(doc.layers()[0].name, "Layer 1", "joined in place");

    // At 50%, the group fades as one picture: red over black, then half
    // of it — the black underneath doesn't show through the red.
    doc.set_group_opacity(gid, 0.6);
    doc.set_group_opacity(gid, 0.5);
    let p = doc.composite_pixel(10, 10).unwrap();
    assert!((p.r - 1.0).abs() < 0.02 && (p.g - 0.5).abs() < 0.02, "half red over white: {p:?}");
    assert_eq!(doc.undo_description(), Some("Group opacity"), "slider drags are one step");

    // Hiding the group hides its layers and blocks painting on them.
    doc.set_group_visible(gid, false);
    assert_eq!(doc.composite_pixel(10, 10), Some(Color::WHITE));
    assert!(!doc.can_paint_active());
    doc.undo();
    assert!(doc.can_paint_active());

    // A new layer added inside the group joins it; the top member moving up leaves it.
    doc.set_active(1);
    doc.add_layer();
    assert_eq!(doc.layers()[2].group, Some(gid));
    doc.move_active_layer(1);
    assert_eq!(doc.layers()[2].group, None, "left the group at its top edge");
    doc.undo();
    assert_eq!(doc.layers()[2].group, Some(gid));

    // Ungroup keeps the layers; undo brings the group back.
    doc.ungroup(gid);
    assert!(doc.groups().is_empty() && doc.layers().iter().all(|l| l.group.is_none()));
    doc.undo();
    assert_eq!(doc.layers()[0].group, Some(gid));

    // Saved and loaded.
    let mut bytes = std::io::Cursor::new(Vec::new());
    doc.save(&mut bytes).unwrap();
    let loaded = Document::load(std::io::Cursor::new(bytes.into_inner())).unwrap();
    assert_eq!(loaded.groups(), doc.groups());
    assert_eq!(loaded.layers()[1].group, Some(gid));
    assert_eq!(loaded.composite_pixel(10, 10), doc.composite_pixel(10, 10));
}

#[test]
fn blend_modes_combine_layers_and_survive_undo_files_merge_and_psd() {
    let gray = Color::rgb(0.5, 0.5, 0.5);
    let mut doc = Document::new(20, 20, Color::WHITE);
    paint_dot(&mut doc, 10.0, 10.0, RED); // layer 0
    doc.add_layer();
    paint_dot(&mut doc, 10.0, 10.0, gray); // layer 1 on top
    assert_eq!(doc.composite_pixel(10, 10), Some(gray), "normal: gray covers red");
    doc.set_layer_blend(1, BlendMode::Multiply);
    let p = doc.composite_pixel(10, 10).unwrap();
    assert!((p.r - 0.5).abs() < 0.01 && p.g < 0.01, "multiply: half-dark red, {p:?}");
    assert_eq!(doc.undo_description(), Some("Blend mode"));
    doc.undo();
    assert_eq!(doc.composite_pixel(10, 10), Some(gray));
    doc.redo();

    // Saved and loaded.
    let mut bytes = std::io::Cursor::new(Vec::new());
    doc.save(&mut bytes).unwrap();
    let loaded = Document::load(std::io::Cursor::new(bytes.into_inner())).unwrap();
    assert_eq!(loaded.layers()[1].blend, BlendMode::Multiply);

    // PSD carries Photoshop's code for it.
    let mut psd = std::io::Cursor::new(Vec::new());
    doc.write_psd(&mut psd, crate::PsdOptions { include_paper: false, sixteen_bit: false }).unwrap();
    assert!(psd.get_ref().windows(8).any(|w| w == b"8BIMmul "), "PSD blend key");

    // Merging down bakes the multiply in.
    assert!(doc.merge_down());
    let merged = doc.composite_pixel(10, 10).unwrap();
    assert!((merged.r - 0.5).abs() < 0.01 && merged.g < 0.01, "{merged:?}");
}

#[test]
fn painting_stays_inside_the_selection() {
    let mut doc = Document::new(40, 20, Color::WHITE);
    doc.select(&SelectShape::Rect { x0: 0.0, y0: 0.0, x1: 20.0, y1: 20.0 }, SelectMode::Replace);
    assert_eq!(doc.selection_bounds(), Some(Region::new(0, 0, 20, 20)));
    // A stroke across the whole width only paints the left half.
    let brush = Brush { radius: 5.0, color: RED, ..Brush::default() };
    doc.begin_stroke();
    let mut stroke = Stroke::begin(doc.active_canvas(), &brush, DabMode::Paint, 2.0, 10.0);
    stroke.line_to(doc.active_canvas(), &brush, 38.0, 10.0);
    doc.end_stroke();
    assert_eq!(doc.composite_pixel(10, 10), Some(RED));
    assert_eq!(doc.composite_pixel(30, 10), Some(Color::WHITE), "outside the selection: untouched");

    // Fill and clear work on the selection only.
    doc.invert_selection();
    assert!(doc.fill_selection(Color::BLACK));
    assert_eq!(doc.composite_pixel(30, 10), Some(Color::BLACK));
    assert_eq!(doc.composite_pixel(10, 10), Some(RED));
    assert_eq!(doc.undo_description(), Some("Fill selection"));
    doc.undo();
    assert_eq!(doc.composite_pixel(30, 10), Some(Color::WHITE));

    // Selection changes are undoable; without a selection, tools work everywhere.
    doc.undo(); // the invert
    assert_eq!(doc.selection_bounds(), Some(Region::new(0, 0, 20, 20)));
    doc.deselect();
    assert!(doc.selection().is_none());
    assert!(!doc.clear_selected(), "nothing to clear without a selection");
    doc.select(&SelectShape::Rect { x0: 5.0, y0: 5.0, x1: 15.0, y1: 15.0 }, SelectMode::Replace);
    assert!(doc.clear_selected());
    assert_eq!(doc.composite_pixel(10, 10), Some(Color::WHITE), "cleared");
    assert_eq!(doc.composite_pixel(2, 10), Some(RED), "outside kept");
}

#[test]
fn layer_masks_hide_without_erasing() {
    let mut doc = Document::new(40, 20, Color::WHITE);
    let wide = Brush { radius: 30.0, color: RED, ..Brush::default() };
    doc.begin_stroke();
    let _ = Stroke::begin(doc.active_canvas(), &wide, DabMode::Paint, 20.0, 10.0);
    doc.end_stroke();
    assert_eq!(doc.composite_pixel(30, 10), Some(RED));

    // A mask from a selection: only the left half shows.
    doc.select(&SelectShape::Rect { x0: 0.0, y0: 0.0, x1: 20.0, y1: 20.0 }, SelectMode::Replace);
    assert!(doc.add_mask());
    doc.deselect();
    assert_eq!(doc.composite_pixel(10, 10), Some(RED));
    assert_eq!(doc.composite_pixel(30, 10), Some(Color::WHITE), "hidden, not erased");
    assert!(doc.layers()[0].canvas.pixel(30, 10).unwrap().a > 0.99, "the paint is still there");

    // Turning the mask off shows everything; undo turns it back on.
    doc.set_mask_enabled(false);
    assert_eq!(doc.composite_pixel(30, 10), Some(RED));
    doc.undo();
    assert_eq!(doc.composite_pixel(30, 10), Some(Color::WHITE));

    // Painting white on the mask reveals; one undo step.
    let reveal = Brush { radius: 4.0, hardness: 1.0, ..Brush::default() };
    assert!(doc.begin_mask_stroke());
    doc.mask_line((28.0, 10.0), (32.0, 10.0), &reveal, 1.0);
    doc.end_mask_stroke();
    assert_eq!(doc.composite_pixel(30, 10), Some(RED), "revealed");
    assert_eq!(doc.undo_description(), Some("Paint on mask"));
    doc.undo();
    assert_eq!(doc.composite_pixel(30, 10), Some(Color::WHITE));

    // Saved and loaded with the mask.
    let mut bytes = std::io::Cursor::new(Vec::new());
    doc.save(&mut bytes).unwrap();
    let loaded = Document::load(std::io::Cursor::new(bytes.into_inner())).unwrap();
    assert_eq!(loaded.layers()[0].mask, doc.layers()[0].mask);

    // Exported layer pixels have the mask applied (PSD looks the same).
    assert_eq!(doc.layer_rgba_f32(0)[10 * 40 + 30][3], 0.0);

    // Apply: the hidden paint is erased and the mask goes away; undo restores both.
    doc.apply_mask();
    assert!(doc.layers()[0].mask.is_none());
    assert_eq!(doc.layers()[0].canvas.pixel(30, 10).unwrap().a, 0.0);
    doc.undo();
    assert!(doc.layers()[0].mask.is_some());
    assert!(doc.layers()[0].canvas.pixel(30, 10).unwrap().a > 0.99);

    // Merge down bakes the mask in.
    doc.add_layer();
    doc.layers.swap(0, 1); // (put the masked layer on top for this test)
    doc.set_active(1);
    assert!(doc.merge_down());
    assert!(doc.layers()[0].mask.is_none());
    assert_eq!(doc.composite_pixel(30, 10), Some(Color::WHITE));
    assert_eq!(doc.composite_pixel(10, 10), Some(RED));
}

#[test]
fn copy_paste_and_moving_just_the_selection() {
    let mut doc = Document::new(60, 20, Color::WHITE);
    paint_dot(&mut doc, 10.0, 10.0, RED);
    paint_dot(&mut doc, 40.0, 10.0, Color::BLACK);
    doc.select(&SelectShape::Rect { x0: 0.0, y0: 0.0, x1: 25.0, y1: 20.0 }, SelectMode::Replace);

    // Copy the red dot (only) and paste it on a new layer, same place.
    let clip = doc.copy_selected().unwrap();
    assert_eq!((clip.x, clip.w), (0, 25));
    doc.paste(&clip);
    assert_eq!(doc.layers().len(), 2);
    assert_eq!(doc.layers()[1].name, "Pasted");
    assert_eq!(doc.layers()[1].canvas.pixel(10, 10), Some(RED));
    assert_eq!(doc.layers()[1].canvas.pixel(40, 10).unwrap().a, 0.0, "the black dot wasn't selected");
    assert_eq!(doc.undo_description(), Some("Paste"));
    doc.undo();

    // Transform with a selection moves only the selected paint, and the selection with it.
    let bounds = doc.begin_transform().unwrap();
    assert_eq!(bounds, Region::new(0, 0, 25, 20), "the handles frame the selection");
    doc.preview_transform(&Transform { dx: 20.0, ..Transform::default() });
    assert!(doc.apply_transform());
    assert_eq!(doc.composite_pixel(30, 10), Some(RED), "moved");
    assert_eq!(doc.composite_pixel(10, 10), Some(Color::WHITE), "left nothing behind");
    assert_eq!(doc.composite_pixel(40, 10), Some(Color::BLACK), "the rest stayed");
    assert_eq!(doc.selection_bounds(), Some(Region::new(20, 0, 45, 20)), "the selection moved too");
    doc.undo();
    assert_eq!(doc.composite_pixel(10, 10), Some(RED));
    assert_eq!(doc.selection_bounds(), Some(Region::new(0, 0, 25, 20)));

    // Cut: copied and erased.
    let cut = doc.cut_selected().unwrap();
    assert_eq!(doc.composite_pixel(10, 10), Some(Color::WHITE));
    assert!(cut.rgba8().as_chunks::<4>().0.iter().any(|p| p == &[255, 0, 0, 255]), "the clipboard image has the red");

    // An image from the system clipboard is centered on a new layer.
    assert!(!doc.paste_image(3, 1, &[0, 0, 255, 255, 0, 0, 255, 255]), "wrong size: refused");
    assert!(doc.paste_image(2, 1, &[0, 0, 255, 255, 0, 0, 255, 255]));
    assert_eq!(doc.layers().last().unwrap().canvas.pixel(29, 9), Some(Color::rgb(0.0, 0.0, 1.0)));
}

#[test]
fn groups_pass_blend_modes_through_at_any_opacity() {
    let gray = Color::rgb(0.5, 0.5, 0.5);
    let mut doc = Document::new(20, 20, Color::WHITE);
    doc.add_layer();
    paint_dot(&mut doc, 10.0, 10.0, gray);
    doc.set_layer_blend(1, BlendMode::Multiply);
    let gid = doc.group_active_layer().unwrap();
    // Gray multiplied onto white paper: gray, at 100% and (almost) at 99%.
    assert_eq!(doc.composite_pixel(10, 10), Some(gray));
    doc.set_group_opacity(gid, 0.99);
    let p = doc.composite_pixel(10, 10).unwrap();
    assert!((p.r - 0.505).abs() < 0.002 && (p.g - 0.505).abs() < 0.002, "no jump at 99%: {p:?}");
    // Over red paint below the group, the multiply still darkens it when faded.
    doc.set_active(0);
    paint_dot(&mut doc, 10.0, 10.0, RED);
    doc.set_group_opacity(gid, 0.5);
    let p = doc.composite_pixel(10, 10).unwrap();
    assert!((p.r - 0.75).abs() < 0.02 && p.g < 0.02, "half of a multiply on red: {p:?}");
}

#[test]
fn merge_down_says_when_a_blend_mode_cant_survive() {
    let gray = Color::rgb(0.5, 0.5, 0.5);
    let wide = Brush { radius: 30.0, color: RED, ..Brush::default() };
    let mut doc = Document::new(20, 20, Color::WHITE);
    doc.add_layer();
    paint_dot(&mut doc, 10.0, 10.0, gray);
    assert!(!doc.merge_down_changes_look(), "Normal merges exactly");
    doc.set_layer_blend(1, BlendMode::Multiply);
    assert!(doc.merge_down_changes_look(), "multiply over an empty layer");
    // With the layer below fully painted, the merge looks the same.
    doc.set_active(0);
    doc.begin_stroke();
    let _ = Stroke::begin(doc.active_canvas(), &wide, DabMode::Paint, 10.0, 10.0);
    doc.end_stroke();
    doc.set_active(1);
    assert!(!doc.merge_down_changes_look());
}

#[test]
fn fill_and_clear_respect_lock_transparency() {
    let mut doc = Document::new(40, 20, Color::WHITE);
    paint_dot(&mut doc, 10.0, 10.0, RED);
    doc.set_lock_transparency(0, true);
    doc.select(&SelectShape::Rect { x0: 0.0, y0: 0.0, x1: 40.0, y1: 20.0 }, SelectMode::Replace);
    assert!(!doc.clear_selected(), "nothing may be erased");
    assert!(doc.cut_selected().is_none(), "cut can't erase either");
    assert_eq!(doc.composite_pixel(10, 10), Some(RED));
    // Fill recolors the paint that's there, and nothing else.
    let alpha = |doc: &Document, x| doc.layers()[0].canvas.pixel(x, 10).unwrap().a;
    let edge = (10..40).find(|&x| alpha(&doc, x) > 0.0 && alpha(&doc, x) < 0.99).expect("a soft edge");
    let before = alpha(&doc, edge);
    assert!(doc.fill_selection(Color::BLACK));
    assert_eq!(doc.composite_pixel(10, 10), Some(Color::BLACK));
    assert_eq!(doc.composite_pixel(30, 10), Some(Color::WHITE), "empty stays empty");
    assert!((alpha(&doc, edge) - before).abs() < 1e-5, "coverage unchanged");
}

#[test]
fn selection_undo_keeps_only_the_changed_box() {
    let mut doc = Document::new(200, 100, Color::WHITE);
    doc.select(&SelectShape::Rect { x0: 10.0, y0: 10.0, x1: 20.0, y1: 20.0 }, SelectMode::Replace);
    doc.select(&SelectShape::Rect { x0: 50.0, y0: 50.0, x1: 60.0, y1: 60.0 }, SelectMode::Add);
    doc.deselect();
    let Some(Edit::Selection { region, before, after }) = doc.history.pop_undo().map(|(_, e)| e) else { panic!("a selection step") };
    assert_eq!(region, Region::new(10, 10, 60, 60), "just the box, not the canvas");
    assert_eq!(before.map(|b| b.len()), Some(50 * 50));
    assert!(after.is_none());
    let mut doc = Document::new(200, 100, Color::WHITE);
    doc.select(&SelectShape::Rect { x0: 10.0, y0: 10.0, x1: 20.0, y1: 20.0 }, SelectMode::Replace);
    doc.select(&SelectShape::Rect { x0: 50.0, y0: 50.0, x1: 60.0, y1: 60.0 }, SelectMode::Replace);
    doc.deselect();
    // Undo and redo walk back and forth exactly.
    doc.undo();
    assert_eq!(doc.selection_bounds(), Some(Region::new(50, 50, 60, 60)));
    doc.undo();
    assert_eq!(doc.selection_bounds(), Some(Region::new(10, 10, 20, 20)));
    doc.undo();
    assert!(doc.selection().is_none());
    doc.redo();
    doc.redo();
    assert_eq!(doc.selection_bounds(), Some(Region::new(50, 50, 60, 60)));
    doc.redo();
    assert!(doc.selection().is_none());
}

#[test]
fn locks_and_tracing() {
    let mut doc = Document::new(20, 20, Color::WHITE);
    paint_dot(&mut doc, 10.0, 10.0, RED);
    doc.set_locked(0, true);
    assert!(!doc.can_paint_active());
    doc.undo();
    assert!(doc.can_paint_active());

    doc.set_tracing(0, true);
    assert_eq!(doc.tracing_layer(), Some(0));
    assert!(doc.layers()[0].locked && !doc.can_paint_active(), "tracing layers are locked");
    assert_eq!(doc.tracing_color(10.0, 10.0), Some(RED));
    assert_eq!(doc.tracing_color(1.0, 1.0), None, "no paint there");
    // Only one tracing layer at a time.
    doc.add_layer();
    doc.set_tracing(1, true);
    assert_eq!(doc.tracing_layer(), Some(1));
    assert!(!doc.layers()[0].tracing);
    doc.undo();
    assert_eq!(doc.tracing_layer(), Some(0));
}

#[test]
fn state_id_tracks_unsaved_changes() {
    let mut doc = Document::new(20, 20, Color::WHITE);
    let saved = doc.state_id();
    paint_dot(&mut doc, 10.0, 10.0, Color::BLACK);
    assert_ne!(doc.state_id(), saved);
    doc.undo();
    assert_eq!(doc.state_id(), saved);
    doc.redo();
    assert_ne!(doc.state_id(), saved);
}
