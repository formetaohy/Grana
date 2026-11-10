use glam::{Mat4, Vec3};
use grana_display::{CameraState, SectionCamera, SectionState};
use grana_figure::{AnnotationItem, Caption, Label, Legend};
use grana_ui::{Instance, Rect, Ui, Vec2};

use crate::panels::Panes;

pub(crate) struct LabelView<'a> {
    pub labels: &'a [Label],
    pub view_projection: Mat4,
    pub depth: grana_figure::Depth,
}

pub(crate) struct Figure<'a> {
    pub panes: &'a Panes,
    pub camera: &'a CameraState,
    pub section: &'a SectionState,
    pub labels: &'a LabelView<'a>,
    pub annotations: &'a [AnnotationItem],
    pub voxel: Vec3,
    pub legends: &'a [Legend],
    pub caption: Option<&'a Caption>,
    pub backdrop: bool,
}

pub(crate) fn instances<A: Clone>(ui: &mut Ui<A>, figure: Figure<'_>) -> Vec<Instance> {
    let panes = figure.panes;
    let scale = panes.pixels_per_point;
    let volume_world_per_point = 2.0 * figure.camera.distance * (0.5 * figure.camera.fov_y).tan()
        / panes.volume_points[3].max(1.0e-3);
    let section_world_per_point = figure.section.span / panes.section_points[3].max(1.0e-3);
    let volume_rect = Rect::from_min_size(
        Vec2::ZERO,
        Vec2::new(panes.volume_points[2], panes.volume_points[3]),
    );
    let section_rect = Rect::from_min_size(
        Vec2::ZERO,
        Vec2::new(panes.section_points[2], panes.section_points[3]),
    );
    let volume_labels = grana_figure::volume_labels(
        figure.labels.labels,
        figure.labels.view_projection,
        figure.labels.depth,
        volume_rect,
    );
    let volume_annotations = grana_figure::volume_annotations(
        figure.annotations,
        figure.labels.view_projection,
        volume_rect,
    );
    let mut instances = grana_figure::instances(
        ui,
        [panes.volume[2], panes.volume[3]],
        scale,
        volume_world_per_point,
        grana_figure::Contents {
            legends: figure.legends,
            caption: figure.caption,
            labels: &volume_labels,
            annotations: &volume_annotations,
        },
        figure.backdrop,
    );
    translate(&mut instances, panes.volume_points);
    let mut section_camera = SectionCamera::new();
    section_camera.restore(*figure.section);
    let section_labels =
        grana_figure::section_labels(figure.labels.labels, &section_camera, section_rect);
    let section_annotations = grana_figure::section_annotations(
        figure.annotations,
        &section_camera,
        section_rect,
        1.5 * figure.voxel.dot(section_camera.normal().abs()),
    );
    let mut scale_bars = grana_figure::instances(
        ui,
        [panes.section[2], panes.section[3]],
        scale,
        section_world_per_point,
        grana_figure::Contents {
            legends: &[],
            caption: None,
            labels: &section_labels,
            annotations: &section_annotations,
        },
        figure.backdrop,
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
