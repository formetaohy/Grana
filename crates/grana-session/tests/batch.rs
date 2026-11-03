use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use grana_geometry::{DEFAULT_BUDGET, GeometryStore};
use grana_image::Image;
use grana_measure::Measurement;
use grana_project::{Project, Source as ProjectSource};
use grana_scene::{ObjectColor, Scene};
use grana_session::{
    Batch, BatchEvent, Outcome, Plan, Protocol, Session, SnapshotRequest, Snapshotter, discover,
};

const EXTENT: [u32; 3] = [40, 40, 8];
const FRAMES: u32 = 2;
const BACKGROUND: u16 = 100;
const SIGNAL: u16 = 60_000;

const PROTOCOL: &str = r#"
{
  "sets": [
    {
      "name": "Puncta",
      "channel": "Cells",
      "color": [0.3, 0.8, 1.0],
      "detection": {"kind": "spots", "diameter": 5.0, "contrast": 0.05},
      "track": {
        "max_step": 2.0,
        "max_gap": 1,
        "division": false,
        "division_range": 2.0,
        "thickness": 0.5
      }
    },
    {
      "name": "Nuclei",
      "channel": "Cells",
      "color": [1.0, 0.7, 0.2],
      "detection": {"kind": "surfaces", "threshold": 0.3, "grain": 0.1, "split": 0.6, "min_volume": 1.0, "opening": 0.0, "closing": 0.0, "min_sphericity": 0.0, "min_mean_intensity": 0.0}
    }
  ],
  "relations": [{"source": "Puncta", "target": "Nuclei"}],
  "coloc": {"settings": {"permutations": 8, "block": 3}, "pairs": [{"a": "Cells", "b": "Signal", "thresholds": [0.5, 0.5]}], "mask": null}
}
"#;

const LIMITED: &str = r#"
{
  "region": {"low": [0.9, 0.9, 0.9], "high": [1.0, 1.0, 1.0]},
  "sets": [
    {
      "name": "Puncta",
      "channel": "Cells",
      "color": [0.3, 0.8, 1.0],
      "detection": {"kind": "spots", "diameter": 5.0, "contrast": 0.05},
      "track": {
        "max_step": 2.0,
        "max_gap": 1,
        "division": false,
        "division_range": 2.0,
        "thickness": 0.5
      },
      "measure": {
        "group": "objects",
        "aggregate": "mean",
        "filters": [{"column": "diameter_um", "minimum": 0.0, "maximum": 1000.0}]
      }
    }
  ]
}
"#;

