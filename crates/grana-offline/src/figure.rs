use glam::Vec3;
use grana_display::Camera;
use grana_figure::{AnnotationItem, Caption, Label, Legend, PlacedAnnotation, PlacedLabel};
use grana_ui::{Instance, Mark};

pub(crate) struct Figure<'a> {
    pub(crate) viewport: &'a grana_engine::Viewport,
    pub(crate) camera: &'a Camera,
    pub(crate) labels: &'a [Label],
    pub(crate) annotations: &'a [AnnotationItem],
    pub(crate) world: Vec3,
    pub(crate) legends: &'a [Legend],
    pub(crate) caption: Option<&'a Caption>,
    pub(crate) backdrop: bool,
}

struct Prepared {
    size: [u32; 2],
    world_per_point: f32,
    labels: Vec<PlacedLabel>,
    annotations: Vec<PlacedAnnotation>,
}

fn prepare(figure: &Figure<'_>) -> Prepared {
    let [_, _, width, height] = figure.viewport.region;
    let size = [width, height];
    let rect = grana_ui::Rect::from_min_size(
        grana_ui::Vec2::ZERO,
        grana_ui::Vec2::new(
            figure.viewport.points()[2].max(1.0),
            figure.viewport.points()[3].max(1.0),
        ),
    );
    let aspect = rect.width() / rect.height().max(1.0e-3);
    let view_projection = figure
        .camera
        .view_projection(aspect, 0.5 * figure.world.length());
    let labels = grana_figure::volume_labels(
        figure.labels,
        view_projection,
        grana_figure::Depth::new(figure.camera),
        rect,
    );
    let annotations = grana_figure::volume_annotations(figure.annotations, view_projection, rect);
    let world_per_point = 2.0 * figure.camera.distance() * (0.5 * figure.camera.fov_y()).tan()
        / rect.height().max(1.0e-3);
    Prepared {
        size,
        world_per_point,
        labels,
        annotations,
    }
}

pub(crate) fn instances(ui: &mut grana_ui::Ui<()>, figure: Figure<'_>) -> Vec<Instance> {
    let prepared = prepare(&figure);
    assert_eq!(
        prepared.size, figure.viewport.size,
        "the figure covers the export"
    );
    grana_figure::instances(
        ui,
        prepared.size,
        figure.viewport.pixels_per_point,
        prepared.world_per_point,
        grana_figure::Contents {
            legends: figure.legends,
            caption: figure.caption,
            labels: &prepared.labels,
            annotations: &prepared.annotations,
        },
        figure.backdrop,
    )
}

pub(crate) fn marks(ui: &mut grana_ui::Ui<()>, figure: Figure<'_>) -> Vec<Mark> {
    let prepared = prepare(&figure);
    assert_eq!(
        prepared.size, figure.viewport.size,
        "the figure covers the export"
    );
    grana_figure::marks(
        ui,
        prepared.size,
        figure.viewport.pixels_per_point,
        prepared.world_per_point,
        grana_figure::Contents {
            legends: figure.legends,
            caption: figure.caption,
            labels: &prepared.labels,
            annotations: &prepared.annotations,
        },
        figure.backdrop,
    )
}
