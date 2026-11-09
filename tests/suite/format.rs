use std::path::PathBuf;

use grana_image::{Format, Image, Source};
use grana_volume::{Basis, Extent, Intensity, Level, Spacing, VoxelData};
use grana_zarr::{ImageSpec, WriteChannel, write_image};

use crate::support;
use crate::support::Temp;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn write_zarr(root: &std::path::Path) {
    let levels = [
        Level::new(Extent::new(24, 16, 8), Spacing::new(0.5, 0.5, 1.0)),
        Level::new(Extent::new(12, 8, 4), Spacing::new(1.0, 1.0, 2.0)),
    ];
    let channels = [
        WriteChannel {
            name: "DAPI",
            tint: [0.0, 0.0, 1.0],
            range: Intensity::new(0.0, 2048.0),
        },
        WriteChannel {
            name: "GFP",
            tint: [0.0, 1.0, 0.0],
            range: Intensity::new(0.0, 1024.0),
        },
    ];
    let mut fill = |level, frame, channel, origin: [u32; 3], extent: [u32; 3], data: &mut [f32]| {
        let scale = 1u32 << level;
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
    write_image(
        root,
        ImageSpec {
            name: "fixture",
            channels: &channels,
            levels: &levels,
            placement: [1.5, -2.0, 3.0],
            frames: 2,
            interval: 1.5,
            fill: &mut fill,
            progress: &mut progress,
        },
    )
    .expect("the tiny image writes");
}

fn sample(channel: usize, frame: u32, position: [u32; 3]) -> u32 {
    channel as u32 * 100_000 + frame * 10_000 + position[2] * 500 + position[1] * 10 + position[0]
}

#[test]
fn writes_and_reads_a_tiny_ome_zarr() {
    let temp = Temp::new("suite-zarr");
    let path = temp.path().join("tiny.ome.zarr");
    write_zarr(&path);
    let image = grana_zarr::Image::open(&path).expect("the written image opens");
    let pyramid = image.pyramid();
    assert_eq!(pyramid.channel_count(), 2);
    assert_eq!(pyramid.frame_count(), 2);
    assert_eq!(pyramid.frame_interval(), 1.5);
    assert_eq!(pyramid.level_count(), 2);
    assert_eq!(pyramid.channels()[0].name(), "DAPI");
    assert_eq!(pyramid.channels()[0].tint(), [0.0, 0.0, 1.0]);
    assert_eq!(pyramid.channels()[0].window(), Intensity::new(0.0, 2048.0));
    assert_eq!(pyramid.level(0).extent(), Extent::new(24, 16, 8));
    assert_eq!(pyramid.level(0).spacing(), Spacing::new(0.5, 0.5, 1.0));
    assert_eq!(pyramid.level(1).extent(), Extent::new(12, 8, 4));
    assert_eq!(pyramid.level(1).spacing(), Spacing::new(1.0, 1.0, 2.0));

    let data = image
        .read_region(1, 0, 1, 2..6, 1..4, 0..2)
        .expect("the written region reads");
    let VoxelData::F32(values) = data else {
        panic!("the written image stores float voxels");
    };
    assert_eq!(values.len(), 4 * 3 * 2);
    for z in 0..2u32 {
        for y in 1..4u32 {
            for x in 2..6u32 {
                let index = ((z * 3 + (y - 1)) * 4 + (x - 2)) as usize;
                assert_eq!(
                    values[index],
                    sample(1, 1, [x, y, z]) as f32,
                    "voxel {x},{y},{z}"
                );
            }
        }
    }
}

#[test]
fn round_trips_a_rotated_placement() {
    let temp = Temp::new("suite-zarr-rotated");
    let path = temp.path().join("rotated.ome.zarr");
    let level = Level::of(
        Extent::new(16, 12, 6),
        Basis::new([[0.0, 0.5, 0.0], [-0.5, 0.0, 0.0], [0.0, 0.0, 1.0]]),
    );
    let channels = [WriteChannel {
        name: "DAPI",
        tint: [0.0, 0.0, 1.0],
        range: Intensity::new(0.0, 255.0),
    }];
    let mut fill = |_level: usize,
                    _frame: u32,
                    _channel: usize,
                    _origin: [u32; 3],
                    _extent: [u32; 3],
                    data: &mut [f32]|
     -> Result<(), String> {
        data.fill(200.0);
        Ok(())
    };
    let mut progress = |_, _| {};
    write_image(
        &path,
        ImageSpec {
            name: "rotated",
            channels: &channels,
            levels: &[level],
            placement: [0.0; 3],
            frames: 1,
            interval: 1.0,
            fill: &mut fill,
            progress: &mut progress,
        },
    )
    .expect("the rotated image writes");
    let image = grana_zarr::Image::open(&path).expect("the rotated image opens");
    let read = image.pyramid().level(0);
    for axis in 0..3 {
        for component in 0..3 {
            let expected = level.basis().column(axis)[component];
            let written = read.basis().column(axis)[component];
            assert!(
                (expected - written).abs() <= 1.0e-5,
                "the placement lost its axis {axis} component {component}: {expected} against {written}"
            );
        }
    }
    for (expected, written) in level.center().into_iter().zip(read.center()) {
        assert!(
            (expected - written).abs() <= 1.0e-4,
            "the placement lost its centre: {expected} against {written}"
        );
    }
    let sheared = Level::of(
        Extent::new(8, 8, 4),
        Basis::new([[0.5, 0.0, 0.0], [0.25, 0.5, 0.0], [0.0, 0.0, 1.0]]),
    );
    let mut fill = |_: usize,
                    _: u32,
                    _: usize,
                    _: [u32; 3],
                    _: [u32; 3],
                    _: &mut [f32]|
     -> Result<(), String> { Ok(()) };
    let mut progress = |_, _| {};
    let rejected = write_image(
        temp.path().join("sheared.ome.zarr").as_path(),
        ImageSpec {
            name: "sheared",
            channels: &channels,
            levels: &[sheared],
            placement: [0.0; 3],
            frames: 1,
            interval: 1.0,
            fill: &mut fill,
            progress: &mut progress,
        },
    );
    assert!(
        rejected.is_err(),
        "a sheared placement is refused instead of written"
    );
}

#[test]
fn opens_a_plane_tiff_through_the_image_layer() {
    let image = Image::open(&Source::file(fixture("plane.tif"))).expect("the tiff opens");
    assert_eq!(image.format(), Some(Format::Tiff));
    assert_eq!(image.name(), "plane");
    assert_eq!(image.pyramid().level(0).extent(), Extent::new(4, 3, 1));
    let data = image
        .read_region(0, 0, 0, &[0..4, 0..3, 0..1], [0.0; 3])
        .expect("the plane reads");
    let VoxelData::U8(values) = data else {
        panic!("the plane stores bytes");
    };
    for y in 0..3u32 {
        for x in 0..4u32 {
            assert_eq!(values[(y * 4 + x) as usize], (x + 10 * y) as u8);
        }
    }
}

#[test]
fn reads_ome_tiff_metadata_and_voxels() {
    let image = grana_tiff::Image::open(&fixture("ome.tif")).expect("the ome tiff opens");
    let pyramid = image.pyramid();
    assert_eq!(pyramid.channel_count(), 1);
    assert_eq!(pyramid.level_count(), 1);
    assert_eq!(pyramid.frame_interval(), 1.5);
    assert_eq!(pyramid.level(0).extent(), Extent::new(32, 32, 2));
    assert_eq!(pyramid.level(0).spacing(), Spacing::new(0.2, 0.2, 1.0));
    assert_eq!(pyramid.channels()[0].name(), "GFP");
    assert_eq!(pyramid.channels()[0].tint(), [0.0, 1.0, 0.0]);
    let data = image
        .read_region(0, 0, 0, 0..32, 0..32, 0..2)
        .expect("the ome volume reads");
    let VoxelData::U8(values) = data else {
        panic!("the ome tiff stores bytes");
    };
    for z in 0..2u32 {
        for y in 0..32u32 {
            for x in 0..32u32 {
                let index = ((z * 32 + y) * 32 + x) as usize;
                assert_eq!(
                    u32::from(values[index]),
                    (z * 97 + y * 5 + x * 3) % 256,
                    "voxel {x},{y},{z}"
                );
            }
        }
    }
}

#[test]
fn browses_an_ome_zarr_plate() {
    let temp = Temp::new("suite-plate");
    let root = temp.path().join("assay.ome.zarr");
    support::plate(&root);
    assert!(grana_image::is_image(&root), "a plate folder is a dataset");
    assert_eq!(
        grana_image::series_count(&root).expect("the plate counts its fields"),
        2
    );
    let names = grana_image::series_names(&root).expect("the plate names its fields");
    assert_eq!(
        names,
        vec![Some("A1/0".to_string()), Some("A2/1".to_string())]
    );
    assert_eq!(
        grana_image::series_group(&root, 1).expect("the second field resolves"),
        Some("/A/2/1".to_string())
    );
    let zarr = grana_zarr::Root::local(&root).expect("the plate opens");
    let plate = grana_zarr::open_plate(&zarr).expect("the plate metadata parses");
    assert_eq!((plate.rows(), plate.columns()), (1, 2));
    let well = plate.well(0, 1).expect("the second column holds a well");
    assert_eq!(well.label(), "A2");
    assert_eq!(well.fields().len(), 1);
    assert_eq!(plate.fields()[1].series(), 1);
    assert_eq!(plate.well_of(1).map(grana_zarr::Well::label), Some("A2"));
    let browse = grana_image::plate(&Source::file(root.clone()))
        .expect("the image layer reads the plate")
        .expect("the folder is a plate");
    assert_eq!(browse.len(), plate.len());
    assert_eq!(browse.wells().len(), plate.wells().len());
    let resolved = grana_image::resolve_series(&Source::file_series(root, 1))
        .expect("the second field resolves");
    assert_eq!(resolved.group(), Some("/A/2/1"));
    let image = Image::open(&resolved).expect("the second field opens");
    assert_eq!(image.pyramid().level(0).extent(), Extent::new(12, 10, 6));
}
