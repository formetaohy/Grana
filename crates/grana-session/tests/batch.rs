use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use grana_geometry::{DEFAULT_BUDGET, GeometryStore};
use grana_image::Image;
use grana_measure::Measurement;
use grana_pipeline::{
    Batch, BatchEvent, DatasetEntry, Outcome, Plan, SnapshotRequest, Snapshotter, discover,
};
use grana_project::{Project, Source as ProjectSource};
use grana_protocol::Protocol;
use grana_scene::{ObjectColor, Scene};
use grana_session::Session;

const EXTENT: [u32; 3] = [40, 40, 8];
const FRAMES: u32 = 2;
const BACKGROUND: u16 = 100;
const SIGNAL: u16 = 60_000;

const PROTOCOL: &str = r#"
{
  "sets": [
    {
      "name": "Puncta",

      "color": [0.3, 0.8, 1.0],
      "source": {"kind": "spots", "channel": "Cells", "diameter": 5.0, "contrast": 0.05},
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

      "color": [1.0, 0.7, 0.2],
      "source": {"kind": "surfaces", "channel": "Cells", "threshold": 0.3, "grain": 0.1, "split": 0.6, "min_volume": 1.0, "opening": 0.0, "closing": 0.0, "min_sphericity": 0.0, "min_mean_intensity": 0.0}
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

      "color": [0.3, 0.8, 1.0],
      "source": {"kind": "spots", "channel": "Cells", "diameter": 5.0, "contrast": 0.05},
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
    write_dataset(&root.join("control").join("alpha.zarr"), "Cells", "Signal");
    write_dataset(&root.join("control").join("beta.zarr"), "Other", "Signal");
    write_dataset(&root.join("control").join("delta.zarr"), "Cells", "Signal");
    write_dataset(
        &root.join("treatment").join("gamma.zarr"),
        "Cells",
        "Signal",
    );
    let output = temp.path().join("out");

    let datasets = discover(&root).expect("the folder lists its datasets");
    assert_eq!(datasets.len(), 4);
    assert_eq!(datasets[0].group(), "control");
    assert_eq!(datasets[0].label(), "control/alpha");
    assert_eq!(datasets[1].group(), "control");
    assert_eq!(datasets[3].group(), "treatment");
    assert_eq!(datasets[3].label(), "treatment/gamma");

    let protocol = Protocol::from_json(PROTOCOL).expect("the protocol is valid");
    let completed = run(&protocol, &datasets, &output, 2, false);
    assert_eq!(
        completed,
        [Status::Ok, Status::Failed, Status::Ok, Status::Ok],
        "the batch finishes"
    );

    let summary = read(&output.join("summary.csv"));
    assert!(
        summary.starts_with("dataset,scene,status,objects,detail"),
        "{summary}"
    );
    assert!(
        summary.contains("alpha,,ok,4,Puncta=2; Nuclei=2"),
        "{summary}"
    );
    assert!(summary.contains("beta,,failed"), "{summary}");
    assert!(summary.contains("delta,,ok,4"), "{summary}");
    assert!(summary.contains("gamma,,ok,4"), "{summary}");
    assert!(output.join("protocol.json").exists());
    assert!(
        !output.join("control").join("beta").exists(),
        "a failed dataset leaves no partial output"
    );

    let folder = output.join("control").join("alpha");
    let objects = read(&folder.join("Puncta-objects.csv"));
    let lines = objects.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 3, "both frames report their spot: {objects}");
    assert!(lines[0].contains("diameter_um"), "{objects}");
    assert!(lines[0].contains("cells_intensity_mean"), "{objects}");
    assert!(
        lines[1].starts_with("1,0.000,1,1,"),
        "the spot belongs to one track: {objects}"
    );
    assert!(lines[0].contains("nuclei_distance_um"), "{objects}");
    assert!(lines[0].contains("nuclei_contact_um2"), "{objects}");
    assert!(
        lines[1].ends_with(",0.000,100.00,1,0.000"),
        "the spot sits inside its nucleus: {objects}"
    );

    let coloc = read(&folder.join("coloc-Cells-Signal.csv"));
    assert!(coloc.starts_with("scope,frame"), "{coloc}");
    assert_eq!(coloc.lines().count(), 4, "{coloc}");

    let quality = read(&output.join("quality.csv"));
    let mut rows = quality.lines();
    assert_eq!(
        rows.next(),
        Some("group,dataset,scene,status,set,kind,objects,volume_um3,area_um2,size_um,intensity"),
        "{quality}"
    );
    let rows = rows.collect::<Vec<_>>();
    assert_eq!(rows.len(), 6, "{quality}");
    let puncta = rows[0].split(',').collect::<Vec<_>>();
    assert_eq!(
        &puncta[..7],
        ["control", "alpha", "", "ok", "Puncta", "Spots", "2"],
        "{quality}"
    );
    assert!(puncta[7].is_empty() && puncta[8].is_empty(), "{quality}");
    assert!(!puncta[9].is_empty() && !puncta[10].is_empty(), "{quality}");
    let nuclei = rows[1].split(',').collect::<Vec<_>>();
    assert_eq!(
        &nuclei[..7],
        ["control", "alpha", "", "ok", "Nuclei", "Surfaces", "2"],
        "{quality}"
    );
    assert!(!nuclei[7].is_empty() && !nuclei[8].is_empty(), "{quality}");
    assert!(nuclei[9].is_empty(), "{quality}");

    let comparison = read(&output.join("comparison.csv"));
    let mut rows = comparison.lines();
    assert_eq!(
        rows.next(),
        Some(
            "group,set,kind,datasets,objects,volume_um3_mean,volume_um3_std,area_um2_mean,area_um2_std,size_um_mean,size_um_std,intensity_mean,intensity_std"
        ),
        "{comparison}"
    );
    let rows = rows.collect::<Vec<_>>();
    assert_eq!(rows.len(), 4, "{comparison}");
    let nuclei = rows[0].split(',').collect::<Vec<_>>();
    assert_eq!(
        &nuclei[..4],
        ["control", "Nuclei", "Surfaces", "2"],
        "the group compares its two datasets: {comparison}"
    );
    let control = rows[1].split(',').collect::<Vec<_>>();
    assert_eq!(
        &control[..5],
        ["control", "Puncta", "Spots", "2", "2.0"],
        "{comparison}"
    );
    assert!(
        !control[9].is_empty() && !control[10].is_empty(),
        "a group of two datasets reports its deviation: {comparison}"
    );
    let treatment = rows[2].split(',').collect::<Vec<_>>();
    assert_eq!(
        &treatment[..4],
        ["treatment", "Nuclei", "Surfaces", "1"],
        "{comparison}"
    );

    let stamp = SystemTime::now() + Duration::from_secs(3600);
    for file in [".zattrs", "0/.zarray", "0/0/0/0/0/0"] {
        let handle = fs::File::options()
            .write(true)
            .open(root.join("control").join("alpha.zarr").join(file))
            .expect("the dataset files stay writable");
        handle
            .set_times(fs::FileTimes::new().set_modified(stamp))
            .expect("the modification time is writable");
    }

    let resumed = run(&protocol, &datasets, &output, 2, false);
    assert_eq!(
        resumed,
        [
            Status::Resumed,
            Status::Failed,
            Status::Resumed,
            Status::Resumed
        ],
        "a repeated protocol resumes the completed datasets"
    );
    let summary = read(&output.join("summary.csv"));
    assert!(
        summary.contains("alpha,,resumed,4,Puncta=2; Nuclei=2"),
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
    assert!(summary.contains("alpha,,ok,0,Puncta=0"), "{summary}");
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

#[test]
fn replays_derived_and_merged_object_sets() {
    const DERIVED: &str = r#"
{
  "sets": [
    {
      "name": "Puncta",
      "color": [0.3, 0.8, 1.0],
      "source": {"kind": "spots", "channel": "Cells", "diameter": 5.0, "contrast": 0.05}
    },
    {
      "name": "Signal",
      "color": [1.0, 0.7, 0.2],
      "source": {"kind": "spots", "channel": "Signal", "diameter": 5.0, "contrast": 0.05}
    },
    {
      "name": "Union",
      "color": [0.4, 0.9, 0.6],
      "source": {"kind": "merged", "parts": [
        {"name": "Puncta", "source": {"kind": "spots", "channel": "Cells", "diameter": 5.0, "contrast": 0.05}},
        {"name": "Signal", "source": {"kind": "spots", "channel": "Signal", "diameter": 5.0, "contrast": 0.05}}
      ]}
    },
    {
      "name": "Big",
      "color": [0.9, 0.5, 0.2],
      "source": {"kind": "filtered", "source": "Puncta", "filters": [{"column": "diameter_um", "minimum": 0.0, "maximum": 1000.0}]},
      "curation": {"expected": [1, 1], "removed": [{"frame": 0, "anchors": [{"position": [0.25, 0.25, 0.5], "low": [-2.25, -2.25, -2.0], "high": [2.75, 2.75, 3.0]}]}]}
    }
  ]
}
"#;
    let temp = TempDir::new("grana-batch-derived");
    let root = temp.path().join("datasets");
    write_dataset(&root.join("alpha.zarr"), "Cells", "Signal");
    let output = temp.path().join("out");
    let datasets = discover(&root).expect("the folder lists its datasets");
    let protocol = Protocol::from_json(DERIVED).expect("the derived protocol is valid");
    let completed = run(&protocol, &datasets, &output, 1, false);
    assert_eq!(completed, [Status::Ok], "the batch finishes");
    let summary = read(&output.join("summary.csv"));
    assert!(
        summary.contains("alpha,,ok,9,Puncta=2; Signal=2; Union=4; Big=1 (1 removed by hand)"),
        "{summary}"
    );
    let folder = output.join("alpha");
    let union = read(&folder.join("union-objects.csv"));
    assert_eq!(
        union.lines().count(),
        5,
        "the merged set reports both halves in every frame: {union}"
    );
    let big = read(&folder.join("big-objects.csv"));
    assert_eq!(
        big.lines().count(),
        2,
        "the curated set reports the object it keeps: {big}"
    );
    let quality = read(&output.join("quality.csv"));
    assert_eq!(
        quality.lines().count(),
        5,
        "every object set reports one quality row: {quality}"
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
    datasets: &[DatasetEntry],
    output: &Path,
    jobs: usize,
    project: bool,
) -> Vec<Status> {
    run_with(protocol, datasets, output, jobs, project, None)
}

fn run_with(
    protocol: &Protocol,
    datasets: &[DatasetEntry],
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

      "color": [0.3, 0.8, 1.0],
      "opacity": 0.6,
      "source": {"kind": "spots", "channel": "Cells", "diameter": 5.0, "contrast": 0.05},
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

      "color": [1.0, 0.7, 0.2],
      "color_by": {"key": "sphericity", "palette": "magma", "reversed": true},
      "source": {"kind": "surfaces", "channel": "Cells", "threshold": 0.3, "grain": 0.1, "split": 0.6, "min_volume": 1.0, "opening": 0.0, "closing": 0.0, "min_sphericity": 0.0, "min_mean_intensity": 0.0},
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

      "color": [0.3, 0.8, 1.0],
      "source": {"kind": "spots", "channel": "Cells", "diameter": 5.0, "contrast": 0.05}
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

    let sheet = grana_raster::read(&output.join("sheet.png")).expect("the quality sheet composes");
    assert_eq!(sheet.size(), [824, 524], "the sheet tiles its snapshots");
    let manifest = read(&output.join("sheet.csv"));
    assert!(
        manifest.starts_with("cell,group,dataset,scene,status"),
        "{manifest}"
    );
    assert!(manifest.contains("1,,alpha,,ok"), "{manifest}");

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
    let rebuilt = grana_raster::read(&output.join("sheet.png")).expect("the sheet is rewritten");
    assert_eq!(rebuilt.size(), [824, 524]);

    let orphaned = run(&protocol, &datasets, &output, 1, false);
    assert_eq!(
        orphaned,
        [Status::Failed],
        "a snapshot protocol needs a renderer"
    );
}

const SCENES: &str = r#"
{
  "sets": [
    {
      "name": "Puncta",

      "color": [0.3, 0.8, 1.0],
      "source": {"kind": "spots", "channel": "Cells", "diameter": 5.0, "contrast": 0.05}
    }
  ]
}
"#;

#[test]
fn runs_every_scene_of_a_multi_scene_image() {
    let temp = TempDir::new("grana-batch-scenes");
    let root = temp.path().join("datasets");
    fs::create_dir_all(&root).expect("the dataset folder is created");
    write_scene_image(&root.join("plate.czi"));
    let datasets = discover(&root).expect("the folder lists its datasets");
    assert_eq!(datasets.len(), 2, "the image holds two scenes");
    assert_eq!(datasets[0].label(), "plate Brightfield");
    assert_eq!(datasets[1].label(), "plate Fluorescence");
    let output = temp.path().join("out");
    let protocol = Protocol::from_json(SCENES).expect("the protocol is valid");
    let completed = run(&protocol, &datasets, &output, 1, false);
    assert_eq!(completed, [Status::Ok, Status::Ok], "the batch finishes");
    let summary = read(&output.join("summary.csv"));
    assert!(
        summary.contains("plate,Brightfield,ok,1,Puncta=1"),
        "{summary}"
    );
    assert!(
        summary.contains("plate,Fluorescence,ok,1,Puncta=1"),
        "{summary}"
    );
    let first = read(&output.join("plate-brightfield").join("Puncta-objects.csv"));
    let second = read(&output.join("plate-fluorescence").join("Puncta-objects.csv"));
    assert_eq!(
        first.lines().count(),
        2,
        "the scene writes its own table: {first}"
    );
    assert_eq!(second.lines().count(), 2, "{second}");
    assert_ne!(first, second, "each scene reads its own voxels");
    let quality = read(&output.join("quality.csv"));
    assert!(quality.contains("plate,Brightfield,ok,Puncta"), "{quality}");
    assert!(
        quality.contains("plate,Fluorescence,ok,Puncta"),
        "{quality}"
    );

    let projects = temp.path().join("projects");
    let completed = run(&protocol, &datasets, &projects, 1, true);
    assert_eq!(
        completed,
        [Status::Ok, Status::Ok],
        "the project batch runs"
    );
    let resumed = run(&protocol, &datasets, &projects, 1, true);
    assert_eq!(
        resumed,
        [Status::Resumed, Status::Resumed],
        "a multi scene project resumes on its own artifacts"
    );
}

const SCENE_SIDE: u32 = 40;
const SCENE_DEPTH: u32 = 8;
const CZI_FILE_HEADER_BODY: usize = 512;
const CZI_XML_OFFSET: usize = 256;
const CZI_SUBBLOCK_ENTRY: usize = 240;

struct CziPlane {
    scene: i32,
    depth: u32,
    data: Vec<u8>,
}

fn write_scene_image(path: &Path) {
    let xml = scene_xml();
    let mut planes = Vec::new();
    for scene in 0..2 {
        let centre = [(20.0, 20.0), (12.0, 28.0)][scene];
        for depth in 0..SCENE_DEPTH {
            planes.push(CziPlane {
                scene: scene as i32,
                depth,
                data: scene_samples(centre, depth),
            });
        }
    }
    let metadata = 32 + CZI_FILE_HEADER_BODY;
    let directory = metadata + 32 + CZI_XML_OFFSET + xml.len();
    let directory_body = 128 + planes.len() * czi_entry(0, 0, &czi_axes(0, 0)).len();
    let first_subblock = directory + 32 + directory_body;
    let payloads = planes.iter().map(czi_subblock).collect::<Vec<Vec<u8>>>();
    let mut positions = Vec::new();
    let mut position = first_subblock;
    for payload in &payloads {
        positions.push(position);
        position += payload.len();
    }
    let mut content = vec![0u8; 128];
    content[..4].copy_from_slice(&(planes.len() as i32).to_le_bytes());
    for (plane, position) in planes.iter().zip(positions.iter()) {
        content.extend_from_slice(&czi_entry(
            0,
            *position as u64,
            &czi_axes(plane.scene, plane.depth),
        ));
    }
    let mut file = czi_segment(b"ZISRAWFILE", &[0u8; CZI_FILE_HEADER_BODY]);
    file[32..36].copy_from_slice(&1i32.to_le_bytes());
    file.extend_from_slice(&czi_segment(b"ZISRAWMETADATA", &czi_metadata(&xml)));
    file.extend_from_slice(&czi_segment(b"ZISRAWDIRECTORY", &content));
    for payload in &payloads {
        file.extend_from_slice(payload);
    }
    czi_patch(&mut file, 52, directory);
    czi_patch(&mut file, 60, metadata);
    write(path, file);
}

fn czi_patch(file: &mut [u8], at: usize, value: usize) {
    file[32 + at..32 + at + 8].copy_from_slice(&(value as i64).to_le_bytes());
}

fn czi_segment(id: &[u8], body: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0u8; 32];
    bytes[..id.len()].copy_from_slice(id);
    bytes[16..24].copy_from_slice(&(body.len() as i64).to_le_bytes());
    bytes[24..32].copy_from_slice(&(body.len() as i64).to_le_bytes());
    bytes.extend_from_slice(body);
    bytes
}

fn czi_metadata(xml: &str) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&(xml.len() as u32).to_le_bytes());
    body.extend_from_slice(&0u32.to_le_bytes());
    body.resize(CZI_XML_OFFSET, 0);
    body.extend_from_slice(xml.as_bytes());
    body
}

fn czi_axes(scene: i32, depth: u32) -> Vec<([u8; 4], i32, u32)> {
    vec![
        (*b"X\0\0\0", 0, SCENE_SIDE),
        (*b"Y\0\0\0", 0, SCENE_SIDE),
        (*b"Z\0\0\0", depth as i32, 1),
        (*b"C\0\0\0", 0, 1),
        (*b"T\0\0\0", 0, 1),
        (*b"S\0\0\0", scene, 1),
    ]
}

fn czi_entry(pixel: i32, position: u64, axes: &[([u8; 4], i32, u32)]) -> Vec<u8> {
    let mut bytes = vec![0u8; 32];
    bytes[..2].copy_from_slice(b"DV");
    bytes[2..6].copy_from_slice(&pixel.to_le_bytes());
    bytes[6..14].copy_from_slice(&(position as i64).to_le_bytes());
    bytes[28..32].copy_from_slice(&(axes.len() as i32).to_le_bytes());
    for (code, start, size) in axes {
        let mut fields = vec![0u8; 20];
        fields[..4].copy_from_slice(code);
        fields[4..8].copy_from_slice(&start.to_le_bytes());
        fields[8..12].copy_from_slice(&(*size as i32).to_le_bytes());
        bytes.extend_from_slice(&fields);
    }
    bytes
}

fn czi_subblock(plane: &CziPlane) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&0u32.to_le_bytes());
    body.extend_from_slice(&0u32.to_le_bytes());
    body.extend_from_slice(&(plane.data.len() as u64).to_le_bytes());
    body.extend_from_slice(&czi_entry(0, 0, &czi_axes(plane.scene, plane.depth)));
    body.resize(16 + CZI_SUBBLOCK_ENTRY, 0);
    body.extend_from_slice(&plane.data);
    czi_segment(b"ZISRAWSUBBLOCK", &body)
}

