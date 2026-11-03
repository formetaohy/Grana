use std::sync::Arc;
use std::time::{Duration, Instant};

use grana_geometry::GeometryStore;
use grana_object::{SurfaceFilter, SurfaceParams};
use grana_scene::{Job, ObjectParams, ObjectSetId};
use grana_session::{DetectRequest, Event, Poll, Source, detect};
use grana_volume::{Channel, Extent, Region, Spacing, Volume, VolumeSeries, VoxelData};

const HIGH: u16 = 65_535;
const EXTENT: Extent = Extent::new(200, 80, 80);
const SPACING: [f32; 3] = [0.1, 0.1, 0.1];
const CENTRES: [[f32; 3]; 2] = [[86.5, 40.0, 40.0], [105.5, 40.0, 40.0]];

fn series() -> VolumeSeries {
    let count = EXTENT.voxel_count() as usize;
    let mut values = vec![0u16; count];
    for z in 0..EXTENT.z() {
        for y in 0..EXTENT.y() {
            for x in 0..EXTENT.x() {
                let inside = CENTRES.iter().any(|centre| {
                    ((x as f32 - centre[0]).powi(2)
                        + (y as f32 - centre[1]).powi(2)
                        + (z as f32 - centre[2]).powi(2))
                    .sqrt()
                        < 10.0
                });
                if inside {
                    values[EXTENT.index(x, y, z) as usize] = HIGH;
                }
            }
        }
    }
    let volume = Volume::new(
        EXTENT,
        Spacing::new(SPACING[0], SPACING[1], SPACING[2]),
        vec![Channel::new(
            "cells",
            [0.3, 0.6, 1.0],
            VoxelData::U16(values),
        )],
    );
    VolumeSeries::new(vec![volume], 1.0)
}

fn seeds() -> Vec<Vec<[f32; 3]>> {
    let world = [0, 1, 2].map(|axis| match axis {
        0 => EXTENT.x() as f32 * SPACING[0],
        1 => EXTENT.y() as f32 * SPACING[1],
        _ => EXTENT.z() as f32 * SPACING[2],
    });
    let positions = CENTRES
        .map(|centre| {
            [0, 1, 2].map(|axis| (centre[axis] + 0.5) * SPACING[axis] - 0.5 * world[axis])
        })
        .to_vec();
    vec![positions]
}

fn run(seeds: Vec<Vec<[f32; 3]>>) -> Result<Vec<usize>, String> {
    let series = series();
    let source = Source::Memory(Arc::new(series));
    let request = DetectRequest::new(
        0,
        ObjectParams::Surfaces(SurfaceParams::new(
            0.5,
            0.0,
            0.0,
            0.0,
            SurfaceFilter::new(0.0, 0.0, 0.0, 0.0),
        )),
        Region::none(),
        None,
        seeds,
        0,
        GeometryStore::memory(),
    );
    let analysis = detect(
        Job::Detect(ObjectSetId::from_raw(1).expect("a set id")),
        source,
        request,
    );
    let deadline = Instant::now() + Duration::from_secs(120);
    let mut frames = Vec::new();
    loop {
        assert!(Instant::now() < deadline, "the detection did not finish");
        match analysis.poll() {
            Poll::Event(Event::Frame { frame, .. }) => frames.push(frame.object_count()),
            Poll::Event(Event::Failed(detail)) => return Err(detail),
            Poll::Event(Event::Done) => return Ok(frames),
            Poll::Event(_) => {}
            Poll::Empty => std::thread::sleep(Duration::from_millis(20)),
            Poll::Closed => panic!("the analysis stopped without a result"),
        }
    }
}

#[test]
fn seeds_reach_the_surface_pipeline() {
    assert_eq!(
        run(Vec::new()).expect("the plain surface is detected"),
        vec![1]
    );
    assert_eq!(
        run(seeds()).expect("the seeded surface is detected"),
        vec![2]
    );
}
