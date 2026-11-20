//! A hue ring with a saturation/value square inside, like most paint apps.
//! The ring can be laid out as a painter's **RYB** wheel (red–yellow–blue
//! primaries, so complements sit opposite as in paint: red–green,
//! yellow–purple, blue–orange) instead of the screen's RGB hues.

use eframe::egui::{self, Color32, Mesh, Pos2, Rect, Sense, Shape, Stroke, Vec2};
use eframe::egui::ecolor::rgb_from_hsv;

/// Which part of the wheel the current drag started on.
#[derive(Clone, Copy, PartialEq)]
enum Grab {
    Ring,
    Square,
}

/// Where hues sit on the RYB wheel: (position on the ring, RGB hue), in
/// degrees. Between these points both scales are linear. Orange takes the
/// sixth of the ring that RGB gives to yellow-green, and green and blue
/// spread out less.
const RYB_STOPS: [(f32, f32); 7] = [(0.0, 0.0), (60.0, 30.0), (120.0, 60.0), (180.0, 120.0), (240.0, 240.0), (300.0, 280.0), (360.0, 360.0)];

/// Map 0..=1 between ring position and hue (`to_hue`) or back.
fn ryb_map(t: f32, to_hue: bool) -> f32 {
    let x = t.rem_euclid(1.0) * 360.0;
    for pair in RYB_STOPS.windows(2) {
        let ((p0, h0), (p1, h1)) = (pair[0], pair[1]);
        let (a0, a1, b0, b1) = if to_hue { (p0, p1, h0, h1) } else { (h0, h1, p0, p1) };
        if x <= a1 {
            return (b0 + (x - a0) / (a1 - a0) * (b1 - b0)) / 360.0;
        }
    }
    t
}

