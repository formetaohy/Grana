use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use grana_image::{Format, Image, Source, is_image};
use grana_volume::{
    BrickCoord, Channel, Extent, PADDED_SIDE, Spacing, Volume, VolumeSeries, VoxelData,
};

#[test]
fn opens_a_tiff_file_through_the_source_layer() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/plane.tif");
    assert!(is_image(&path));
    let image = Image::open(&Source::file(path)).expect("the tiff opens");
    assert_eq!(image.format(), Some(Format::Tiff));
    assert_eq!(image.name(), "plane");
    assert_eq!(image.pyramid().level(0).extent(), Extent::new(4, 3, 1));
    let data = image
        .read_region(0, 0, 0, &[0..4, 0..3, 0..1], [0.0; 3])
        .expect("the plane reads");
    let VoxelData::U8(values) = data else {
        panic!("the fixture stores bytes");
    };
    for y in 0..3u32 {
        for x in 0..4u32 {
            assert_eq!(values[(y * 4 + x) as usize], (x + 10 * y) as u8);
        }
    }
}

#[test]
fn shifts_a_read_within_the_level() {
    let extent = Extent::new(8, 8, 2);
    let mut samples = Vec::with_capacity(extent.voxel_count() as usize);
    for z in 0..extent.z() {
        for y in 0..extent.y() {
            for x in 0..extent.x() {
                samples.push((x + 10 * y + 100 * z) as u16);
            }
        }
    }
    let volume = Volume::new(
        extent,
        Spacing::new(1.0, 1.0, 1.0),
        vec![Channel::new(
            "signal",
            [1.0, 0.0, 0.0],
            VoxelData::U16(samples),
        )],
    );
    let image = Image::open(&Source::memory(VolumeSeries::new(vec![volume], 1.0)))
        .expect("the synthetic series opens");
    let ranges = [0..4, 0..1, 0..1];
    assert_eq!(
        image
            .read_region(0, 0, 0, &ranges, [2.0, 0.0, 0.0])
            .expect("the shifted read"),
        VoxelData::U16(vec![2, 3, 4, 5])
    );
    assert_eq!(
        image
            .read_region(0, 0, 0, &ranges, [-1.0, 0.0, 0.0])
            .expect("the clipped read"),
        VoxelData::U16(vec![0, 0, 1, 2])
    );
    assert_eq!(
        image
            .read_region(0, 0, 0, &ranges, [8.0, 0.0, 0.0])
            .expect("the empty read"),
        VoxelData::U16(vec![0, 0, 0, 0])
    );
    let brick = image
        .read_brick(0, 0, BrickCoord::new(0, 0, 0), [1.0, 0.0, 0.0])
        .expect("the shifted brick reads");
    let side = PADDED_SIDE as usize;
    let VoxelData::U16(padded) = &brick.channels()[0] else {
        panic!("a brick keeps the voxel format");
    };
    assert_eq!(padded[1], 1);
    assert_eq!(padded[side * side * side - 1], 0);
}

#[test]
fn resolves_coarser_levels_from_the_stored_level() {
    let extent = Extent::new(64, 64, 8);
    let mut samples = Vec::with_capacity(extent.voxel_count() as usize);
    for z in 0..extent.z() {
        for y in 0..extent.y() {
            for x in 0..extent.x() {
                samples.push((4 * x + 8 * y + 16 * z) as u16);
            }
        }
    }
    let volume = Volume::new(
        extent,
        Spacing::new(1.0, 1.0, 1.0),
        vec![Channel::new(
            "signal",
            [1.0, 0.0, 0.0],
            VoxelData::U16(samples),
        )],
    );
    let image = Image::open(&Source::memory(VolumeSeries::new(vec![volume], 1.0)))
        .expect("the synthetic series opens");
    assert_eq!(image.pyramid().stored(), 1);
    assert_eq!(image.pyramid().level_count(), 2);
    assert_eq!(image.pyramid().level(1).extent(), Extent::new(32, 32, 4));
    let resolved = image
        .read_region(0, 1, 0, &[0..32, 0..32, 0..4], [0.0; 3])
        .expect("the resolved level reads");
    let VoxelData::U16(values) = resolved else {
        panic!("the resolved level keeps the voxel format");
    };
    for z in 0..4u32 {
        for y in 0..32u32 {
            for x in 0..32u32 {
                let expected = (8 * x + 16 * y + 32 * z + 14) as u16;
                assert_eq!(values[(x + 32 * (y + 32 * z)) as usize], expected);
            }
        }
    }
    let brick = image
        .read_brick(0, 1, BrickCoord::new(0, 0, 0), [0.0; 3])
        .expect("the resolved brick reads");
    let VoxelData::U16(samples) = &brick.channels()[0] else {
        panic!("the resolved brick keeps the voxel format");
    };
    let side = PADDED_SIDE as usize;
    assert_eq!(samples.len(), side * side * side);
    assert_eq!(samples[side * (1 + side)], 14);
    assert_eq!(samples[side * side * side - 1], 854);
    assert!(
        image
            .read_region(0, 2, 0, &[0..1, 0..1, 0..1], [0.0; 3])
            .is_err()
    );
}

