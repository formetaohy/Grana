use std::path::PathBuf;

use grana_image::{Format, Image, Source};
use grana_interop::{Detection, Edge, Lineage, geff};
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

#[test]
fn writes_a_sixteen_bit_ome_tiff() {
    let temp = Temp::new("suite-tiff-write");
    let path = temp.path().join("picture.ome.tif");
    let size = [5u32, 4];
    let mut pixels = vec![0u16; size[0] as usize * size[1] as usize * 3];
    for (index, value) in pixels.iter_mut().enumerate() {
        *value = (index as u16).wrapping_mul(1237).wrapping_add(3);
    }
    grana_tiff::write(
        &path,
        &grana_tiff::Picture {
            size,
            samples: 3,
            pixels: &pixels,
            name: "figure & frame",
            physical: Some([0.5, 0.5]),
            interval: Some(2.0),
        },
    )
    .expect("the picture writes");
    let bytes = std::fs::read(&path).expect("the picture reads as bytes");
    let metadata = String::from_utf8_lossy(&bytes);
    assert!(
        metadata.contains("PhysicalSizeX=\"0.5\"") && metadata.contains("PhysicalSizeY=\"0.5\""),
        "the picture records its physical pixel size"
    );
    assert!(
        metadata.contains("TimeIncrement=\"2\""),
        "the picture records its frame interval"
    );
    let image = grana_tiff::Image::open(&path).expect("the written picture opens");
    assert_eq!(image.name(), "figure & frame");
    let pyramid = image.pyramid();
    assert_eq!(pyramid.channel_count(), 3);
    assert_eq!(pyramid.frame_count(), 1);
    assert_eq!(pyramid.level(0).extent(), Extent::new(5, 4, 1));
    assert_eq!(pyramid.channels()[0].name(), "Red");
    assert_eq!(pyramid.channels()[1].tint(), [0.0, 1.0, 0.0]);
    for channel in 0..3 {
        let data = image
            .read_region(0, 0, channel, 0..5, 0..4, 0..1)
            .expect("the written plane reads");
        let VoxelData::U16(values) = data else {
            panic!("the written picture stores sixteen bit voxels");
        };
        for y in 0..4u32 {
            for x in 0..5u32 {
                let index = (x + 5 * y) as usize;
                assert_eq!(
                    values[index],
                    pixels[index * 3 + channel],
                    "texel {x},{y} of channel {channel}"
                );
            }
        }
    }
}