#[test]
fn runs_a_protocol_over_a_folder_of_datasets() {
    let temp = TempDir::new("grana-batch");
    let root = temp.path().join("datasets");
    write_dataset(&root.join("alpha.zarr"), "Cells", "Signal");
    write_dataset(&root.join("beta.zarr"), "Other", "Signal");
    let output = temp.path().join("out");

    let datasets = discover(&root).expect("the folder lists its datasets");
    assert_eq!(datasets.len(), 2);
    assert!(datasets[0].ends_with("alpha.zarr"));
    assert!(datasets[1].ends_with("beta.zarr"));

    let protocol = Protocol::from_json(PROTOCOL).expect("the protocol is valid");
    let completed = run(&protocol, &datasets, &output, 2, false);
    assert_eq!(
        completed,
        [Status::Ok, Status::Failed],
        "the batch finishes"
    );

    let summary = read(&output.join("summary.csv"));
    assert!(
        summary.contains("alpha,ok,4,Puncta=2; Nuclei=2"),
        "{summary}"
    );
    assert!(summary.contains("beta,failed"), "{summary}");
    assert!(output.join("protocol.json").exists());
    assert!(
        !output.join("beta").exists(),
        "a failed dataset leaves no partial output"
    );

    let objects = read(&output.join("alpha").join("Puncta-objects.csv"));
    let lines = objects.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 3, "both frames report their spot: {objects}");
    assert!(lines[0].contains("diameter_um"), "{objects}");
    assert!(lines[0].contains("cells_intensity_mean"), "{objects}");
    assert!(
        lines[1].starts_with("1,0.000,1,1,"),
        "the spot belongs to one track: {objects}"
    );
    assert!(lines[0].contains("nuclei_distance_um"), "{objects}");
    assert!(
        lines[1].ends_with(",0.000,100.00,1"),
        "the spot sits inside its nucleus: {objects}"
    );

    let coloc = read(&output.join("alpha").join("coloc-Cells-Signal.csv"));
    assert!(coloc.starts_with("scope,frame"), "{coloc}");
    assert_eq!(coloc.lines().count(), 4, "{coloc}");

    let quality = read(&output.join("quality.csv"));
    let mut rows = quality.lines();
    assert_eq!(
        rows.next(),
        Some("dataset,status,set,kind,objects,volume_um3,area_um2,size_um,intensity"),
        "{quality}"
    );
    let rows = rows.collect::<Vec<_>>();
    assert_eq!(rows.len(), 2, "{quality}");
    let puncta = rows[0].split(',').collect::<Vec<_>>();
    assert_eq!(
        &puncta[..5],
        ["alpha", "ok", "Puncta", "Spots", "2"],
        "{quality}"
    );
    assert!(puncta[5].is_empty() && puncta[6].is_empty(), "{quality}");
    assert!(!puncta[7].is_empty() && !puncta[8].is_empty(), "{quality}");
    let nuclei = rows[1].split(',').collect::<Vec<_>>();
    assert_eq!(
        &nuclei[..5],
        ["alpha", "ok", "Nuclei", "Surfaces", "2"],
        "{quality}"
    );
    assert!(!nuclei[5].is_empty() && !nuclei[6].is_empty(), "{quality}");
    assert!(nuclei[7].is_empty(), "{quality}");

    let stamp = SystemTime::now() + Duration::from_secs(3600);
    for file in [".zattrs", "0/.zarray", "0/0/0/0/0/0"] {
        let handle = fs::File::options()
            .write(true)
            .open(root.join("alpha.zarr").join(file))
            .expect("the dataset files stay writable");
        handle
            .set_times(fs::FileTimes::new().set_modified(stamp))
            .expect("the modification time is writable");
    }

    let resumed = run(&protocol, &datasets, &output, 2, false);
    assert_eq!(
        resumed,
        [Status::Resumed, Status::Failed],
        "a repeated protocol resumes the completed dataset"
    );
    let summary = read(&output.join("summary.csv"));
    assert!(
        summary.contains("alpha,resumed,4,Puncta=2; Nuclei=2"),
        "{summary}"
    );
}

#[test]
fn applies_the_region_and_the_statistics_filters() {
    let temp = TempDir::new("grana-batch-region");
    let root = temp.path().join("datasets");
    write_dataset(&root.join("alpha.zarr"), "Cells", "Signal");
    let output = temp.path().join("out");
    let datasets = discover(&root).expect("the folder lists its datasets");
    let protocol = Protocol::from_json(LIMITED).expect("the protocol is valid");
    let completed = run(&protocol, &datasets, &output, 1, true);
    assert_eq!(completed, [Status::Ok], "the batch finishes");
    let summary = read(&output.join("summary.csv"));
    assert!(summary.contains("alpha,ok,0,Puncta=0"), "{summary}");
    let objects = read(&output.join("alpha").join("Puncta-objects.csv"));
    assert_eq!(
        objects.lines().count(),
        1,
        "the region excludes every spot: {objects}"
    );
    assert!(
        !output.join("alpha").join("Puncta-tracks.csv").exists(),
        "the protocol exports only its grouping"
    );
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    Ok,
    Resumed,
    Failed,
}

fn run(
    protocol: &Protocol,
    datasets: &[PathBuf],
    output: &Path,
    jobs: usize,
    project: bool,
) -> Vec<Status> {
    run_with(protocol, datasets, output, jobs, project, None)
}

fn run_with(
    protocol: &Protocol,
    datasets: &[PathBuf],
    output: &Path,
    jobs: usize,
    project: bool,
    snapshot: Option<std::sync::Arc<dyn Snapshotter>>,
) -> Vec<Status> {
    let batch = Batch::start(Plan {
        protocol: protocol.clone(),
        datasets: datasets.to_vec(),
        output: output.to_path_buf(),
        jobs,
        project,
        snapshot,
    });
    let mut completed = vec![Status::Failed; datasets.len()];
    while let Some(event) = batch.wait() {
        match event {
            BatchEvent::Dataset { index, outcome } => {
                completed[index] = match outcome {
                    Outcome::Complete { resumed: false, .. } => Status::Ok,
                    Outcome::Complete { resumed: true, .. } => Status::Resumed,
                    Outcome::Failed { .. } => Status::Failed,
                };
            }
            BatchEvent::Done { cancelled } => {
                assert!(!cancelled, "the batch was not cancelled");
                return completed;
            }
            _ => {}
        }
    }
    panic!("the batch stopped before it finished");
}