fn scene_samples(centre: (f32, f32), depth: u32) -> Vec<u8> {
    let mut samples = vec![0u8; (SCENE_SIDE * SCENE_SIDE) as usize];
    for y in 0..SCENE_SIDE {
        for x in 0..SCENE_SIDE {
            let distance = ((x as f32 - centre.0).powi(2)
                + (y as f32 - centre.1).powi(2)
                + (depth as f32 - 4.0).powi(2))
            .sqrt();
            if distance < 5.0 {
                samples[(x + SCENE_SIDE * y) as usize] = u8::MAX;
            }
        }
    }
    samples
}

fn scene_xml() -> String {
    let image = |name: &str| {
        format!(
            "<Image Name=\"{name}\"><SizeX>{SCENE_SIDE}</SizeX><SizeY>{SCENE_SIDE}</SizeY><SizeZ>{SCENE_DEPTH}</SizeZ>\
<SizeC>1</SizeC><SizeT>1</SizeT>\
<Dimensions><Channels>\
<Channel Id=\"Channel:1\"><Color>#FF00FF00</Color><Fluor>Cells</Fluor></Channel>\
</Channels></Dimensions></Image>"
        )
    };
    format!(
        "<ImageDocument><Metadata><Information>{}{}</Information>\
<Scaling><Items>\
<Distance Id=\"X\"><Value>0.5E-06</Value></Distance>\
<Distance Id=\"Y\"><Value>0.5E-06</Value></Distance>\
<Distance Id=\"Z\"><Value>1E-06</Value></Distance>\
</Items></Scaling>\
<DisplaySetting><Channels>\
<Channel Id=\"Channel:1\" Name=\"Cells\"><Low>0</Low><High>1</High></Channel>\
</Channels></DisplaySetting>\
</Metadata></ImageDocument>",
        image("Brightfield"),
        image("Fluorescence"),
    )
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
        let raster = grana_raster::Raster::solid(8, 4, [32, 64, 128, 255]);
        grana_raster::write(path, &raster).map_err(String::from)
    }
}