#[test]
fn writes_an_alpha_channel_plane() {
    let temp = Temp::new("suite-tiff-alpha");
    let path = temp.path().join("picture.ome.tif");
    let size = [3u32, 2];
    let mut pixels = vec![0u16; size[0] as usize * size[1] as usize * 4];
    for (index, value) in pixels.iter_mut().enumerate() {
        *value = 1000 + index as u16;
    }
    grana_tiff::write(
        &path,
        &grana_tiff::Picture {
            size,
            samples: 4,
            pixels: &pixels,
            name: "transparent",
            physical: None,
            interval: None,
        },
    )
    .expect("the transparent picture writes");
    let image = grana_tiff::Image::open(&path).expect("the transparent picture opens");
    let pyramid = image.pyramid();
    assert_eq!(pyramid.channel_count(), 4);
    assert_eq!(pyramid.channels()[3].name(), "Alpha");
    let data = image
        .read_region(0, 0, 3, 0..3, 0..2, 0..1)
        .expect("the alpha plane reads");
    let VoxelData::U16(values) = data else {
        panic!("the alpha plane stores sixteen bit voxels");
    };
    for (index, value) in values.iter().enumerate() {
        assert_eq!(*value, pixels[index * 4 + 3]);
    }
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
    let sheared_path = temp.path().join("sheared.ome.zarr");
    write_image(
        &sheared_path,
        ImageSpec {
            name: "sheared",
            channels: &channels,
            levels: &[sheared],
            placement: [1.5, -2.0, 3.0],
            frames: 1,
            interval: 1.0,
            fill: &mut fill,
            progress: &mut progress,
        },
    )
    .expect("the sheared image writes");
    let metadata: serde_json::Value = serde_json::from_slice(
        &std::fs::read(sheared_path.join("zarr.json")).expect("the sheared metadata reads"),
    )
    .expect("the sheared metadata parses");
    assert_eq!(
        metadata["attributes"]["ome"]["multiscales"][0]["coordinateTransformations"][0]["transformations"]
            [1]["type"],
        "affine",
        "a sheared placement is written as an affine transform"
    );
    let sheared_image = grana_zarr::Image::open(&sheared_path).expect("the sheared image opens");
    let written = sheared_image.pyramid().level(0);
    for axis in 0..3 {
        for component in 0..3 {
            let expected = sheared.basis().column(axis)[component];
            let actual = written.basis().column(axis)[component];
            assert!(
                (expected - actual).abs() <= 1.0e-5,
                "the shear lost its axis {axis} component {component}: {expected} against {actual}"
            );
        }
    }
    for (expected, actual) in [1.5, -2.0, 3.0]
        .into_iter()
        .zip(sheared_image.pyramid().placement())
    {
        assert!(
            (expected - actual).abs() <= 1.0e-4,
            "the shear lost its placement: {expected} against {actual}"
        );
    }
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
    assert_eq!(plate.row_name(0), Some("A"));
    assert_eq!(plate.column_name(1), Some("2"));
    let well = plate.well(0, 1).expect("the second column holds a well");
    assert_eq!(well.label(), "A2");
    assert_eq!(well.fields().len(), 1);
    assert_eq!(plate.fields()[1].series(), 1);
    assert_eq!(
        (plate.fields()[1].row(), plate.fields()[1].column()),
        (0, 1)
    );
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

#[test]
fn writes_and_reads_a_label_image() {
    let temp = Temp::new("suite-zarr-label-write");
    let path = temp.path().join("labelled.ome.zarr");
    write_zarr(&path);
    let mut fill = |_level: usize,
                    _frame: u32,
                    _origin: [u32; 3],
                    _extent: [u32; 3],
                    data: &mut [u16]|
     -> Result<(), String> {
        data.fill(3);
        Ok(())
    };
    let mut progress = |_, _| {};
    grana_zarr::write_group_labels(
        &path,
        "/",
        grana_zarr::LabelSpec {
            name: "cells",
            frames: &[0, 1],
            labels: 3,
            color: [1.0, 0.0, 0.0],
            opacity: 1.0,
            properties: &[],
            fill: &mut fill,
            progress: &mut progress,
        },
    )
    .expect("the labels write");
    let image = grana_zarr::Image::open(&path).expect("the labelled image opens");
    let labels = image.labels().expect("the labels read");
    let label = labels.get("cells").expect("the written label exists");
    assert_eq!(label.frames(), 2);
    let data = label
        .read(1, 0, &[0..2, 0..2, 0..1], [0, 0, 0])
        .expect("the written label region reads");
    assert_eq!(data.len(), 4);
    assert!(data.iter().all(|value| *value == 3), "the labels hold 3");
}

#[test]
fn opens_permuted_axes_and_inherits_label_placement() {
    let temp = Temp::new("suite-zarr-axes");
    let path = temp.path().join("ordered.ome.zarr");
    support::ordered_field(&path);
    let image = grana_zarr::Image::open(&path).expect("the ordered image opens");
    let pyramid = image.pyramid();
    assert_eq!(pyramid.level(0).extent(), Extent::new(4, 3, 2));
    assert_eq!(pyramid.level(0).spacing(), Spacing::new(1.5, 0.5, 2.0));
    assert_eq!(pyramid.channel_count(), 2);
    assert_eq!(pyramid.frame_count(), 1);
    let data = image
        .read_region(0, 0, 1, 0..4, 0..3, 0..2)
        .expect("the ordered region reads");
    let VoxelData::U8(values) = data else {
        panic!("the ordered image stores bytes");
    };
    for x in 0..4u32 {
        for y in 0..3u32 {
            for z in 0..2u32 {
                let index = (x as usize * 3 + y as usize) * 2 + z as usize;
                assert_eq!(
                    u32::from(values[index]),
                    100 + x * 10 + y + z * 5,
                    "voxel {x},{y},{z}"
                );
            }
        }
    }
    let labels = image.labels().expect("the ordered labels read");
    let label = labels.get("cells").expect("the ordered label exists");
    assert_eq!(label.level_count(), 1);
    assert_eq!(label.frames(), 1);
    let level = label.level(0).expect("the ordered label level exists");
    let image_level = pyramid.level(0);
    assert_eq!(level.extent(), image_level.extent());
    assert!(level.basis().close(image_level.basis()));
    for (left, right) in level.origin().into_iter().zip(image_level.origin()) {
        assert!(
            (left - right).abs() <= 1.0e-5,
            "the label inherits the placement of its image: {left} against {right}"
        );
    }
}

#[test]
fn writes_labels_over_a_rotated_custom_axis_image() {
    let temp = Temp::new("suite-zarr-label-angled");
    let path = temp.path().join("angled.ome.zarr");
    support::rotated_field_with_axis(&path);
    let image = grana_zarr::Image::open(&path).expect("the angled image opens");
    assert_eq!(image.pyramid().level(0).extent(), Extent::new(4, 3, 2));
    let mut fill = |_level: usize,
                    _frame: u32,
                    _origin: [u32; 3],
                    _extent: [u32; 3],
                    data: &mut [u16]|
     -> Result<(), String> {
        data.fill(7);
        Ok(())
    };
    let mut progress = |_, _| {};
    grana_zarr::write_group_labels(
        &path,
        "/",
        grana_zarr::LabelSpec {
            name: "cells",
            frames: &[0],
            labels: 7,
            color: [0.0, 1.0, 0.0],
            opacity: 1.0,
            properties: &[],
            fill: &mut fill,
            progress: &mut progress,
        },
    )
    .expect("labels write over a rotated image with a custom axis");
    let image = grana_zarr::Image::open(&path).expect("the labelled angled image opens");
    let labels = image.labels().expect("the angled labels read");
    let label = labels.get("cells").expect("the angled label exists");
    let level = label.level(0).expect("the angled label level exists");
    let image_level = image.pyramid().level(0);
    assert!(level.basis().close(image_level.basis()));
    for (left, right) in level.origin().into_iter().zip(image_level.origin()) {
        assert!(
            (left - right).abs() <= 1.0e-4,
            "the angled label inherits the placement of its image: {left} against {right}"
        );
    }
}

#[test]
fn writes_labels_over_a_rotated_plane() {
    let temp = Temp::new("suite-zarr-label-plane");
    let path = temp.path().join("plane.ome.zarr");
    support::rotated_plane_with_axis(&path);
    let image = grana_zarr::Image::open(&path).expect("the angled plane opens");
    assert_eq!(image.pyramid().level(0).extent(), Extent::new(4, 3, 1));
    let mut fill = |_level: usize,
                    _frame: u32,
                    _origin: [u32; 3],
                    _extent: [u32; 3],
                    data: &mut [u16]|
     -> Result<(), String> {
        data.fill(5);
        Ok(())
    };
    let mut progress = |_, _| {};
    grana_zarr::write_group_labels(
        &path,
        "/",
        grana_zarr::LabelSpec {
            name: "cells",
            frames: &[0],
            labels: 5,
            color: [1.0, 0.0, 0.0],
            opacity: 1.0,
            properties: &[],
            fill: &mut fill,
            progress: &mut progress,
        },
    )
    .expect("labels write over a rotated plane with a custom axis");
    let image = grana_zarr::Image::open(&path).expect("the labelled angled plane opens");
    let labels = image.labels().expect("the angled plane labels read");
    let label = labels.get("cells").expect("the angled plane label exists");
    let level = label.level(0).expect("the angled plane label level exists");
    let image_level = image.pyramid().level(0);
    assert!(level.basis().close(image_level.basis()));
    for (left, right) in level.origin().into_iter().zip(image_level.origin()) {
        assert!(
            (left - right).abs() <= 1.0e-4,
            "the angled plane label inherits its placement: {left} against {right}"
        );
    }
}

#[test]
fn orders_plate_fields_naturally() {
    let temp = Temp::new("suite-plate-order");
    let root = temp.path().join("ordered.ome.zarr");
    support::plate_fields(&root, &["10", "2", "1"]);
    let zarr = grana_zarr::Root::local(&root).expect("the plate opens");
    let plate = grana_zarr::open_plate(&zarr).expect("the plate metadata parses");
    assert_eq!(
        plate
            .fields()
            .iter()
            .map(|field| field.name().to_string())
            .collect::<Vec<_>>(),
        vec!["A1/1".to_string(), "A1/2".to_string(), "A1/10".to_string()]
    );
    assert_eq!(plate.fields()[2].index(), 2);
    assert_eq!(plate.fields()[2].series(), 2);
    assert_eq!(plate.field(0).map(grana_zarr::Field::series), Some(0));
    let entries = grana_pipeline::discover(&root, false).expect("the plate discovers its fields");
    assert_eq!(
        entries
            .iter()
            .map(grana_pipeline::DatasetEntry::title)
            .collect::<Vec<_>>(),
        vec!["A1/1".to_string(), "A1/2".to_string(), "A1/10".to_string()]
    );
    assert_eq!(entries[2].group(), "ordered");
    assert_eq!(entries[2].well_row(), Some(0));
    assert_eq!(entries[2].well_column(), Some(0));
    assert_eq!(entries[2].well_field(), Some(2));
}

#[test]
fn round_trips_a_typed_geff_lineage() {
    let temp = Temp::new("suite-geff");
    let path = temp.path().join("graph.geff");
    let detections = vec![
        Detection::new(1, 0, [0.0, 0.0, 0.0], 0.8, 0.9),
        Detection::new(2, 0, [2.0, 0.0, 0.0], 0.8, 0.7),
        Detection::new(3, 1, [1.0, 0.0, 0.0], 1.0, 0.6),
        Detection::new(5, 2, [1.1, 0.0, 0.0], 1.0, 0.5),
        Detection::new(6, 3, [0.7, 0.0, 0.0], 0.9, 0.4),
        Detection::new(7, 3, [1.5, 0.0, 0.0], 0.9, 0.3),
        Detection::new(8, 4, [0.75, 0.0, 0.0], 0.9, 0.2),
        Detection::new(9, 6, [0.8, 0.0, 0.0], 0.9, 0.1),
    ];
    let edges = vec![
        Edge::new(1, 3),
        Edge::new(2, 3),
        Edge::new(3, 5),
        Edge::new(5, 6),
        Edge::new(5, 7),
        Edge::new(6, 8),
        Edge::new(8, 9),
    ];
    let lineage = Lineage::new(detections, edges).expect("the lineage is consistent");
    let axes = geff::Axes::new([0.25, 0.25, 0.5], 2.0);
    geff::write(&path, &lineage, &axes).expect("the graph writes");
    let read = geff::read(&path, 0.5).expect("the graph reads");
    assert_eq!(read.len(), 8);
    assert_eq!(read.edges().len(), 7);
    assert_eq!(read.frames(), vec![0, 1, 2, 3, 4, 6]);
    assert_eq!(read.detections()[2].id(), 3);
    assert_eq!(read.detections()[2].position(), [1.0, 0.0, 0.0]);
    assert!((read.detections()[2].radius() - 1.0).abs() < 1.0e-6);
    assert!((read.detections()[2].quality() - 0.6).abs() < 1.0e-6);
    let set = read.tracks(2.0).expect("the graph forms tracks");
    assert_eq!(set.merges(), 1);
    assert_eq!(set.divisions(), 1);
    assert_eq!(
        set.links()
            .iter()
            .filter(|link| link.kind() == grana_object::Link::Gap)
            .count(),
        1
    );
    assert_eq!(
        read.tracklets()
            .into_iter()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        5
    );
    assert_eq!(
        read.lineages()
            .into_iter()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        1
    );
    let empty = Lineage::new(
        vec![Detection::new(1, 0, [0.0, 0.0, 0.0], 1.0, 0.0)],
        Vec::new(),
    )
    .expect("a single detection is a lineage");
    let path = temp.path().join("empty.geff");
    geff::write(&path, &empty, &axes).expect("an edgeless graph writes");
    let read = geff::read(&path, 0.5).expect("an edgeless graph reads");
    assert_eq!(read.len(), 1);
    assert!(read.edges().is_empty());
    let path = temp.path().join("mismatched.geff");
    geff::write(&path, &lineage, &axes).expect("the graph writes");
    let store = std::sync::Arc::new(
        zarrs::filesystem::FilesystemStore::new(&path).expect("the graph opens as a store"),
    );
    let kinds = zarrs::array::Array::open(store, "/edges/props/grana_kind/values")
        .expect("the edge kinds open");
    kinds
        .store_chunk(&[0], &[0u8; 7])
        .expect("the edge kinds overwrite");
    assert!(
        geff::read(&path, 0.5).is_err(),
        "the graph rejects edge kinds that contradict its topology"
    );
}

#[test]
fn round_trips_a_shaped_geff_graph() {
    use grana_interop::{Kind, Mesh, Objects, Related, Shape};

    let temp = Temp::new("suite-geff-shapes");
    let path = temp.path().join("shapes.geff");
    let detections = vec![
        Detection::new(1, 0, [2.0, 2.0, 2.0], 1.5, 0.4),
        Detection::new(2, 1, [2.5, 2.0, 2.0], 1.5, 0.5),
    ];
    let lineage =
        Lineage::new(detections, vec![Edge::new(1, 2)]).expect("the lineage is consistent");
    let mesh = || {
        Mesh::new(
            vec![
                [1.0, 1.0, 1.0],
                [3.0, 1.0, 1.0],
                [1.0, 3.0, 1.0],
                [1.0, 1.0, 3.0],
            ],
            vec![[0.0, 0.0, 1.0]; 4],
            vec![0, 1, 2, 0, 1, 3, 0, 2, 3, 1, 2, 3],
        )
    };
    let shapes = vec![
        Shape::Surface {
            mesh: mesh(),
            voxels: 10,
            volume: 2.5,
            area: 6.0,
        },
        Shape::Surface {
            mesh: mesh(),
            voxels: 11,
            volume: 2.6,
            area: 6.2,
        },
    ];
    let intensities = Some(vec![
        vec![grana_object::SampleStatistics::new(
            5.0, 1.0, 9.0, 2.0, 50.0,
        )],
        vec![grana_object::SampleStatistics::new(
            6.0, 2.0, 8.0, 1.0, 60.0,
        )],
    ]);
    let related = vec![Related::new("labels", "../labels/", Some("seg_id"))];
    let objects = Objects::new(Kind::Surfaces, shapes, vec![1, 2], intensities, related);
    let axes = geff::Axes::new([0.25, 0.25, 0.5], 2.0);
    geff::write_objects(&path, &lineage, &axes, &objects).expect("the shaped graph writes");
    assert_eq!(geff::kind(&path).expect("the kind reads"), Kind::Surfaces);
    let store = std::sync::Arc::new(
        zarrs::filesystem::FilesystemStore::new(&path).expect("the graph opens as a store"),
    );
    let polygon = zarrs::array::Array::open(store, "/nodes/props/polygon/data")
        .expect("the polygon points open");
    assert!(polygon.shape()[0] > 0, "the graph writes object polygons");
    let (read, objects) = geff::read_objects(&path, 0.5).expect("the shaped graph reads");
    assert_eq!(read.len(), 2);
    assert_eq!(objects.kind(), Kind::Surfaces);
    assert_eq!(objects.labels(), &[1, 2]);
    assert_eq!(objects.related().len(), 1);
    assert_eq!(objects.related()[0].object_type(), "labels");
    assert_eq!(objects.related()[0].path(), "../labels/");
    assert_eq!(objects.related()[0].node_prop(), Some("seg_id"));
    let Shape::Surface {
        mesh,
        voxels,
        volume,
        area,
    } = &objects.shapes()[0]
    else {
        panic!("the first shape is a surface");
    };
    assert_eq!(*voxels, 10);
    assert!((volume - 2.5).abs() < 1.0e-6);
    assert!((area - 6.0).abs() < 1.0e-6);
    assert_eq!(mesh.positions().len(), 4);
    assert_eq!(mesh.indices(), &[0, 1, 2, 0, 1, 3, 0, 2, 3, 1, 2, 3]);
    let intensities = objects
        .intensities()
        .expect("the graph carries intensities");
    assert_eq!(intensities.len(), 2);
    assert_eq!(intensities[0][0].mean(), 5.0);
    assert_eq!(objects.channels(), 1);
}

#[test]
fn round_trips_a_filament_geff_graph() {
    use grana_interop::{Kind, Mesh, Objects, Shape};

    let temp = Temp::new("suite-geff-filaments");
    let path = temp.path().join("filaments.geff");
    let lineage = Lineage::new(
        vec![Detection::new(1, 0, [0.0, 0.0, 0.0], 2.0, 0.5)],
        Vec::new(),
    )
    .expect("the lineage is consistent");
    let mesh = Mesh::new(
        vec![
            [-1.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
        ],
        vec![[0.0, 0.0, 1.0]; 4],
        vec![0, 1, 2, 0, 1, 3, 0, 2, 3, 1, 2, 3],
    );
    let objects = Objects::new(
        Kind::Filaments,
        vec![Shape::Filament {
            mesh,
            length: 3.0,
            diameter: 0.5,
            volume: 0.4,
            branches: 1,
            terminals: 2,
            junctions: 0,
            nodes: 3,
        }],
        vec![1],
        None,
        Vec::new(),
    );
    let axes = geff::Axes::new([0.25, 0.25, 0.5], 2.0);
    geff::write_objects(&path, &lineage, &axes, &objects).expect("the filament graph writes");
    let (read, objects) = geff::read_objects(&path, 0.5).expect("the filament graph reads");
    assert_eq!(read.len(), 1);
    assert_eq!(objects.kind(), Kind::Filaments);
    let Shape::Filament {
        length,
        branches,
        terminals,
        ..
    } = &objects.shapes()[0]
    else {
        panic!("the shape is a filament");
    };
    assert!((length - 3.0).abs() < 1.0e-6);
    assert_eq!(*branches, 1);
    assert_eq!(*terminals, 2);
}