fn write_dataset(path: &Path, first: &str, second: &str) {
    fs::create_dir_all(path.join("0")).expect("the dataset folders are created");
    write(&path.join(".zgroup"), br#"{"zarr_format": 2}"#.to_vec());
    write(
        &path.join(".zattrs"),
        format!(
            r#"{{
                "multiscales": [{{
                    "version": "0.4",
                    "name": "fixture",
                    "axes": [
                        {{"name": "t", "type": "time", "unit": "second"}},
                        {{"name": "c", "type": "channel"}},
                        {{"name": "z", "type": "space", "unit": "micrometer"}},
                        {{"name": "y", "type": "space", "unit": "micrometer"}},
                        {{"name": "x", "type": "space", "unit": "micrometer"}}
                    ],
                    "datasets": [
                        {{"path": "0", "coordinateTransformations": [{{"type": "scale", "scale": [1.0, 1.0, 1.0, 0.5, 0.5]}}]}}
                    ]
                }}],
                "omero": {{
                    "channels": [
                        {{"color": "00FF00", "window": {{"min": 0, "max": 65535, "start": 0, "end": 65535}}, "label": "{first}"}},
                        {{"color": "FF0000", "window": {{"min": 0, "max": 65535, "start": 0, "end": 65535}}, "label": "{second}"}}
                    ]
                }}
            }}"#
        )
        .into_bytes(),
    );
    write(
        &path.join("0").join(".zarray"),
        format!(
            r#"{{"zarr_format":2,"shape":[{FRAMES},2,{},{},{}],"chunks":[1,1,{},{},{}],"dtype":"<u2","compressor":null,"fill_value":0,"filters":null,"order":"C","dimension_separator":"/"}}"#,
            EXTENT[2], EXTENT[1], EXTENT[0], EXTENT[2], EXTENT[1], EXTENT[0]
        )
        .into_bytes(),
    );
    for time in 0..FRAMES {
        for channel in 0..2u16 {
            let mut data = vec![BACKGROUND; (EXTENT[0] * EXTENT[1] * EXTENT[2]) as usize];
            paint(&mut data, time);
            let mut chunk = Vec::new();
            for z in 0..EXTENT[2] as usize {
                for y in 0..EXTENT[1] as usize {
                    for x in 0..EXTENT[0] as usize {
                        let value = data[x + EXTENT[0] as usize * (y + EXTENT[1] as usize * z)];
                        chunk.extend_from_slice(&value.to_le_bytes());
                    }
                }
            }
            write(
                &path
                    .join("0")
                    .join(time.to_string())
                    .join(channel.to_string())
                    .join("0")
                    .join("0")
                    .join("0"),
                chunk,
            );
        }
    }
}

fn paint(data: &mut [u16], time: u32) {
    let centre = [20.0 + time as f32, 20.0, 4.0];
    for z in 0..EXTENT[2] {
        for y in 0..EXTENT[1] {
            for x in 0..EXTENT[0] {
                let distance = ((x as f32 - centre[0]).powi(2)
                    + (y as f32 - centre[1]).powi(2)
                    + (z as f32 - centre[2]).powi(2))
                .sqrt();
                if distance < 5.0 {
                    data[(x + EXTENT[0] * (y + EXTENT[1] * z)) as usize] = SIGNAL;
                }
            }
        }
    }
}

