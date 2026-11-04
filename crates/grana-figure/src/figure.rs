use glam::Vec2;
use grana_ui::{Instance, Rect, Ui};

use crate::overlay::{self, Caption, Legend, Placed};

pub fn instances(
    ui: &mut Ui,
    size: [u32; 2],
    pixels_per_point: f32,
    world_per_point: f32,
    legends: &[Legend],
    caption: Option<&Caption>,
    backdrop: bool,
) -> Vec<Instance> {
    let points = Vec2::new(
        size[0] as f32 / pixels_per_point,
        size[1] as f32 / pixels_per_point,
    );
    let rect = Rect::from_min_size(Vec2::ZERO, points);
    let placed = caption.map(|caption| Placed::new(ui, caption, rect));
    ui.render_figure(points, pixels_per_point, |painter| {
        overlay::figure(
            painter,
            rect,
            world_per_point,
            legends,
            placed.as_ref(),
            backdrop,
        );
    })
}
