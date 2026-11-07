use std::sync::Arc;

use grana_image::{Image, Source};
use grana_object::{IntensityTable, SampleStatistics, Spot, SpotSet};
use grana_pipeline::Channels;
use grana_scene::{Dataset, ObjectFrame, ObjectKind, Scene};
use grana_volume::{BrickCoord, Channel, Extent, Spacing, Volume, VolumeSeries, VoxelData};

const EXTENT: Extent = Extent::new(16, 16, 4);

fn series() -> VolumeSeries {
    let data = VoxelData::U16(vec![1000; EXTENT.voxel_count() as usize]);
    let volume = Volume::new(
        EXTENT,
        Spacing::new(1.0, 1.0, 1.0),
        vec![Channel::new("dapi", [0.0, 0.0, 1.0], data)],
    );
    VolumeSeries::new(vec![volume], 1.0)
}

fn scene(reader: &Image) -> Scene {
    let mut scene = Scene::new(Dataset::synthetic("mask", reader.pyramid().clone()));
    let set = scene.add_object(ObjectKind::Spots, 0);
    scene.install(
        set,
        0,
        ObjectFrame::Spots(SpotSet::new(
            0,
            vec![Spot::new([0.0, 0.0, 0.0], 2.0, 1.0)],
            IntensityTable::new(1, vec![SampleStatistics::new(1.0, 1.0, 1.0, 0.0, 1.0)]),
        )),
    );
    let channel = scene.add_mask(set, 0).expect("the mask channel is added");
    assert_eq!(channel, 1);
    scene
}

#[test]
fn a_mask_channel_keeps_the_signal_inside_the_objects() {
    let reader = Arc::new(Image::open(&Source::memory(series())).expect("the volume opens"));
    let scene = scene(&reader);
    assert_eq!(scene.base_channels(), 1);
    assert_eq!(scene.dataset.channels().len(), 2);
    assert_eq!(scene.dataset.channels()[1].name(), "dapi in Spots 1");
    assert_eq!(
        scene.dataset.channels()[1].intensity(),
        grana_volume::Intensity::new(0.0, 1000.0)
    );
    let level = reader.pyramid().level(0);
    let mask = scene.derived()[0]
        .mask()
        .expect("the mask channel is bound")
        .mask(0, level)
        .expect("the object mask rasterizes");
    assert!(mask.contains_cell([8, 8, 2]), "the spot covers its centre");
    assert!(!mask.contains_cell([0, 0, 0]), "the spot leaves the corner");

    let channels = Channels::new(Arc::clone(&reader), &scene);
    let inside = channels
        .read_region(0, 0, 1, &[8..9, 8..9, 2..3], [0.0; 3])
        .expect("the mask channel reads");
    assert_eq!(inside, VoxelData::U16(vec![1000]));
    let outside = channels
        .read_region(0, 0, 1, &[0..1, 0..1, 0..1], [0.0; 3])
        .expect("the mask channel reads");
    assert_eq!(outside, VoxelData::U16(vec![0]));
    let stored = channels
        .read_region(0, 0, 0, &[0..1, 0..1, 0..1], [0.0; 3])
        .expect("the stored channel reads");
    assert_eq!(stored, VoxelData::U16(vec![1000]));
}

#[test]
fn a_mask_channel_rides_every_brick_of_the_volume() {
    let reader = Arc::new(Image::open(&Source::memory(series())).expect("the volume opens"));
    let scene = scene(&reader);
    let channels = Channels::new(Arc::clone(&reader), &scene);
    let brick = channels
        .read_brick(0, 0, BrickCoord::new(0, 0, 0), [0.0; 3])
        .expect("the brick reads");
    assert_eq!(brick.channels().len(), 2);
    assert_eq!(brick.channels()[1].format(), brick.channels()[0].format());
    let VoxelData::U16(values) = &brick.channels()[1] else {
        panic!("the masked brick keeps the stored format");
    };
    assert!(values.contains(&1000));
    assert!(values.contains(&0));
}

#[test]
fn removing_a_mask_channel_restores_the_stored_channels() {
    let reader = Arc::new(Image::open(&Source::memory(series())).expect("the volume opens"));
    let mut scene = scene(&reader);
    scene
        .remove_derived(1)
        .expect("the mask channel is removed");
    assert_eq!(scene.base_channels(), 1);
    assert_eq!(scene.dataset.channels().len(), 1);
    assert!(scene.derived().is_empty());
    assert!(scene.derived().is_empty());
}
