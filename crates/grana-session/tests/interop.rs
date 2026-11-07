use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use grana_session::Session;
use zarrs::array::Array;
use zarrs::filesystem::FilesystemStore;

const WIDTH: u32 = 6;
const HEIGHT: u32 = 5;
const DEPTH: u32 = 4;
const FRAMES: u32 = 2;

const FIXTURE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<TrackMate version="8.0.0">
  <Model spatialunits="µm" timeunits="frame">
    <FeatureDeclarations><SpotFeatures /><EdgeFeatures /><TrackFeatures /></FeatureDeclarations>
    <AllSpots nspots="3">
      <SpotsInFrame frame="0">
        <Spot ID="10" name="10" QUALITY="1.5" POSITION_X="0.5" POSITION_Y="0.0" POSITION_Z="0.5" POSITION_T="0.0" FRAME="0" RADIUS="0.4" />
      </SpotsInFrame>
      <SpotsInFrame frame="1">
        <Spot ID="11" name="11" QUALITY="2.5" POSITION_X="-0.5" POSITION_Y="0.0" POSITION_Z="0.5" POSITION_T="1.0" FRAME="1" RADIUS="0.4" />
        <Spot ID="12" name="12" QUALITY="3.5" POSITION_X="0.5" POSITION_Y="-1.0" POSITION_Z="0.5" POSITION_T="1.0" FRAME="1" RADIUS="0.4" />
      </SpotsInFrame>
    </AllSpots>
    <AllTracks>
      <Track TRACK_ID="0" name="Track_0" NUMBER_SPOTS="1">
        <Edge SPOT_SOURCE_ID="10" SPOT_TARGET_ID="11" />
        <Edge SPOT_SOURCE_ID="10" SPOT_TARGET_ID="12" />
      </Track>
    </AllTracks>
  </Model>
  <Settings>
    <ImageData filename="fixture" folder="" width="6" height="5" nslices="4" nframes="2" pixelwidth="1" pixelheight="1" voxeldepth="1" timeinterval="1" />
  </Settings>
</TrackMate>
"#;

#[test]
fn imports_and_exports_a_trackmate_lineage() {
    let temp = TempDir::new("grana-interop");
    let root = temp.path().join("image.zarr");
    write_image(&root);
    let source = temp.path().join("tracks.xml");
    fs::write(&source, FIXTURE).expect("the fixture track file is written");
    let mut session = Session::new();
    session.begin_load(root);
    wait(&mut session, |session| session.scene().is_some());
    assert!(session.import_tracks(&source), "the import starts");
    wait(&mut session, |session| {
        session.scene().is_some_and(|scene| {
            scene
                .objects
                .iter()
                .any(|set| set.provenance().is_interop() && set.detected() > 0)
        }) && !session
            .progress()
            .iter()
            .any(|progress| matches!(progress.job, grana_scene::Job::Import(_)))
    });
    let (id, name, provenance) = {
        let scene = session.scene().expect("the dataset is open");
        let set = scene
            .objects
            .iter()
            .find(|set| set.provenance().is_interop())
            .expect("the track file became an object set");
        assert_eq!(set.provenance().file(), Some(source.to_str().unwrap()));
        assert_eq!(set.name(), "Tracks tracks");
        assert_eq!(set.kind(), grana_scene::ObjectKind::Spots);
        assert_eq!(set.detected(), 2);
        let tracks = set.tracks().expect("the import installed tracks");
        assert_eq!(tracks.set().divisions(), 1);
        assert_eq!(tracks.set().len(), 3);
        assert_eq!(tracks.set().node_count(), 3);
        let first = set.frame(0).expect("the import filled the first frame");
        assert_eq!(first.object_count(), 1);
        let mean = |frame: &grana_scene::ObjectFrame, object: usize| {
            frame.intensities().object(object, 0).mean()
        };
        assert!((mean(first, 0) - 223.0).abs() < 1.0e-3);
        let second = set.frame(1).expect("the import filled the second frame");
        assert_eq!(second.object_count(), 2);
        assert!((mean(second, 0) - 1222.0).abs() < 1.0e-3);
        assert!((mean(second, 1) - 1213.0).abs() < 1.0e-3);
        (set.id(), set.name().to_string(), set.provenance().clone())
    };
    assert_eq!(name, "Tracks tracks");
    assert!(provenance.is_interop());
    let exported = temp.path().join("export.xml");
    session
        .export_tracks(id, &exported)
        .expect("the tracks export");
    let lineage = grana_interop::trackmate::read(&exported).expect("the export reads back");
    assert_eq!(lineage.len(), 3);
    assert_eq!(lineage.edges().len(), 2);
    let rebuilt = lineage.tracks(1.0).expect("the export holds a lineage");
    assert_eq!(rebuilt.divisions(), 1);
    assert_eq!(rebuilt.node_count(), 3);
    let graph = temp.path().join("export.geff");
    session.export_geff(id, &graph).expect("the graph export");
    read_graph(&graph);
}

