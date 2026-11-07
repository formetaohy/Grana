use std::time::{Duration, Instant};

use grana_optics::PsfParams;
use grana_protocol::DerivedSpec;
use grana_session::{Command, Session};
use grana_volume::{
    BrickCoord, BrickRequest, Channel, Extent, Spacing, Volume, VolumeSeries, VoxelData,
};

const EXTENT: Extent = Extent::new(64, 64, 16);

fn series() -> VolumeSeries {
    let mut data = vec![0u16; EXTENT.voxel_count() as usize];
    let centre = [EXTENT.x() / 2, EXTENT.y() / 2, EXTENT.z() / 2];
    for z in 0..EXTENT.z() {
        for y in 0..EXTENT.y() {
            for x in 0..EXTENT.x() {
                let offset = [
                    x as f32 - centre[0] as f32,
                    y as f32 - centre[1] as f32,
                    z as f32 - centre[2] as f32,
                ];
                let distance = offset.iter().map(|value| value * value).sum::<f32>().sqrt();
                if distance <= 3.0 {
                    data[EXTENT.index(x, y, z) as usize] = 40_000;
                }
            }
        }
    }
    let volume = Volume::new(
        EXTENT,
        Spacing::new(0.1, 0.1, 0.2),
        vec![Channel::new("dapi", [0.0, 0.0, 1.0], VoxelData::U16(data))],
    );
    VolumeSeries::new(vec![volume], 1.0)
}

fn params() -> PsfParams {
    PsfParams::new(1.4, 0.52, 1.515, 1.33, 0.8)
}

fn settle(session: &mut Session) {
    let deadline = Instant::now() + Duration::from_secs(240);
    while !session.progress().is_empty() || session.pending() > 0 {
        assert!(
            Instant::now() < deadline,
            "the restored analysis never settled"
        );
        session.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn restored_brick(session: &mut Session) -> Vec<f32> {
    session.request(
        0,
        &[BrickRequest {
            time: 0,
            level: 0,
            coord: BrickCoord::new(1, 1, 0),
        }],
    );
    let deadline = Instant::now() + Duration::from_secs(240);
    loop {
        session.poll();
        let bricks = session.take_bricks();
        if !bricks.is_empty() {
            let VoxelData::F32(values) = &bricks[0].1.channels()[1] else {
                panic!("the restored channel reads as floats");
            };
            return values.clone();
        }
        assert!(
            Instant::now() < deadline,
            "the restored brick never arrives"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn a_restored_channel_rides_the_scene_and_its_protocol() {
    let mut session = Session::new();
    session.open_series("restored dapi", series());
    session.execute(Command::SelectChannel(0));
    session.execute(Command::AddObject(grana_scene::ObjectKind::Spots));
    session.execute(Command::AddRestoreChannel {
        source: 0,
        params: params(),
        iterations: 8,
    });
    settle(&mut session);
    {
        let scene = session.scene().expect("the series opens");
        assert_eq!(scene.dataset.channels().len(), 2);
        assert_eq!(scene.derived().len(), 1);
        let restore = scene.derived()[0]
            .spec()
            .restore()
            .expect("the derived channel restores its source");
        assert_eq!(restore.channel(), 0);
        assert_eq!(restore.iterations(), 8);
        assert!(restore.intensity().span() > 0.0);
    }
    let values = restored_brick(&mut session);
    assert!(
        values.iter().any(|value| *value > 0.0),
        "the restored brick carries signal"
    );
    assert!(
        values
            .iter()
            .all(|value| value.is_finite() && *value >= 0.0),
        "the restored brick stays finite and non negative"
    );

    session.execute(Command::CaptureProtocol);
    let protocol = session.protocol().expect("the protocol captures");
    assert_eq!(protocol.derived().len(), 1);
    assert!(matches!(
        protocol.derived()[0],
        DerivedSpec::Restore(ref restore) if restore.iterations == 8
    ));

    session.execute(Command::RemoveDerivedChannel { channel: 1 });
    settle(&mut session);
    {
        let scene = session.scene().expect("the series opens");
        assert_eq!(scene.dataset.channels().len(), 1);
        assert!(scene.derived().is_empty());
    }
}
