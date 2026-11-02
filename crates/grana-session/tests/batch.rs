use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use grana_session::{Batch, BatchEvent, Outcome, Plan, Protocol, discover};

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
      "detection": {"kind": "surfaces", "threshold": 0.3, "grain": 0.1, "seed": 0.6, "min_volume": 1.0, "opening": 0.0, "closing": 0.0, "min_sphericity": 0.0, "min_mean_intensity": 0.0}
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
    let completed = run(&protocol, &datasets, &output, 2);
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
        lines[1].ends_with(",0.000,1.00,1"),
        "the spot sits inside its nucleus: {objects}"
    );

    let coloc = read(&output.join("alpha").join("coloc-Cells-Signal.csv"));
    assert!(coloc.starts_with("scope,frame"), "{coloc}");
    assert_eq!(coloc.lines().count(), 4, "{coloc}");

    let resumed = run(&protocol, &datasets, &output, 2);
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
    let completed = run(&protocol, &datasets, &output, 1);
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

fn run(protocol: &Protocol, datasets: &[PathBuf], output: &Path, jobs: usize) -> Vec<Status> {
    let batch = Batch::start(Plan {
        protocol: protocol.clone(),
        datasets: datasets.to_vec(),
        output: output.to_path_buf(),
        jobs,
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
