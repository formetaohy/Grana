use grana_display::{CameraState, SectionState};
use grana_figure::{Caption, Legend};
use grana_ui::{Instance, Ui, Vec2};

use crate::panels::Panes;

pub(crate) fn instances(
    ui: &mut Ui,
    panes: &Panes,
    camera: &CameraState,
    section: &SectionState,
    legends: &[Legend],
    caption: Option<&Caption>,
    backdrop: bool,
) -> Vec<Instance> {
    let scale = panes.pixels_per_point;
    let volume_world_per_point =
        2.0 * camera.distance * (0.5 * camera.fov_y).tan() / panes.volume_points[3].max(1.0e-3);
    let section_world_per_point = section.span / panes.section_points[3].max(1.0e-3);
    let mut instances = grana_figure::instances(
        ui,
        [panes.volume[2], panes.volume[3]],
        scale,
        volume_world_per_point,
        legends,
        caption,
        backdrop,
    );
    translate(&mut instances, panes.volume_points);
    let mut scale_bars = grana_figure::instances(
        ui,
        [panes.section[2], panes.section[3]],
        scale,
        section_world_per_point,
        &[],
        None,
        backdrop,
    );
    translate(&mut scale_bars, panes.section_points);
    instances.extend(scale_bars);
    instances
}

fn translate(instances: &mut [Instance], rect: [f32; 4]) {
    let delta = Vec2::new(rect[0], rect[1]);
    for instance in instances {
        instance.translate(delta);
    }
}
