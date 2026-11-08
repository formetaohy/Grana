use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use grana_volume::{Extent, Intensity, Level, Spacing, VoxelData, VoxelFormat};
use grana_zarr::{Image, ImageSpec, WriteChannel, write_image};

#[test]
fn writes_an_ome_zarr_image_that_reads_back() {
    let temp = TempDir::new("grana-zarr-write");
    let path = temp.path().join("written.ome.zarr");
    let levels = [
        Level::new(Extent::new(300, 40, 12), Spacing::new(0.5, 0.5, 1.0)),
        Level::new(Extent::new(150, 20, 6), Spacing::new(1.0, 1.0, 2.0)),
    ];
    let name = "written";
    let channels = [
        WriteChannel {
            name: "DAPI",
            tint: [0.0, 0.0, 1.0],
            range: Intensity::new(0.0, 2048.0),
        },
        WriteChannel {
            name: "masked",
            tint: [0.0, 1.0, 0.0],
            range: Intensity::new(0.0, 512.0),
        },
    ];
    let mut fill = |level, frame, channel, origin: [u32; 3], extent: [u32; 3], data: &mut [f32]| {
        let scale = (1 << level) as u32;
        for z in 0..extent[2] {
            for y in 0..extent[1] {
                for x in 0..extent[0] {
                    let index = x as usize
                        + extent[0] as usize * (y as usize + extent[1] as usize * z as usize);
                    let position = [
                        (origin[0] + x) * scale,
                        (origin[1] + y) * scale,
                        (origin[2] + z) * scale,
                    ];
                    data[index] = sample(channel, frame, position) as f32;
                }
            }
        }
        Ok(())
    };
    let mut progress = |_, _| {};
    let placement = [12.0, -3.5, 40.0];
    let written = write_image(
        &path,
        ImageSpec {
            name,
            channels: &channels,
            levels: &levels,
            placement,
            frames: 2,
            interval: 3.0,
            fill: &mut fill,
            progress: &mut progress,
        },
    )
    .expect("the dataset writes");
    assert_eq!(written.channels(), 2);
    assert_eq!(written.frames(), 2);
    assert_eq!(written.levels(), 2);
    assert_eq!(written.path(), path);
    assert!(written.bytes() > 0);

    let image = Image::open(&path).expect("the written dataset opens");
    assert_eq!(image.name(), name);
    let pyramid = image.pyramid();
    assert_eq!(pyramid.channel_count(), 2);
    assert_eq!(pyramid.frame_count(), 2);
    assert_eq!(pyramid.frame_interval(), 3.0);
    assert_eq!(pyramid.channels()[0].name(), "DAPI");
    assert_eq!(pyramid.channels()[0].tint(), [0.0, 0.0, 1.0]);
    assert_eq!(pyramid.channels()[0].format(), VoxelFormat::F32);
    assert_eq!(pyramid.channels()[0].window(), Intensity::new(0.0, 2048.0));
    assert_eq!(pyramid.channels()[1].name(), "masked");
    assert_eq!(pyramid.channels()[1].tint(), [0.0, 1.0, 0.0]);
    assert_eq!(pyramid.level_count(), 2);
    assert_eq!(pyramid.level(0).extent(), Extent::new(300, 40, 12));
    assert_eq!(pyramid.level(0).spacing(), Spacing::new(0.5, 0.5, 1.0));
    assert_eq!(pyramid.level(1).extent(), Extent::new(150, 20, 6));
    assert_eq!(pyramid.level(1).spacing(), Spacing::new(1.0, 1.0, 2.0));
    for (first, second) in pyramid.placement().iter().zip(placement) {
        assert!((first - second).abs() < 1.0e-3);
    }
    for (index, level) in levels.iter().enumerate() {
        let read = pyramid.level(index);
        assert_eq!(read.extent(), level.extent());
        assert_eq!(read.spacing(), level.spacing());
        assert!(
            read.origin()
                .iter()
                .zip(level.origin())
                .all(|(first, second)| (first - second).abs() < 1.0e-3)
        );
    }

    let x = 250..260u32;
    let y = 4..7u32;
    let z = 2..4u32;
    let region = image
        .read_region(1, 0, 1, x.clone(), y.clone(), z.clone())
        .expect("the region crossing a chunk boundary reads");
    let VoxelData::F32(values) = &region else {
        panic!("the written dataset stores float voxels");
    };
    for (depth, plane) in z.enumerate() {
        for (row, line) in y.clone().enumerate() {
            for (column, value) in x.clone().enumerate() {
                let index = column
                    + (x.end - x.start) as usize * (row + (y.end - y.start) as usize * depth);
                assert_eq!(
                    values[index],
                    sample(1, 1, [value, line, plane]) as f32,
                    "voxel ({value}, {line}, {plane})"
                );
            }
        }
    }
}

fn sample(channel: usize, frame: u32, position: [u32; 3]) -> u32 {
    (channel as u32) * 1_000_000
        + frame * 100_000
        + position[2] * 1_000
        + position[1] * 10
        + position[0]
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