fn write(path: &Path, content: Vec<u8>) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("the fixture directories are created");
    }
    fs::write(path, content).expect("the fixture files are written");
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(name: &str) -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("the clock is after the epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("{name}-{}-{stamp}", std::process::id()));
        fs::create_dir_all(&path).expect("the temporary directory is created");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

const PROJECT: &str = r#"
{
  "channels": [
    {"name": "Cells", "enabled": true, "opacity": 0.8, "window": {"low": 0.0, "high": 65535.0}},
    {"name": "Signal", "enabled": false, "opacity": 0.5, "window": {"low": 0.0, "high": 65535.0}}
  ],
  "sets": [
    {
      "name": "Puncta",
      "channel": "Cells",
      "color": [0.3, 0.8, 1.0],
      "opacity": 0.6,
      "detection": {"kind": "spots", "diameter": 5.0, "contrast": 0.05},
      "track": {
        "max_step": 2.0,
        "max_gap": 1,
        "division": false,
        "division_range": 2.0,
        "thickness": 0.5
      }
    },
    {
      "name": "Nuclei",
      "channel": "Cells",
      "color": [1.0, 0.7, 0.2],
      "color_by": {"key": "sphericity", "palette": "magma", "reversed": true},
      "detection": {"kind": "surfaces", "threshold": 0.3, "grain": 0.1, "split": 0.6, "min_volume": 1.0, "opening": 0.0, "closing": 0.0, "min_sphericity": 0.0, "min_mean_intensity": 0.0},
      "measure": {"group": "objects", "aggregate": "mean", "columns": ["volume_um3"]}
    }
  ],
  "relations": [{"source": "Puncta", "target": "Nuclei"}],
  "view": {"camera": {"pivot": [0.0, 0.0, 0.0], "yaw": 0.3, "pitch": 0.2, "distance": 120.0, "fov_y": 0.61}, "section": {"axis": "z", "span": 9.0}, "split": 0.4}
}
"#;

#[test]
fn writes_a_project_that_reopens() {
    let temp = TempDir::new("grana-batch-project");
    let root = temp.path().join("datasets");
    write_dataset(&root.join("alpha.zarr"), "Cells", "Signal");
    let output = temp.path().join("out");
    let datasets = discover(&root).expect("the folder lists its datasets");
    let protocol = Protocol::from_json(PROJECT).expect("the protocol is valid");
    let completed = run(&protocol, &datasets, &output, 1, true);
    assert_eq!(completed, [Status::Ok], "the batch finishes");
    let folder = output.join("alpha");
    let file = fs::read_dir(&folder)
        .expect("the dataset folder lists its files")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|kind| kind == "granaproj"))
        .expect("the batch writes the project");
    let store = GeometryStore::temporary(DEFAULT_BUDGET).expect("the geometry store opens");
    let cancel = AtomicBool::new(false);
    let project = Project::read(&file, store, &cancel, &mut |_, _| {}).expect("the project reads");
    let names = project
        .scene
        .sets
        .iter()
        .map(|set| set.name.clone())
        .collect::<Vec<_>>();
    assert_eq!(names, ["Puncta", "Nuclei"]);
    assert!(matches!(project.source, ProjectSource::File { .. }));
    assert_eq!(project.scene.channels.len(), 2);
    assert!(project.scene.channels[0].enabled);
    assert_eq!(project.scene.channels[0].opacity, 0.8);
    assert!(!project.scene.channels[1].enabled);
    assert_eq!(project.scene.channels[1].opacity, 0.5);
    assert_eq!(project.scene.channels[1].window.min(), 0.0);
    assert_eq!(project.scene.channels[1].window.max(), 65_535.0);
    let puncta = &project.scene.sets[0];
    assert_eq!(puncta.opacity, 0.6);
    assert_eq!(puncta.color, [0.3, 0.8, 1.0]);
    assert_eq!(puncta.frames.len(), FRAMES as usize);
    assert_eq!(
        puncta.frames.iter().filter(|frame| frame.is_some()).count(),
        FRAMES as usize
    );
    assert!(puncta.tracks.is_some(), "the project carries the tracks");
    assert_eq!(puncta.relations.len(), 1);
    assert!(
        puncta.relations[0]
            .frames
            .iter()
            .all(|frame| frame.is_some()),
        "the project carries the relation of every frame"
    );
    let nuclei = &project.scene.sets[1];
    assert_eq!(
        nuclei.object_color.metric,
        Some(ObjectColor::Measurement(Measurement::Sphericity))
    );
    assert!(project.measure.set.is_some());
    assert_eq!(project.measure.set, Some(nuclei.id));
    assert!(project.measure.columns[..4].iter().all(|shown| *shown));
    assert!(
        project.measure.columns.iter().any(|shown| !*shown),
        "the protocol hides the columns it does not show"
    );
    assert_eq!(project.view.camera.distance, 120.0);
    assert_eq!(project.view.section.span, 9.0);
    assert_eq!(project.view.split, 0.4);
    drop(project);

    let mut reopened = Session::new();
    reopened.begin_project(file.clone());
    let deadline = Instant::now() + Duration::from_secs(60);
    while reopened.transferring() || reopened.loading().is_some() {
        assert!(Instant::now() < deadline, "the session opens the project");
        reopened.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
    reopened.poll();
    let scene = reopened.scene().expect("the project opens its dataset");
    let names = scene
        .objects
        .iter()
        .map(|set| set.name().to_string())
        .collect::<Vec<_>>();
    assert_eq!(names, ["Puncta", "Nuclei"]);
    assert_eq!(
        scene
            .objects
            .get(scene.objects.iter().next().unwrap().id())
            .unwrap()
            .detected(),
        FRAMES as usize
    );
    assert_eq!(
        reopened.measure().set(),
        Some(scene.objects.iter().nth(1).unwrap().id())
    );
    let view = reopened.take_view().expect("the project restores its view");
    assert_eq!(view.camera.distance, 120.0);
    drop(reopened);
    fs::remove_file(&file).expect("the project is removable");
    let again = run(&protocol, &datasets, &output, 1, true);
    assert_eq!(
        again,
        [Status::Ok],
        "a deleted project invalidates the resume marker"
    );
}

