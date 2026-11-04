use glam::Vec2;
use grana_ui::{Instance, Rect, Ui};

use crate::labels::{self, Placed as PlacedLabel};
use crate::overlay::{self, Caption, Legend, Placed};

pub struct Contents<'a> {
    pub legends: &'a [Legend],
    pub caption: Option<&'a Caption>,
    pub labels: &'a [PlacedLabel],
}

pub fn instances(
    ui: &mut Ui,
    size: [u32; 2],
    pixels_per_point: f32,
    world_per_point: f32,
    contents: Contents<'_>,
    backdrop: bool,
) -> Vec<Instance> {
    let points = Vec2::new(
        size[0] as f32 / pixels_per_point,
        size[1] as f32 / pixels_per_point,
    );
    let rect = Rect::from_min_size(Vec2::ZERO, points);
    let placed = contents
        .caption
        .map(|caption| Placed::new(ui, caption, rect));
    ui.render_figure(points, pixels_per_point, |painter| {
        overlay::figure(
            painter,
            rect,
            world_per_point,
            contents.legends,
            placed.as_ref(),
            backdrop,
        );
        labels::paint(painter, rect, contents.labels);
    })
}
