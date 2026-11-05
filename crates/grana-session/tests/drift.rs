use std::time::{Duration, Instant};

use grana_object::{SurfaceFilter, SurfaceParams};
use grana_scene::{ObjectKind, ObjectParams, ObjectSetId};
use grana_session::{Command, Session};
use grana_volume::{
    BrickCoord, BrickRequest, Channel, Extent, PADDED_SIDE, Spacing, Volume, VolumeSeries,
    VoxelData,
};

const HIGH: u16 = 65_535;
const EXTENT: Extent = Extent::new(32, 32, 16);
const SPACING: f32 = 0.1;
const CENTRES: [[f32; 3]; 2] = [[10.0, 16.0, 8.0], [22.0, 16.0, 8.0]];
const RADIUS: f32 = 4.0;
const OFFSET: u32 = 3;

fn noise(x: u32, y: u32, z: u32) -> u16 {
    let mut state =
        u64::from(x) * 0x9e37_79b9 + u64::from(y) * 0x85eb_ca6b + u64::from(z) * 0xc2b2_ae35;
    state ^= state >> 15;
    state = state.wrapping_mul(0x2545_f491_4f6c_dd1d);
    (state >> 48) as u16
}

fn frame(shift: u32) -> Vec<u16> {
    let mut values = vec![0u16; EXTENT.voxel_count() as usize];
    for z in 0..EXTENT.z() {
        for y in 0..EXTENT.y() {
            for x in 0..EXTENT.x() {
                let source = (x as i64) - i64::from(shift);
                let value = if source < 0 {
                    0
                } else {
                    let source = source as u32;
                    let distance = CENTRES
                        .iter()
                        .map(|centre| {
                            ((source as f32 - centre[0]).powi(2)
                                + (y as f32 - centre[1]).powi(2)
                                + (z as f32 - centre[2]).powi(2))
                            .sqrt()
                        })
                        .fold(f32::INFINITY, f32::min);
                    let texture = noise(source, y, z) >> 6;
                    if distance < RADIUS { HIGH } else { texture }
                };
                values[EXTENT.index(x, y, z) as usize] = value;
            }
        }
    }
    values
}

fn series() -> VolumeSeries {
    VolumeSeries::new(
        vec![
            Volume::new(
                EXTENT,
                Spacing::new(SPACING, SPACING, SPACING),
                vec![Channel::new(
                    "signal",
                    [1.0, 0.0, 0.0],
                    VoxelData::U16(frame(0)),
                )],
            ),
            Volume::new(
                EXTENT,
                Spacing::new(SPACING, SPACING, SPACING),
                vec![Channel::new(
                    "signal",
                    [1.0, 0.0, 0.0],
                    VoxelData::U16(frame(OFFSET)),
                )],
            ),
        ],
        1.0,
    )
}

fn surface_params() -> ObjectParams {
    ObjectParams::Surfaces(SurfaceParams::new(
        0.5,
        0.0,
        0.0,
        0.0,
        SurfaceFilter::new(0.0, 0.0, 0.0, 0.0),
    ))
}

