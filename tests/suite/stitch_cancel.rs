use grana_exec::Cancel;
use grana_image::Source;
use grana_session::Session;
use grana_stitch::{Request, stitch};
use grana_volume::{Channel, Extent, Spacing, Volume, VolumeSeries, VoxelData};

const TILE: usize = 160;
const OVERLAP: usize = 32;
const DEPTH: u32 = 4;

#[test]
fn scans_a_tile_folder_before_offering_a_grid() {
    let temp = crate::support::Temp::new("grana-stitch-scan");
    let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("plane.tif");
    for index in 0..4 {
        std::fs::copy(&fixture, temp.path().join(format!("tile-{index}.tif")))
            .expect("a tile is placed in the folder");
    }
    let mut session = Session::new();
    assert!(session.begin_stitch(temp.path().to_path_buf()));
    let scanned = session.stitch().expect("the stitch panel opens");
    assert_eq!(scanned.view().tiles, 0, "the panel starts out empty");
    assert!(scanned.view().scanning, "the panel reports the scan");
    assert_eq!(
        session.stitch().map(|stitch| stitch.view().rows),
        Some(0),
        "a grid arrives only once the folder is scanned"
    );
    for _ in 0..600 {
        session.poll();
        if !session
            .stitch()
            .expect("the panel stays open")
            .view()
            .scanning
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let view = session.stitch().expect("the panel stays open").view();
    assert!(!view.scanning, "the scan settles");
    assert_eq!(view.tiles, 4, "the folder holds four tiles");
    assert_eq!((view.rows, view.columns), (2, 2), "the tiles form a grid");
    assert_eq!(view.channels.len(), 1, "the tiles name one channel");
}

#[test]
fn a_cancelled_stitch_removes_its_staging() {
    let width = 3 * TILE - 2 * OVERLAP;
    let canvas = (0..width * width * DEPTH as usize)
        .map(|index| pattern(index % width, (index / width) % width))
        .collect::<Vec<_>>();
    let tiles = (0..9)
        .map(|index| {
            memory_tile(
                &canvas,
                width,
                (index % 3) * (TILE - OVERLAP),
                (index / 3) * (TILE - OVERLAP),
            )
        })
        .collect::<Vec<_>>();
    let output = std::env::temp_dir().join(format!("grana-stitch-cancel-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&output);
    let cancel = Cancel::new();
    let token = cancel.clone();
    let request = Request {
        tiles: tiles.clone(),
        output: output.clone(),
        rows: 3,
        columns: 3,
        channel: 0,
        series: 0,
    };
    let worker = std::thread::spawn(move || stitch(&request, &cancel).is_err());
    let mut seen = false;
    for _ in 0..2000 {
        if staging(&output) > 0 {
            seen = true;
            token.trigger();
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let failed = worker.join().expect("the stitcher finishes");
    assert!(seen, "the stitch builds a staging entry before it commits");
    assert!(failed, "a cancelled stitch reports an error");
    assert!(!output.exists(), "a cancelled stitch leaves no mosaic");
    assert_eq!(
        staging(&output),
        0,
        "a cancelled stitch deletes its staging entry"
    );
    let request = Request {
        tiles,
        output: output.clone(),
        rows: 3,
        columns: 3,
        channel: 0,
        series: 0,
    };
    let cancelled = Cancel::new();
    cancelled.trigger();
    assert!(
        stitch(&request, &cancelled).is_err(),
        "a stitch cancelled before it starts reports an error"
    );
    assert!(
        !output.exists(),
        "a stitch cancelled before it starts writes nothing"
    );
}

fn staging(output: &std::path::Path) -> usize {
    let name = output
        .file_name()
        .expect("the output has a name")
        .to_string_lossy()
        .into_owned();
    let prefix = format!(".{name}.");
    std::fs::read_dir(output.parent().expect("the output has a parent"))
        .expect("the temporary folder lists")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(&prefix))
        .count()
}

fn memory_tile(values: &[f32], width: usize, x: usize, y: usize) -> Source {
    let mut data = Vec::with_capacity(TILE * TILE * DEPTH as usize);
    for z in 0..DEPTH as usize {
        for row in 0..TILE {
            for column in 0..TILE {
                data.push(values[(column + x) + width * (row + y) + width * width * z]);
            }
        }
    }
    let extent = Extent::new(TILE as u32, TILE as u32, DEPTH);
    let channel = Channel::new("structure", [0.2, 0.8, 0.4], VoxelData::F32(data));
    let volume = Volume::new(extent, Spacing::new(0.5, 0.5, 1.0), vec![channel]);
    Source::memory(VolumeSeries::new(vec![volume], 1.0))
}

fn pattern(x: usize, y: usize) -> f32 {
    noise(x as u64, 0x9E37_79B9_7F4A_7C15) + 0.5 * noise(y as u64, 0xC2B2_AE3D_27D4_EB4F)
}

fn noise(index: u64, salt: u64) -> f32 {
    let mut value = index.wrapping_mul(salt) ^ salt.rotate_left(17);
    value ^= value >> 29;
    value = value.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value ^= value >> 32;
    (value & 0xffff) as f32
}
