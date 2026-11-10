use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use grana_animation::Timeline;
use grana_display::{PaneLayout, Panes};
use grana_project::{Export, Project, Source as ProjectSource, ViewState};
use grana_scene::{Dataset, Measure, Scene};

use crate::support::Temp;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn image_request(input: PathBuf, output: PathBuf) -> grana_app::render::Request {
    grana_app::render::Request {
        input,
        output,
        width: Some(96),
        height: Some(64),
        time: None,
        view: None,
        transparent: false,
    }
}

fn painted(path: &std::path::Path) -> [u32; 2] {
    let image = grana_raster::read(path).expect("the render writes a readable image");
    assert!(
        image.pixels().iter().any(|value| *value > 0),
        "the render writes the scene"
    );
    image.size()
}

#[test]
fn renders_a_dataset_and_a_project_without_a_window() {
    let Some(_gpu) = crate::support::gpu() else {
        return;
    };
    let temp = Temp::new("grana-command");
    let input = fixture("ome.tif");
    let dataset = temp.path().join("dataset.png");
    let written = grana_app::render::run(&image_request(input.clone(), dataset.clone()))
        .expect("the dataset renders");
    assert_eq!(written, dataset);
    assert_eq!(painted(&dataset), [96, 64]);
    let source = grana_image::Source::file(input.clone());
    let image = grana_image::Image::open(&source).expect("the fixture opens");
    let fingerprint = image.fingerprint().expect("the fixture fingerprints");
    let scene = Scene::new(Dataset::tiff(
        image.name().to_string(),
        image.pyramid().clone(),
    ));
    let mut panes = Panes::new();
    panes.set_layout(PaneLayout::Single);
    panes.fit(
        scene.dataset.world_size(),
        scene.dataset.voxel_size(),
        [96.0 / 64.0, 1.0, 1.0, 1.0],
    );
    let project = Project::capture(
        &scene,
        &Measure::new(),
        ProjectSource::File {
            name: image.name().to_string(),
            path: input,
            series: 0,
        },
        ViewState {
            panes: panes.states(),
            layout: panes.layout(),
            split: panes.split(),
            row: panes.row(),
            timeline: Timeline::new(30, 1.0),
            export: Export::new(96, 320, false),
        },
        Vec::new(),
        fingerprint,
    );
    let saved = temp.path().join("scene.grana");
    project
        .write(&saved, &AtomicBool::new(false), &mut |_, _| {})
        .expect("the project writes");
    let rendered = temp.path().join("project.png");
    let written = grana_app::render::run(&image_request(saved, rendered.clone()))
        .expect("the project renders");
    assert_eq!(written, rendered);
    assert_eq!(painted(&rendered), [96, 64]);
}