/// Show the wheel. `hsv` is (hue, saturation, value), each 0..=1 (the hue
/// is always the screen's RGB hue; `ryb` only changes where hues sit on the ring).
/// Returns true if the user changed the color.
pub fn color_wheel(ui: &mut egui::Ui, hsv: &mut (f32, f32, f32), diameter: f32, ryb: bool) -> bool {
    let hue_at = |pos: f32| if ryb { ryb_map(pos, true) } else { pos };
    let pos_of = |hue: f32| if ryb { ryb_map(hue, false) } else { hue };
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(diameter), Sense::click_and_drag());
    let center = rect.center();
    let outer = diameter / 2.0;
    let thickness = (diameter * 0.09).max(10.0);
    let inner = outer - thickness;
    // The largest square that fits inside the ring, with a little gap.
    let half_side = (inner - 6.0) / std::f32::consts::SQRT_2;
    let square = Rect::from_center_size(center, Vec2::splat(half_side * 2.0));

    let mut changed = false;
    if let (true, Some(pos)) = (response.is_pointer_button_down_on(), response.interact_pointer_pos()) {
        // Decide what we're dragging once, when the press starts, and remember
        // it so dragging off the square keeps adjusting the square (and vice versa).
        let grab_id = response.id.with("grab");
        let just_pressed = ui.input(|i| i.pointer.primary_pressed());
        let stored = ui.data(|d| d.get_temp::<Grab>(grab_id));
        let grab = match stored {
            Some(g) if !just_pressed => g,
            _ => {
                let g = if square.expand(4.0).contains(pos) { Grab::Square } else { Grab::Ring };
                ui.data_mut(|d| d.insert_temp(grab_id, g));
                g
            }
        };
        {
            match grab {
                Grab::Ring => {
                    let d = pos - center;
                    // Hue 0 = red at the top, increasing clockwise.
                    let angle = d.x.atan2(-d.y);
                    hsv.0 = hue_at((angle / std::f32::consts::TAU).rem_euclid(1.0));
                }
                Grab::Square => {
                    hsv.1 = ((pos.x - square.left()) / square.width()).clamp(0.0, 1.0);
                    hsv.2 = 1.0 - ((pos.y - square.top()) / square.height()).clamp(0.0, 1.0);
                }
            }
            changed = true;
        }
    }

    let painter = ui.painter_at(rect);

    // Hue ring: a strip of quads, each colored by its hue.
    let mut ring = Mesh::default();
    const SEGMENTS: u32 = 96;
    for i in 0..=SEGMENTS {
        let h = i as f32 / SEGMENTS as f32;
        let dir = angle_dir(h);
        let color = to_color32(rgb_from_hsv((hue_at(h), 1.0, 1.0)));
        ring.colored_vertex(center + dir * inner, color);
        ring.colored_vertex(center + dir * outer, color);
        if i > 0 {
            let k = i * 2;
            ring.add_triangle(k - 2, k - 1, k);
            ring.add_triangle(k - 1, k + 1, k);
        }
    }
    painter.add(Shape::mesh(ring));

    // Saturation (left -> right) / value (bottom -> top) square for the current hue.
    let mut sq = Mesh::default();
    const GRID: u32 = 16;
    for row in 0..=GRID {
        for col in 0..=GRID {
            let s = col as f32 / GRID as f32;
            let v = 1.0 - row as f32 / GRID as f32;
            let pos = square.min + Vec2::new(s, 1.0 - v) * square.size();
            sq.colored_vertex(pos, to_color32(rgb_from_hsv((hsv.0, s, v))));
            if row > 0 && col > 0 {
                let i = row * (GRID + 1) + col;
                let (a, b, c, d) = (i - GRID - 2, i - GRID - 1, i - 1, i);
                sq.add_triangle(a, b, c);
                sq.add_triangle(b, d, c);
            }
        }
    }
    painter.add(Shape::mesh(sq));

    // Markers for the current hue and saturation/value.
    let marker = |p: Pos2, r: f32| {
        painter.circle_stroke(p, r + 1.0, Stroke::new(1.0, Color32::BLACK));
        painter.circle_stroke(p, r, Stroke::new(2.0, Color32::WHITE));
    };
    marker(center + angle_dir(pos_of(hsv.0)) * (inner + thickness / 2.0), thickness * 0.35);
    marker(square.min + Vec2::new(hsv.1, 1.0 - hsv.2) * square.size(), 5.0);

    changed
}

/// The classic picker: a saturation/brightness square for the current hue,
/// then hue, saturation and brightness sliders drawn as gradients.
/// Same `hsv` and `ryb` meaning as [`color_wheel`]. Returns true if changed.
pub fn classic_picker(ui: &mut egui::Ui, hsv: &mut (f32, f32, f32), ryb: bool) -> bool {
    let mut changed = false;
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, (width * 0.75).clamp(100.0, 220.0)), Sense::click_and_drag());
    if let (true, Some(pos)) = (response.is_pointer_button_down_on(), response.interact_pointer_pos()) {
        hsv.1 = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
        hsv.2 = 1.0 - ((pos.y - rect.top()) / rect.height()).clamp(0.0, 1.0);
        changed = true;
    }
    let painter = ui.painter_at(rect.expand(8.0));
    let mut sq = Mesh::default();
    const GRID: u32 = 16;
    for row in 0..=GRID {
        for col in 0..=GRID {
            let s = col as f32 / GRID as f32;
            let v = 1.0 - row as f32 / GRID as f32;
            let pos = rect.min + Vec2::new(s, 1.0 - v) * rect.size();
            sq.colored_vertex(pos, to_color32(rgb_from_hsv((hsv.0, s, v))));
            if row > 0 && col > 0 {
                let i = row * (GRID + 1) + col;
                let (a, b, c, d) = (i - GRID - 2, i - GRID - 1, i - 1, i);
                sq.add_triangle(a, b, c);
                sq.add_triangle(b, d, c);
            }
        }
    }
    painter.add(Shape::mesh(sq));
    let p = rect.min + Vec2::new(hsv.1, 1.0 - hsv.2) * rect.size();
    painter.circle_stroke(p, 8.0, Stroke::new(1.0, Color32::BLACK));
    painter.circle_stroke(p, 7.0, Stroke::new(2.0, Color32::WHITE));

    let hue_at = |pos: f32| if ryb { ryb_map(pos, true) } else { pos };
    let pos_of = |hue: f32| if ryb { ryb_map(hue, false) } else { hue };
    let (h, s, v) = *hsv;
    let mut hue_pos = pos_of(h);
    if gradient_slider(ui, &mut hue_pos, |t| rgb_from_hsv((hue_at(t), 1.0, 1.0)), "Hue") {
        hsv.0 = hue_at(hue_pos);
        changed = true;
    }
    changed |= gradient_slider(ui, &mut hsv.1, |t| rgb_from_hsv((h, t, v)), "Saturation");
    changed |= gradient_slider(ui, &mut hsv.2, |t| rgb_from_hsv((h, s, t)), "Brightness");
    changed
}

