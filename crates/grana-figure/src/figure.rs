use glam::Vec2;
use grana_ui::{Instance, Rect, Ui};

use crate::overlay::{self, Legend};

pub fn instances(
    ui: &mut Ui,
    size: [u32; 2],
    pixels_per_point: f32,
    world_per_point: f32,
    legends: &[Legend],
) -> Vec<Instance> {
    let points = Vec2::new(
        size[0] as f32 / pixels_per_point,
        size[1] as f32 / pixels_per_point,
    );
    ui.render_figure(points, pixels_per_point, |painter| {
        overlay::figure(
            painter,
            Rect::from_min_size(Vec2::ZERO, points),
            world_per_point,
            legends,
        );
    })
}