fn read_graph(path: &Path) {
    let text = fs::read_to_string(path.join("zarr.json")).expect("the graph root is written");
    let root: serde_json::Value = serde_json::from_str(&text).expect("the graph root is json");
    let geff = &root["attributes"]["geff"];
    assert_eq!(geff["geff_version"], grana_interop::geff::VERSION);
    assert_eq!(geff["directed"], true);
    assert_eq!(geff["axes"].as_array().map(Vec::len), Some(4));
    let store = Arc::new(FilesystemStore::new(path).expect("the graph store opens"));
    let ids = Array::open(store.clone(), "/nodes/ids").expect("the node ids exist");
    let nodes = ids
        .retrieve_chunk::<Vec<u64>>(&[0])
        .expect("the node ids read back");
    assert_eq!(nodes, vec![0, 1, 2]);
    let edges = Array::open(store, "/edges/ids").expect("the edge ids exist");
    let rows = edges
        .retrieve_chunk::<Vec<u64>>(&[0, 0])
        .expect("the edge ids read back");
    assert_eq!(rows, vec![0, 1, 0, 2]);
}

fn wait(session: &mut Session, done: impl Fn(&Session) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !done(session) {
        assert!(Instant::now() < deadline, "the session never settled");
        session.poll();
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn write_image(root: &Path) {
    write(
        root.join("zarr.json"),
        serde_json::json!({
            "zarr_format": 3,
            "node_type": "group",
            "attributes": {
                "ome": {
                    "version": "0.5",
                    "multiscales": [{
                        "name": "fixture",
                        "axes": [
                            {"name": "t", "type": "time", "unit": "second"},
                            {"name": "z", "type": "space", "unit": "micrometer"},
                            {"name": "y", "type": "space", "unit": "micrometer"},
                            {"name": "x", "type": "space", "unit": "micrometer"}
                        ],
                        "datasets": [{
                            "path": "0",
                            "coordinateTransformations": [{
                                "type": "scale",
                                "scale": [1.0, 1.0, 1.0, 1.0],
                            }],
                        }],
                    }],
                    "omero": {
                        "channels": [{
                            "color": "00FF00",
                            "window": {"min": 0, "max": 65535, "start": 0, "end": 65535},
                            "label": "GFP",
                        }],
                    },
                },
            },
        })
        .to_string()
        .as_bytes(),
    );
    write(
        root.join("0").join("zarr.json"),
        serde_json::json!({
            "zarr_format": 3,
            "node_type": "array",
            "shape": [FRAMES, DEPTH, HEIGHT, WIDTH],
            "data_type": "uint16",
            "chunk_grid": {
                "name": "regular",
                "configuration": {"chunk_shape": [1, DEPTH, HEIGHT, WIDTH]},
            },
            "chunk_key_encoding": {"name": "default", "configuration": {"separator": "/"}},
            "fill_value": 0,
            "codecs": [{"name": "bytes", "configuration": {"endian": "little"}}],
            "attributes": {},
        })
        .to_string()
        .as_bytes(),
    );
    for frame in 0..FRAMES {
        let mut data = Vec::with_capacity((WIDTH * HEIGHT * DEPTH * 2) as usize);
        for z in 0..DEPTH {
            for y in 0..HEIGHT {
                for x in 0..WIDTH {
                    let value = 1000 * frame + 100 * z + 10 * y + x;
                    data.extend_from_slice(&(value as u16).to_le_bytes());
                }
            }
        }
        write(
            root.join("0")
                .join("c")
                .join(frame.to_string())
                .join("0")
                .join("0")
                .join("0"),
            &data,
        );
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