/// A horizontal bar showing `color_at(0..=1)` with a round handle. Returns
/// true while the user drags it.
fn gradient_slider(ui: &mut egui::Ui, value: &mut f32, color_at: impl Fn(f32) -> [f32; 3], name: &str) -> bool {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 18.0), Sense::click_and_drag());
    let response = response.on_hover_text(name);
    let bar = rect.shrink2(Vec2::new(8.0, 5.0));
    let mut changed = false;
    if let (true, Some(pos)) = (response.is_pointer_button_down_on(), response.interact_pointer_pos()) {
        *value = ((pos.x - bar.left()) / bar.width()).clamp(0.0, 1.0);
        changed = true;
    }
    let painter = ui.painter_at(rect);
    let mut mesh = Mesh::default();
    const STEPS: u32 = 48;
    for i in 0..=STEPS {
        let t = i as f32 / STEPS as f32;
        let color = to_color32(color_at(t));
        let x = bar.left() + t * bar.width();
        mesh.colored_vertex(Pos2::new(x, bar.top()), color);
        mesh.colored_vertex(Pos2::new(x, bar.bottom()), color);
        if i > 0 {
            let k = i * 2;
            mesh.add_triangle(k - 2, k - 1, k);
            mesh.add_triangle(k - 1, k + 1, k);
        }
    }
    painter.add(Shape::mesh(mesh));
    let p = Pos2::new(bar.left() + *value * bar.width(), bar.center().y);
    painter.circle_filled(p, 7.0, to_color32(color_at(*value)));
    painter.circle_stroke(p, 7.0, Stroke::new(2.0, Color32::WHITE));
    changed
}

/// Unit vector for a hue: 0 points up, going clockwise.
fn angle_dir(h: f32) -> Vec2 {
    let a = h * std::f32::consts::TAU;
    Vec2::new(a.sin(), -a.cos())
}

fn to_color32([r, g, b]: [f32; 3]) -> Color32 {
    Color32::from_rgb((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

#[cfg(test)]
mod tests {
    use super::ryb_map;

    #[test]
    fn ryb_wheel_puts_paint_complements_opposite() {
        let deg = |pos: f32| ryb_map(pos / 360.0, true) * 360.0;
        assert_eq!(deg(0.0), 0.0); // red at the top
        assert_eq!(deg(180.0), 120.0); // green opposite red
        assert_eq!(deg(120.0), 60.0); // yellow...
        assert_eq!(deg(300.0), 280.0); // ...opposite purple
        assert_eq!(deg(60.0), 30.0); // orange...
        assert_eq!(deg(240.0), 240.0); // ...opposite blue
        for h in [0.1f32, 0.37, 0.5, 0.81] {
            assert!((ryb_map(ryb_map(h, true), false) - h).abs() < 1e-5, "round trip {h}");
        }
    }
}