const RELATION_FILTER: &str = r#"
{
  "sets": [
    {
      "name": "Puncta",

      "color": [0.3, 0.8, 1.0],
      "source": {"kind": "spots", "channel": "Cells", "diameter": 5.0, "contrast": 0.05}
    },
    {
      "name": "Nuclei",

      "color": [1.0, 0.7, 0.2],
      "source": {"kind": "surfaces", "channel": "Cells", "threshold": 0.3, "grain": 0.1, "split": 0.6, "min_volume": 1.0, "opening": 0.0, "closing": 0.0, "min_sphericity": 0.0, "min_mean_intensity": 0.0}
    },
    {
      "name": "Inner",

      "color": [0.4, 0.9, 0.6],
      "source": {"kind": "filtered", "source": "Puncta", "filters": [{"column": "nuclei_distance_um", "minimum": 0.0, "maximum": 0.5}]}
    }
  ],
  "relations": [{"source": "Puncta", "target": "Nuclei"}, {"source": "Inner", "target": "Nuclei"}]
}
"#;

#[test]
fn replays_a_filtered_set_that_reads_a_relation() {
    let temp = TempDir::new("grana-batch-relation");
    let root = temp.path().join("datasets");
    write_dataset(&root.join("alpha.zarr"), "Cells", "Signal");
    let output = temp.path().join("out");
    let datasets = discover(&root).expect("the folder lists its datasets");
    let protocol = Protocol::from_json(RELATION_FILTER).expect("the protocol is valid");
    assert_eq!(
        run(&protocol, &datasets, &output, 1, false),
        [Status::Ok],
        "the relation filter replays"
    );
    let summary = read(&output.join("summary.csv"));
    assert!(
        summary.contains("alpha,,ok,6,Puncta=2; Nuclei=2; Inner=2"),
        "{summary}"
    );
    let inner = read(&output.join("alpha").join("Inner-objects.csv"));
    let lines = inner.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 3, "both frames keep their spot: {inner}");
    assert!(lines[0].contains("nuclei_distance_um"), "{inner}");
    assert!(lines[0].contains("nuclei_inside"), "{inner}");
    assert!(
        lines[1].ends_with(",0.000,100.00,1,0.000"),
        "the kept spot sits inside its nucleus: {inner}"
    );
}
