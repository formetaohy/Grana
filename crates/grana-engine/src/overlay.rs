use glam::Vec3;
use grana_display::{PANE_COUNT, PaneKind, PaneView};
use grana_figure::{AnnotationItem, Caption, Label, Legend};
use grana_ui::{Instance, Rect, Ui, Vec2};

use crate::panels::Regions;

pub(crate) struct Figure<'a> {
    pub regions: &'a Regions,
    pub views: &'a [PaneView; PANE_COUNT],
    pub labels: &'a [Label],
    pub annotations: &'a [AnnotationItem],
    pub world: Vec3,
    pub voxel: Vec3,
    pub legends: &'a [Legend],
    pub caption: Option<&'a Caption>,
    pub backdrop: bool,
}

pub(crate) fn instances<A: Clone>(ui: &mut Ui<A>, figure: Figure<'_>) -> Vec<Instance> {
    let scale = figure.regions.pixels_per_point;
    let mut instances = Vec::new();
    for (index, pane) in figure.regions.panes.iter().enumerate() {
        let view = figure.views[index];
        let rect = Rect::from_min_size(
            Vec2::ZERO,
            Vec2::new(pane.points[2].max(1.0), pane.points[3].max(1.0)),
        );
        let first = index == 0;
        let legends = if first { figure.legends } else { &[] };
        let caption = if first { figure.caption } else { None };
        let mut built = match pane.kind {
            PaneKind::Volume => {
                let camera = crate::output::restored_camera(&view);
                let aspect = rect.width() / rect.height().max(1.0e-3);
                let view_projection = camera.view_projection(aspect, 0.5 * figure.world.length());
                let labels = grana_figure::volume_labels(
                    figure.labels,
                    view_projection,
                    grana_figure::Depth::new(&camera),
                    rect,
                );
                let annotations =
                    grana_figure::volume_annotations(figure.annotations, view_projection, rect);
                let world_per_point = 2.0 * camera.distance() * (0.5 * camera.fov_y()).tan()
                    / rect.height().max(1.0e-3);
                grana_figure::instances(
                    ui,
                    [pane.region[2], pane.region[3]],
                    scale,
                    world_per_point,
                    grana_figure::Contents {
                        legends,
                        caption,
                        labels: &labels,
                        annotations: &annotations,
                    },
                    figure.backdrop,
                )
            }
            PaneKind::Section => {
                let section = crate::output::restored_section(&view);
                let labels = grana_figure::section_labels(figure.labels, &section, rect);
                let annotations = grana_figure::section_annotations(
                    figure.annotations,
                    &section,
                    rect,
                    1.5 * figure.voxel.dot(section.normal().abs()),
                );
                let world_per_point = section.span() / rect.height().max(1.0e-3);
                grana_figure::instances(
                    ui,
                    [pane.region[2], pane.region[3]],
                    scale,
                    world_per_point,
                    grana_figure::Contents {
                        legends,
                        caption,
                        labels: &labels,
                        annotations: &annotations,
                    },
                    figure.backdrop,
                )
            }
        };
        translate(&mut built, pane.points);
        instances.extend(built);
    }
    instances
}

fn translate(instances: &mut [Instance], rect: [f32; 4]) {
    let delta = Vec2::new(rect[0], rect[1]);
    for instance in instances {
        instance.translate(delta);
    }
}
