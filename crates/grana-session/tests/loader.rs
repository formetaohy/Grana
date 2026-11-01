use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use grana_session::Session;
use grana_volume::{BrickCoord, BrickRequest, PADDED_SIDE, VoxelData};

const SIDE: u32 = 32;
const LEVELS: [(u32, u16); 2] = [(64, 100), (32, 200)];

#[test]
fn reads_the_requested_level() {
    let root = TempDir::new("grana-loader").path().join("pyramid.zarr");
    write_pyramid(&root);
    let mut session = Session::new();
    session.begin_load(root);
    let deadline = Instant::now() + Duration::from_secs(10);
    while session.scene().is_none() {
        assert!(Instant::now() < deadline, "the dataset never opened");
        session.poll();
        std::thread::sleep(Duration::from_millis(1));
    }
    let scene = session.scene().expect("the dataset is open");
    assert_eq!(scene.dataset.level_count(), LEVELS.len());

    let requests = [
        BrickRequest {
            time: 0,
            level: 0,
            coord: BrickCoord::new(0, 0, 0),
        },
        BrickRequest {
            time: 0,
            level: 1,
            coord: BrickCoord::new(0, 0, 0),
        },
    ];
    session.request(0, &requests);
    let mut values = Vec::new();
    while values.len() < requests.len() {
        assert!(Instant::now() < deadline, "the bricks never arrived");
        session.poll();
        for (request, data) in session.take_bricks() {
            let VoxelData::U16(voxels) = &data.channels()[0] else {
                panic!("the fixture stores uint16 voxels");
            };
            let value = voxels[voxels.len() / 2];
            values.push((request.level, value));
            assert_eq!(
                voxels.len() as u32,
                PADDED_SIDE * PADDED_SIDE * PADDED_SIDE,
                "a brick carries its apron"
            );
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    values.sort();
    assert_eq!(
        values,
        vec![(0, LEVELS[0].1), (1, LEVELS[1].1)],
        "a brick must decode the level it was requested at"
    );
}

fn write_pyramid(root: &Path) {
    write(root.join(".zgroup"), br#"{"zarr_format":2}"#);
    write(
        root.join(".zattrs"),
        br#"{
            "multiscales": [{
                "version": "0.4",
                "name": "pyramid",
                "axes": [
                    {"name": "z", "type": "space", "unit": "micrometer"},
                    {"name": "y", "type": "space", "unit": "micrometer"},
                    {"name": "x", "type": "space", "unit": "micrometer"}
                ],
                "datasets": [
                    {"path": "0", "coordinateTransformations": [{"type": "scale", "scale": [4.0, 0.5, 0.5]}]},
                    {"path": "1", "coordinateTransformations": [{"type": "scale", "scale": [8.0, 1.0, 1.0]}]}
                ]
            }]
        }"#,
    );
    for (level, (side, value)) in LEVELS.iter().enumerate() {
        let chunks = side.div_ceil(SIDE);
        write(
            root.join(level.to_string()).join(".zarray"),
            format!(
                r#"{{"zarr_format":2,"shape":[{side},{side},{side}],"chunks":[{SIDE},{SIDE},{SIDE}],"dtype":"<u2","compressor":null,"fill_value":0,"filters":null,"order":"C","dimension_separator":"/"}}"#
            )
            .as_bytes(),
        );
        for z in 0..chunks {
            for y in 0..chunks {
                for x in 0..chunks {
                    let mut data = Vec::with_capacity((SIDE * SIDE * SIDE) as usize * 2);
                    for _ in 0..SIDE * SIDE * SIDE {
                        data.extend_from_slice(&value.to_le_bytes());
                    }
                    write(
                        root.join(level.to_string())
                            .join(z.to_string())
                            .join(y.to_string())
                            .join(x.to_string()),
                        &data,
                    );
                }
            }
        }
    }
}

fn write(path: PathBuf, content: &[u8]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("the fixture directories are created");
    }
    fs::write(path, content).expect("the fixture files are written");
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
