use std::time::{Duration, Instant};

use grana_object::{SurfaceFilter, SurfaceParams};
use grana_scene::{ObjectFrame, ObjectKind, ObjectParams, ObjectSetId};
use grana_session::{Command, Session};
use grana_volume::{
    BrickCoord, BrickRequest, Channel, Extent, Spacing, Volume, VolumeSeries, VoxelData,
};

const HIGH: u16 = 65_535;
const EXTENT: Extent = Extent::new(96, 48, 48);
const CENTRES: [[f32; 3]; 2] = [[39.0, 24.0, 24.0], [57.0, 24.0, 24.0]];
const RADII: [f32; 2] = [3.0, 5.0];

fn series() -> VolumeSeries {
    let count = EXTENT.voxel_count() as usize;
    let mut nuclei = vec![0u16; count];
    for z in 0..EXTENT.z() {
        for y in 0..EXTENT.y() {
            for x in 0..EXTENT.x() {
                for (index, centre) in CENTRES.iter().enumerate() {
                    let offset = ((x as f32 - centre[0]).powi(2)
                        + (y as f32 - centre[1]).powi(2)
                        + (z as f32 - centre[2]).powi(2))
                    .sqrt();
                    if offset < RADII[index] {
                        nuclei[EXTENT.index(x, y, z) as usize] = HIGH;
                    }
                }
            }
        }
    }
    let volume = Volume::new(
        EXTENT,
        Spacing::new(0.1, 0.1, 0.1),
        vec![Channel::new(
            "nuclei",
            [0.3, 0.6, 1.0],
            VoxelData::U16(nuclei),
        )],
    );
    VolumeSeries::new(vec![volume], 1.0)
}

fn surface_params() -> ObjectParams {
    ObjectParams::Surfaces(SurfaceParams::new(
        0.5,
        0.0,
        0.5,
        0.0,
        SurfaceFilter::new(0.0, 0.0, 0.0, 0.0),
    ))
}

fn settle(session: &mut Session) {
    let deadline = Instant::now() + Duration::from_secs(240);
    while !session.progress().is_empty() || session.pending() > 0 {
        assert!(
            Instant::now() < deadline,
            "the masked analysis never settled"
        );
        session.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn add(session: &mut Session, channel: usize) -> ObjectSetId {
    session.execute(Command::SelectChannel(channel));
    session.execute(Command::AddObject(ObjectKind::Surfaces));
    let scene = session.scene().expect("the series opens");
    let set = scene
        .objects
        .iter()
        .filter(|set| set.kind() == ObjectKind::Surfaces)
        .map(|set| set.id())
        .last()
        .expect("the object set is staged");
    session.execute(Command::SetObjectParams {
        set,
        params: surface_params(),
    });
    set
}

fn objects(session: &Session, set: ObjectSetId) -> usize {
    session
        .scene()
        .expect("the series opens")
        .objects
        .get(set)
        .and_then(|set| set.frame(0))
        .map_or(0, ObjectFrame::object_count)
}

#[test]
fn masks_a_channel_from_an_object_set() {
    let mut session = Session::new();
    session.open_series("masked nuclei", series());
    let nuclei = add(&mut session, 0);
    session.execute(Command::Detect(nuclei));
    settle(&mut session);
    assert_eq!(objects(&session, nuclei), 2, "the two nuclei are detected");

    session.execute(Command::AddMaskChannel { set: nuclei });
    {
        let scene = session.scene().expect("the series opens");
        assert_eq!(scene.dataset.channels().len(), 2);
        assert_eq!(scene.derived().len(), 1);
        assert_eq!(
            scene.derived()[0].spec().mask().map(|spec| spec.set()),
            Some(nuclei)
        );
        assert_eq!(scene.derived()[0].spec().source(), 0);
        assert!(scene.derived()[0].mask().is_some());
    }

    session.request(
        0,
        &[BrickRequest {
            time: 0,
            level: 0,
            coord: BrickCoord::new(1, 0, 0),
        }],
    );
    let deadline = Instant::now() + Duration::from_secs(60);
    let bricks = loop {
        session.poll();
        let bricks = session.take_bricks();
        if !bricks.is_empty() {
            break bricks;
        }
        assert!(Instant::now() < deadline, "the masked brick never arrives");
        std::thread::sleep(Duration::from_millis(2));
    };
    let brick = &bricks[0].1;
    assert_eq!(brick.channels().len(), 2);
    let VoxelData::U16(masked) = &brick.channels()[1] else {
        panic!("the masked channel keeps the stored format");
    };
    assert!(masked.contains(&HIGH));
    assert!(masked.contains(&0));
    assert!(masked.iter().all(|value| *value == HIGH || *value == 0));

    session.execute(Command::CaptureProtocol);
    let protocol = session.protocol().expect("the protocol captures");
    assert_eq!(protocol.derived().len(), 1);
    assert_eq!(
        protocol.derived()[0].name(),
        session
            .scene()
            .expect("the series opens")
            .dataset
            .channels()[1]
            .name()
    );

    let spots = add(&mut session, 1);
    session.execute(Command::Detect(spots));
    settle(&mut session);
    assert!(
        objects(&session, spots) > 0,
        "spots detect on the mask channel"
    );

    session.execute(Command::RemoveDerivedChannel { channel: 1 });
    let scene = session.scene().expect("the series opens");
    assert_eq!(
        scene.dataset.channels().len(),
        2,
        "an object set measures the mask channel"
    );
}
