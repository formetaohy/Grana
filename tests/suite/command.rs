use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use grana_animation::Timeline;
use grana_display::Camera;
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
        lineage: None,
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
    let mut camera = Camera::new(35.0);
    camera.fit(scene.dataset.world_size(), 96.0 / 64.0);
    let project = Project::capture(
        &scene,
        &Measure::new(),
        ProjectSource::File {
            name: image.name().to_string(),
            path: input,
            series: 0,
        },
        ViewState {
            camera: camera.state(),
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
fn keeps_previous_results_when_a_batch_rerun_fails() {
    let temp = Temp::new("grana-batch-swap");
    let datasets = temp.path().join("datasets");
    std::fs::create_dir_all(&datasets).expect("the dataset folder is created");
    std::fs::copy(fixture("ome.tif"), datasets.join("first.tif")).expect("the fixture is copied");
    std::fs::copy(fixture("ome.tif"), datasets.join("second.tif")).expect("the fixture is copied");
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
    let mut capture = toml::from_str::<toml::Value>(
        &grana_protocol::Protocol::capture(&scene, &Measure::new(), None, Vec::new(), None)
            .expect("the scene captures")
            .to_toml()
            .expect("the protocol serializes"),
    )
    .expect("the protocol is toml");
    capture
        .as_table_mut()
        .expect("the protocol is a table")
        .remove("snapshot");
    let protocol = grana_protocol::Protocol::from_toml(
        &toml::to_string(&capture).expect("the protocol serializes"),
    )
    .expect("the protocol parses");
    let output = temp.path().join("runs");
    let start = || {
        grana_pipeline::Batch::start(grana_pipeline::Plan {
            protocol: protocol.clone(),
            datasets: grana_pipeline::discover(&datasets, false).expect("the folder discovers"),
            output: output.clone(),
            jobs: 1,
            project: false,
            channels: false,
            capture: None,
        })
    };
    let running = start();
    let mut failures = 0;
    let mut done = false;
    while let Some(event) = running.wait() {
        match event {
            grana_pipeline::BatchEvent::Dataset {
                outcome: grana_pipeline::Outcome::Failed { .. },
                ..
            }
            | grana_pipeline::BatchEvent::Failed(_) => failures += 1,
            grana_pipeline::BatchEvent::Done { cancelled } => {
                assert!(!cancelled, "the first run is not cancelled");
                done = true;
                break;
            }
            _ => {}
        }
    }
    assert!(done && failures == 0, "the first run completes");
    let written = results(&output);
    assert_eq!(written.len(), 2, "the first run writes both datasets");
    let manifest = std::fs::read_to_string(output.join("run.toml")).expect("the run record lands");
    let manifest = toml::from_str::<toml::Value>(&manifest).expect("the run record is toml");
    let rows = manifest["datasets"]
        .as_array()
        .expect("the run record lists its datasets");
    assert!(!rows.is_empty(), "the run record names the datasets");
    for field in ["content", "bytes", "modified", "protocol"] {
        assert!(
            rows.iter().all(|dataset| dataset.get(field).is_some()),
            "the run record keeps the input evidence {field}"
        );
    }
    let metadata = std::fs::read_to_string(output.join("ro-crate-metadata.json"))
        .expect("the provenance crate lands");
    for field in [
        "\"@context\"",
        "\"#input-1\"",
        "\"#output-1\"",
        "\"protocol.toml\"",
        "\"sha256\"",
        "\"CreateAction\"",
    ] {
        assert!(
            metadata.contains(field),
            "the provenance crate records {field}"
        );
    }

    let truncated = std::fs::read(fixture("ome.tif")).expect("the fixture reads");
    std::fs::write(datasets.join("second.tif"), &truncated[..8]).expect("the dataset truncates");
    let running = start();
    let mut failed = false;
    while let Some(event) = running.wait() {
        match event {
            grana_pipeline::BatchEvent::Dataset {
                outcome: grana_pipeline::Outcome::Failed { .. },
                ..
            }
            | grana_pipeline::BatchEvent::Failed(_) => failed = true,
            grana_pipeline::BatchEvent::Done { .. } => {}
            _ => {}
        }
    }
    assert!(failed, "the truncated dataset fails");
    assert_eq!(
        results(&output).len(),
        2,
        "a failed rerun keeps the previous results of both datasets"
    );
}

fn results(root: &std::path::Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut folders = vec![root.to_path_buf()];
    while let Some(folder) = folders.pop() {
        let Ok(listing) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in listing.flatten() {
            let path = entry.path();
            if path.is_dir() {
                folders.push(path);
            } else if path.file_name().and_then(|name| name.to_str()) == Some("result.toml") {
                found.push(path);
            }
        }
    }
    found
}