const SNAPSHOT: &str = r#"
{
  "sets": [
    {
      "name": "Puncta",
      "channel": "Cells",
      "color": [0.3, 0.8, 1.0],
      "detection": {"kind": "spots", "diameter": 5.0, "contrast": 0.05}
    }
  ],
  "snapshot": {"width": 320, "height": 200}
}
"#;

#[test]
fn renders_a_snapshot_at_the_end_of_every_dataset() {
    let temp = TempDir::new("grana-batch-snapshot");
    let root = temp.path().join("datasets");
    write_dataset(&root.join("alpha.zarr"), "Cells", "Signal");
    let output = temp.path().join("out");
    let datasets = discover(&root).expect("the folder lists its datasets");
    let protocol = Protocol::from_json(SNAPSHOT).expect("the protocol is valid");
    let recorder = std::sync::Arc::new(Recorder::default());
    let completed = run_with(
        &protocol,
        &datasets,
        &output,
        1,
        false,
        Some(recorder.clone()),
    );
    assert_eq!(completed, [Status::Ok], "the batch finishes");
    let file = output.join("alpha").join("snapshot.png");
    assert!(file.is_file(), "the dataset carries its quality snapshot");
    let record = read(&output.join("alpha").join("result.json"));
    assert!(record.contains("\"snapshot.png\""), "{record}");
    let request = recorder
        .request
        .lock()
        .expect("the recorder is readable")
        .take()
        .expect("the batch requests one snapshot");
    assert_eq!(request.size, [320, 200]);
    assert!(!request.transparent);
    assert_eq!(request.time, 0);
    assert!(
        request.view.is_none(),
        "a protocol without a view lets the renderer fit its panes"
    );
    assert!(request.objects > 0, "the snapshot renders the detected set");
    assert_eq!(recorder.calls.load(Ordering::Relaxed), 1);
    assert_eq!(
        recorder.opened.load(Ordering::Relaxed),
        1,
        "the run opens its renderer before any dataset"
    );

    fs::remove_file(&file).expect("the snapshot is removable");
    let again = run_with(
        &protocol,
        &datasets,
        &output,
        1,
        false,
        Some(recorder.clone()),
    );
    assert_eq!(
        again,
        [Status::Ok],
        "a deleted snapshot invalidates the resume marker"
    );
    assert_eq!(recorder.calls.load(Ordering::Relaxed), 2);

    let orphaned = run(&protocol, &datasets, &output, 1, false);
    assert_eq!(
        orphaned,
        [Status::Failed],
        "a snapshot protocol needs a renderer"
    );
}

struct SnapshotCall {
    size: [u32; 2],
    transparent: bool,
    time: u32,
    view: Option<f32>,
    objects: usize,
}

#[derive(Default)]
struct Recorder {
    calls: AtomicUsize,
    opened: AtomicUsize,
    request: Mutex<Option<SnapshotCall>>,
}

impl Snapshotter for Recorder {
    fn open(&self) -> Result<(), String> {
        self.opened.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    fn snapshot(
        &self,
        _reader: &Image,
        scene: &Scene,
        request: &SnapshotRequest,
        path: &Path,
    ) -> Result<(), String> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        *self.request.lock().expect("the recorder is writable") = Some(SnapshotCall {
            size: request.size,
            transparent: request.transparent,
            time: request.time,
            view: request.view.as_ref().map(|view| view.split),
            objects: scene.objects.iter().map(|set| set.detected()).sum(),
        });
        fs::write(path, b"snapshot").map_err(|error| error.to_string())
    }
}
