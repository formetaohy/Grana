use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use grana_animation::Timeline;
use grana_display::{PaneLayout, Panes};
use grana_pipeline::{Capture as CapturePort, CaptureKind, CaptureRequest, Channels};
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

#[test]
fn captures_a_movie_and_a_sequence_without_a_window() {
    let Some(_gpu) = crate::support::gpu() else {
        return;
    };
    let temp = Temp::new("grana-capture");
    let source = grana_image::Source::file(fixture("ome.tif"));
    let image = Arc::new(grana_image::Image::open(&source).expect("the fixture opens"));
    let mut scene = Scene::new(Dataset::tiff(
        image.name().to_string(),
        image.pyramid().clone(),
    ));
    scene.set_time(0);
    let channels = Channels::render(Arc::clone(&image), &scene);
    let mut panes = Panes::new();
    panes.set_layout(PaneLayout::Single);
    panes.fit(
        scene.dataset.world_size(),
        scene.dataset.voxel_size(),
        [1.0; grana_display::PANE_COUNT],
    );
    let mut timeline = Timeline::new(4, 0.5);
    timeline.insert(grana_animation::Keyframe {
        at: 0.0,
        time: 0,
        views: panes.views(),
    });
    let view = ViewState {
        panes: panes.states(),
        layout: panes.layout(),
        split: panes.split(),
        row: panes.row(),
        timeline,
        export: Export::new(96, 96, false),
    };
    let capture = grana_app::OfflineCapture::new();
    CapturePort::open(&capture).expect("the capture context opens");
    let still = temp.path().join("still.png");
    CapturePort::capture(
        &capture,
        &channels,
        &scene,
        &CaptureRequest {
            view: view.clone(),
            size: [96, 64],
            transparent: false,
            kind: CaptureKind::Still { time: 0 },
        },
        &still,
    )
    .expect("the still captures");
    assert!(grana_raster::read(&still).is_ok(), "the still is readable");
    let movie = temp.path().join("movie.mp4");
    CapturePort::capture(
        &capture,
        &channels,
        &scene,
        &CaptureRequest {
            view: view.clone(),
            size: [96, 64],
            transparent: false,
            kind: CaptureKind::Movie,
        },
        &movie,
    )
    .expect("the movie captures");
    let bytes = std::fs::read(&movie).expect("the movie lands on disk");
    assert!(
        bytes.len() > 8 && &bytes[4..8] == b"ftyp",
        "the movie is not an mp4: {} bytes",
        bytes.len()
    );
    let frames = temp.path().join("frames");
    CapturePort::capture(
        &capture,
        &channels,
        &scene,
        &CaptureRequest {
            view,
            size: [96, 64],
            transparent: false,
            kind: CaptureKind::Sequence,
        },
        &frames,
    )
    .expect("the sequence captures");
    assert_eq!(
        std::fs::read_dir(&frames)
            .expect("the sequence folder exists")
            .count() as u32,
        scene.frame_count(),
        "the sequence writes one frame per time point"
    );
}

#[test]
fn captures_a_protocol_film_without_a_window() {
    let Some(_gpu) = crate::support::gpu() else {
        return;
    };
    let temp = Temp::new("grana-batch-capture");
    let datasets = temp.path().join("datasets");
    std::fs::create_dir_all(&datasets).expect("the dataset folder is created");
    std::fs::copy(fixture("ome.tif"), datasets.join("ome.tif")).expect("the fixture is copied");
    let output = temp.path().join("runs");
    let source = grana_image::Source::file(fixture("ome.tif"));
    let image = grana_image::Image::open(&source).expect("the fixture opens");
    let mut scene = Scene::new(Dataset::tiff(
        image.name().to_string(),
        image.pyramid().clone(),
    ));
    let spots = scene.add_object(grana_scene::ObjectKind::Spots, 0);
    scene
        .objects
        .get_mut(spots)
        .expect("the set exists")
        .set_params(grana_scene::ObjectParams::Spots(
            grana_object::SpotParams::new(0.5, 0.1),
        ));
    let mut panes = Panes::new();
    panes.set_layout(PaneLayout::Single);
    panes.fit(
        scene.dataset.world_size(),
        scene.dataset.voxel_size(),
        [1.0; grana_display::PANE_COUNT],
    );
    let mut timeline = Timeline::new(4, 0.5);
    timeline.insert(grana_animation::Keyframe {
        at: 0.0,
        time: 0,
        views: panes.views(),
    });
    let view = ViewState {
        panes: panes.states(),
        layout: panes.layout(),
        split: panes.split(),
        row: panes.row(),
        timeline,
        export: Export::new(96, 96, false),
    };
    let captured = grana_protocol::Protocol::capture(
        &scene,
        &Measure::new(),
        Some(grana_protocol::ViewSpec::of(&view, 0)),
        Vec::new(),
    )
    .expect("the scene captures");
    let mut json = serde_json::from_str::<serde_json::Value>(
        &captured.to_json().expect("the protocol serializes"),
    )
    .expect("the protocol is json");
    json["snapshot"]["movies"] = serde_json::Value::Bool(true);
    json["snapshot"]["sequences"] = serde_json::Value::Bool(true);
    let protocol =
        grana_protocol::Protocol::from_json(&json.to_string()).expect("the filmed protocol parses");
    let protocol_path = temp.path().join("protocol.json");
    protocol.write(&protocol_path).expect("the protocol writes");
    let plan = grana_pipeline::Plan {
        protocol,
        datasets: grana_pipeline::discover(&datasets).expect("the folder discovers"),
        output: output.clone(),
        jobs: 1,
        project: false,
        channels: false,
        capture: Some(Arc::new(grana_app::OfflineCapture::new())),
    };
    let running = grana_pipeline::Batch::start(plan);
    let mut done = false;
    while let Some(event) = running.wait() {
        match event {
            grana_pipeline::BatchEvent::Failed(detail) => panic!("the batch failed: {detail}"),
            grana_pipeline::BatchEvent::Done { cancelled } => {
                assert!(!cancelled, "the batch was cancelled");
                done = true;
                break;
            }
            _ => {}
        }
    }
    assert!(done, "the batch stopped before it finished");
    let movie = find(&output, "movie.mp4").expect("the batch writes the movie");
    let bytes = std::fs::read(&movie).expect("the movie is readable");
    assert!(
        bytes.len() > 8 && &bytes[4..8] == b"ftyp",
        "the movie is an mp4"
    );
    let frames = find(&output, "sequence").expect("the batch writes the frame sequence");
    assert!(
        std::fs::read_dir(&frames)
            .expect("the sequence folder reads")
            .count()
            > 0,
        "the sequence writes its frames"
    );
}

fn find(root: &std::path::Path, name: &str) -> Option<PathBuf> {
    for entry in std::fs::read_dir(root).ok()?.flatten() {
        let path = entry.path();
        if path.file_name().and_then(|value| value.to_str()) == Some(name) {
            return Some(path);
        }
        if path.is_dir()
            && let Some(found) = find(&path, name)
        {
            return Some(found);
        }
    }
    None
}