#[test]
fn persists_resolved_tiles_across_sessions() {
    let temporary = Temporary::new("grana-image-pyramid");
    let dataset = temporary.path().join("fixture.zarr");
    write_zarr(&dataset);
    let source = Source::file(dataset.clone());
    let cache = temporary.path().join("cache");

    let first = Image::open_with_cache(&source, Some(cache.clone())).expect("the fixture opens");
    let region = [40..104, 10..60, 0..4];
    let resolved = first
        .read_region(0, 1, 0, &region, [0.0; 3])
        .expect("the resolved region reads");
    let brick = first
        .read_brick(0, 1, BrickCoord::new(1, 1, 0), [0.0; 3])
        .expect("the resolved brick reads");
    let VoxelData::U16(values) = &resolved else {
        panic!("the resolved level keeps the voxel format");
    };
    assert_eq!(values[0], 4 * 40 + 8 * 10 + 7);
    let side = PADDED_SIDE as usize;
    let VoxelData::U16(samples) = &brick.channels()[0] else {
        panic!("the resolved brick keeps the voxel format");
    };
    assert_eq!(samples[1 + side * (1 + side)], 4 * 32 + 8 * 32 + 7);
    drop(first);

    let second = Image::open_with_cache(&source, Some(cache.clone())).expect("the fixture reopens");
    fs::remove_dir_all(&dataset).expect("the fixture is removed");
    assert_eq!(
        second
            .read_region(0, 1, 0, &region, [0.0; 3])
            .expect("the cached region reads"),
        resolved,
        "the second session reads the persisted tiles"
    );
    assert_eq!(
        second
            .read_brick(0, 1, BrickCoord::new(1, 1, 0), [0.0; 3])
            .expect("the cached brick reads"),
        brick,
        "the second session assembles the persisted bricks"
    );
    assert!(
        entries(&cache) > 0,
        "the resolved tiles are written to the cache directory"
    );
    drop(second);

    write_zarr(&dataset);
    let source = Source::file(dataset.clone());
    let uncached = temporary.path().join("uncached");
    let third = Image::open_with_cache(&source, Some(uncached)).expect("the fixture reopens");
    fs::remove_dir_all(&dataset).expect("the fixture is removed again");
    let recovered = third.read_region(0, 1, 0, &region, [0.0; 3]).ok();
    assert!(
        recovered.as_ref() != Some(&resolved),
        "an empty cache cannot recover the removed source"
    );
}

#[test]
fn composes_small_resolved_regions_without_caching_whole_tiles() {
    let temporary = Temporary::new("grana-image-partial");
    let dataset = temporary.path().join("fixture.zarr");
    write_zarr(&dataset);
    let cache = temporary.path().join("cache");
    let image = Image::open_with_cache(&Source::file(dataset), Some(cache.clone()))
        .expect("the fixture opens");
    let region = [4..6, 4..6, 0..1];
    let small = image
        .read_region(0, 1, 0, &region, [0.0; 3])
        .expect("the small region reads");
    assert_eq!(small, VoxelData::U16(vec![55, 59, 63, 67]));
    assert_eq!(
        entries(&cache),
        0,
        "a small region is composed without caching whole tiles"
    );
    let whole = image
        .read_region(0, 1, 0, &[0..128, 0..64, 0..4], [0.0; 3])
        .expect("the whole level reads");
    let VoxelData::U16(samples) = &whole else {
        panic!("the resolved level keeps the voxel format");
    };
    assert_eq!(samples[4 + 128 * 4], 55);
    assert_eq!(samples[5 + 128 * 4], 59);
    assert_eq!(samples[4 + 128 * 5], 63);
    assert_eq!(samples[5 + 128 * 5], 67);
    assert!(entries(&cache) > 0, "a whole region caches its tiles");
}

fn entries(root: &Path) -> usize {
    let Ok(directories) = fs::read_dir(root) else {
        return 0;
    };
    directories
        .flatten()
        .filter_map(|directory| fs::read_dir(directory.path()).ok())
        .map(|files| files.flatten().count())
        .sum()
}

fn write_zarr(root: &Path) {
    let (width, height, depth) = (256u32, 128u32, 8u32);
    write(root.join(".zgroup"), br#"{"zarr_format":2}"#);
    write(
        root.join(".zattrs"),
        br#"{
            "multiscales": [{
                "version": "0.4",
                "name": "fixture",
                "axes": [
                    {"name": "t", "type": "time", "unit": "second"},
                    {"name": "c", "type": "channel"},
                    {"name": "z", "type": "space", "unit": "micrometer"},
                    {"name": "y", "type": "space", "unit": "micrometer"},
                    {"name": "x", "type": "space", "unit": "micrometer"}
                ],
                "datasets": [
                    {"path": "0", "coordinateTransformations": [{"type": "scale", "scale": [1.0, 1.0, 1.0, 1.0, 1.0]}]}
                ]
            }],
            "omero": {
                "channels": [
                    {"color": "FF0000", "window": {"min": 0, "max": 65535, "start": 0, "end": 65535}, "label": "signal"}
                ]
            }
        }"#,
    );
    write(
        root.join("0").join(".zarray"),
        format!(
            r#"{{"zarr_format":2,"shape":[1,1,{depth},{height},{width}],"chunks":[1,1,{depth},{height},{width}],"dtype":"<u2","compressor":null,"fill_value":0,"filters":null,"order":"C","dimension_separator":"/"}}"#
        )
        .as_bytes(),
    );
    let mut data = Vec::with_capacity((width * height * depth * 2) as usize);
    for z in 0..depth {
        for y in 0..height {
            for x in 0..width {
                let value = (2 * x + 4 * y + 8 * z) as u16;
                data.extend_from_slice(&value.to_le_bytes());
            }
        }
    }
    write(
        root.join("0")
            .join("0")
            .join("0")
            .join("0")
            .join("0")
            .join("0"),
        &data,
    );
}

fn write(path: PathBuf, content: &[u8]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("the fixture directories are created");
    }
    fs::write(path, content).expect("the fixture files are written");
}

struct Temporary {
    path: PathBuf,
}

impl Temporary {
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

impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
