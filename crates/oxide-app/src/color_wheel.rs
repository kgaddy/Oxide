//! A hue ring with a saturation/value square inside, like most paint apps.

use eframe::egui::{self, Color32, Mesh, Pos2, Rect, Sense, Shape, Stroke, Vec2};
use eframe::egui::ecolor::rgb_from_hsv;

/// Which part of the wheel the current drag started on.
#[derive(Clone, Copy, PartialEq)]
enum Grab {
    Ring,
    Square,
}

/// Show the wheel. `hsv` is (hue, saturation, value), each 0..=1.
/// Returns true if the user changed the color.
pub fn color_wheel(ui: &mut egui::Ui, hsv: &mut (f32, f32, f32), diameter: f32) -> bool {
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
                    hsv.0 = (angle / std::f32::consts::TAU).rem_euclid(1.0);
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
        let color = to_color32(rgb_from_hsv((h, 1.0, 1.0)));
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
    marker(center + angle_dir(hsv.0) * (inner + thickness / 2.0), thickness * 0.35);
    marker(square.min + Vec2::new(hsv.1, 1.0 - hsv.2) * square.size(), 5.0);

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