fn settle(session: &mut Session) {
    let deadline = Instant::now() + Duration::from_secs(240);
    while !session.progress().is_empty() || session.pending() > 0 {
        assert!(Instant::now() < deadline, "the analysis never settled");
        session.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn add_surfaces(session: &mut Session) -> ObjectSetId {
    session.execute(Command::AddObject(ObjectKind::Surfaces));
    let set = session
        .scene()
        .expect("the series opens")
        .objects
        .iter()
        .map(|set| set.id())
        .last()
        .expect("the object set is staged");
    session.execute(Command::SetObjectParams {
        set,
        params: surface_params(),
    });
    set
}

fn centre(session: &Session, set: ObjectSetId, time: u32) -> f32 {
    let frame = session
        .scene()
        .expect("the series opens")
        .objects
        .get(set)
        .and_then(|set| set.frame(time))
        .expect("the frame is detected");
    let mut centres = frame.centroids();
    centres.sort_by(|left, right| left[0].partial_cmp(&right[0]).expect("the centroids order"));
    assert_eq!(centres.len(), 2, "the two objects survive detection");
    centres[0][0]
}

fn brick(session: &mut Session, time: u32) -> Vec<u16> {
    let request = BrickRequest {
        time,
        level: 0,
        coord: BrickCoord::new(0, 0, 0),
    };
    session.request(time, &[request]);
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        session.poll();
        if let Some((_, data)) = session.take_bricks().first() {
            let VoxelData::U16(values) = &data.channels()[0] else {
                panic!("the fixture stores uint16 voxels");
            };
            return values.clone();
        }
        assert!(Instant::now() < deadline, "the brick never arrived");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn resamples_a_half_voxel_alignment() {
    let mut session = Session::new();
    session.open_series("drifting cells", series());
    session.execute(Command::ComputeDrift);
    settle(&mut session);
    let reference = brick(&mut session, 0);
    session.execute(Command::SetDriftOffset {
        time: 1,
        offset: [OFFSET as f32 + 0.5, 0.0, 0.0],
    });
    let aligned = brick(&mut session, 1);
    let side = PADDED_SIDE as usize;
    for z in 0..EXTENT.z() as usize {
        for y in 0..EXTENT.y() as usize {
            for x in 0..(EXTENT.x() - OFFSET - 2) as usize {
                let slot = (x + 1) + side * ((y + 1) + side * (z + 1));
                let left = u32::from(reference[slot]);
                let right = u32::from(reference[slot + 1]);
                assert_eq!(
                    aligned[slot],
                    (left + right).div_ceil(2) as u16,
                    "the half voxel offset averages its neighbours at ({x}, {y}, {z})"
                );
            }
        }
    }
}

#[test]
fn reads_bricks_through_the_drift() {
    let mut session = Session::new();
    session.open_series("drifting cells", series());
    session.execute(Command::ComputeDrift);
    settle(&mut session);
    session.execute(Command::SetDriftOffset {
        time: 1,
        offset: [OFFSET as f32, 0.0, 0.0],
    });
    let reference = brick(&mut session, 0);
    let aligned = brick(&mut session, 1);
    let side = PADDED_SIDE as usize;
    for z in 0..EXTENT.z() as usize {
        for y in 0..EXTENT.y() as usize {
            for x in 0..(EXTENT.x() - OFFSET) as usize {
                let slot = (x + 1) + side * ((y + 1) + side * (z + 1));
                assert_eq!(
                    aligned[slot], reference[slot],
                    "the aligned brick carries the reference voxel at ({x}, {y}, {z})"
                );
            }
        }
    }

    session.execute(Command::ClearDrift);
    let unaligned = brick(&mut session, 1);
    assert!(
        unaligned != reference,
        "the unaligned brick reads the frame where it is stored"
    );
}

#[test]
fn corrects_the_drift_between_the_frames() {
    let mut session = Session::new();
    session.open_series("drifting cells", series());
    let set = add_surfaces(&mut session);
    session.execute(Command::Detect(set));
    settle(&mut session);
    let first = centre(&session, set, 0);
    let moved = centre(&session, set, 1) - first;
    assert!(
        (moved - OFFSET as f32 * SPACING).abs() < 0.5 * SPACING,
        "the drifted frame reports the motion: {moved}"
    );

    session.execute(Command::ComputeDrift);
    settle(&mut session);
    let drift = session.scene().expect("the series opens").drift();
    assert_eq!(drift.frames(), 2, "the table covers every frame");
    assert_eq!(drift.offset(0), [0.0; 3], "the reference frame holds still");
    let found = drift.offset(1);
    assert!(
        (found[0] - OFFSET as f32).abs() < 0.2 && found[1].abs() < 0.2 && found[2].abs() < 0.2,
        "the estimator recovers the whole motion: {found:?}"
    );

    session.execute(Command::Detect(set));
    settle(&mut session);
    let aligned = centre(&session, set, 1) - first;
    assert!(
        aligned.abs() < 0.5 * SPACING,
        "the aligned frame reports the same position: {aligned}"
    );

    session.execute(Command::ClearDrift);
    assert!(
        session
            .scene()
            .expect("the series opens")
            .drift()
            .is_empty(),
        "the table clears"
    );
}

#[test]
fn re_references_and_edits_the_alignment() {
    let mut session = Session::new();
    session.open_series("drifting cells", series());
    session.execute(Command::ComputeDrift);
    settle(&mut session);
    session.execute(Command::SetDriftReference(1));
    let drift = session.scene().expect("the series opens").drift().clone();
    assert_eq!(drift.reference(), 1, "the chosen frame anchors the table");
    assert_eq!(drift.offset(1), [0.0; 3], "the reference frame holds still");
    let found = drift.offset(0);
    assert!(
        (found[0] + OFFSET as f32).abs() < 0.2 && found[1].abs() < 0.2 && found[2].abs() < 0.2,
        "the other frames move against the new anchor: {found:?}"
    );

    session.execute(Command::SetDriftOffset {
        time: 0,
        offset: [-(OFFSET as f32), 0.0, 0.0],
    });
    let first = brick(&mut session, 0);
    let reference = brick(&mut session, 1);
    let side = PADDED_SIDE as usize;
    for z in 0..EXTENT.z() as usize {
        for y in 0..EXTENT.y() as usize {
            for x in OFFSET as usize..EXTENT.x() as usize {
                let slot = (x + 1) + side * ((y + 1) + side * (z + 1));
                assert_eq!(
                    first[slot], reference[slot],
                    "the first frame aligns onto the reference at ({x}, {y}, {z})"
                );
            }
        }
    }

    session.execute(Command::SetDriftOffset {
        time: 0,
        offset: [0.0; 3],
    });
    let drift = session.scene().expect("the series opens").drift();
    assert_eq!(drift.offset(0), [0.0; 3], "a frame offset is editable");
    let unaligned = brick(&mut session, 0);
    assert!(
        unaligned != first,
        "the edited offset reads the frame where it is stored"
    );
}

#[test]
fn replays_the_drift_correction_of_a_protocol() {
    let mut session = Session::new();
    session.open_series("drifting cells", series());
    let set = add_surfaces(&mut session);
    session.execute(Command::Detect(set));
    settle(&mut session);
    session.execute(Command::ComputeDrift);
    settle(&mut session);
    session.execute(Command::CaptureProtocol);

    session.execute(Command::ClearDrift);
    assert!(
        session
            .scene()
            .expect("the series opens")
            .drift()
            .is_empty(),
        "the table clears before the replay"
    );
    session.execute(Command::ApplyProtocol);
    settle(&mut session);

    let drift = session.scene().expect("the series opens").drift();
    assert_eq!(drift.frames(), 2, "the replay aligns every frame");
    let found = drift.offset(1);
    assert!(
        (found[0] - OFFSET as f32).abs() < 0.2 && found[1].abs() < 0.2 && found[2].abs() < 0.2,
        "the replay installs the captured alignment: {found:?}"
    );
    assert!(
        session
            .scene()
            .expect("the series opens")
            .objects
            .get(set)
            .is_some_and(|set| set.detected() > 0),
        "the detection runs after the alignment"
    );
}
