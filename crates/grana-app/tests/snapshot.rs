use std::path::PathBuf;

use grana_app::Snapshot;
use grana_image::{Image, Source};
use grana_scene::{Dataset, Scene};
use grana_session::{SnapshotRequest, Snapshotter};
use grana_volume::{Channel, Extent, Spacing, Volume, VolumeSeries, VoxelData};

#[test]
fn renders_a_snapshot_without_a_window() {
    if grana_gpu::Headless::new().is_err() {
        println!("no graphics adapter is available, skipping");
        return;
    }
    let reader = reader();
    let scene = Scene::new(Dataset::synthetic("cells", reader.pyramid().clone()));
    let path = scratch("snapshot").join("snapshot.png");
    let request = SnapshotRequest {
        view: None,
        size: [320, 200],
        transparent: false,
        time: 0,
    };
    Snapshot::new()
        .snapshot(&reader, &scene, &request, &path)
        .expect("the snapshot renders");
    let captured = decode(&path);
    assert_eq!(captured.size, [320, 200]);
    assert_eq!(
        captured.corner_alpha(),
        255,
        "the snapshot keeps a backdrop"
    );
    assert!(
        captured.lit() > 0,
        "the snapshot shows the channel of the dataset"
    );
}

fn reader() -> Image {
    let extent = Extent::new(48, 48, 24);
    let mut voxels = vec![0u16; extent.voxel_count() as usize];
    for z in 8..16 {
        for y in 16..32 {
            for x in 16..32 {
                let index = x + 48 * (y + 48 * z);
                voxels[index] = 60_000;
            }
        }
    }
    let volume = Volume::new(
        extent,
        Spacing::new(0.1, 0.1, 0.3),
        vec![Channel::new(
            "Signal",
            [0.3, 0.7, 1.0],
            VoxelData::U16(voxels),
        )],
    );
    let series = VolumeSeries::new(vec![volume], 1.0);
    Image::open(&Source::memory(series)).expect("the in-memory volume opens")
}

struct Decoded {
    size: [u32; 2],
    pixels: Vec<u8>,
}

impl Decoded {
    fn corner_alpha(&self) -> u8 {
        self.pixels[3]
    }

    fn lit(&self) -> usize {
        self.pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pixel| {
                pixel[..3]
                    .iter()
                    .map(|value| u32::from(*value))
                    .sum::<u32>()
                    > 200
            })
            .count()
    }
}

fn decode(path: &std::path::Path) -> Decoded {
    let raster = grana_raster::read(path).expect("the snapshot reads");
    Decoded {
        size: raster.size(),
        pixels: raster.pixels().to_vec(),
    }
}

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("grana-snapshot-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("the scratch folder is created");
    path
}
