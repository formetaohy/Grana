use std::time::{Duration, Instant};

use grana_object::{SurfaceFilter, SurfaceParams};
use grana_scene::{ObjectFrame, ObjectKind, ObjectParams, ObjectSetId};
use grana_session::{Command, Session};
use grana_volume::{Channel, Extent, Spacing, Volume, VolumeSeries, VoxelData};

const HIGH: u16 = 65_535;
const EXTENT: Extent = Extent::new(96, 48, 48);
const SPACING: [f32; 3] = [0.1, 0.1, 0.1];
const CENTRES: [[f32; 3]; 2] = [[39.0, 24.0, 24.0], [57.0, 24.0, 24.0]];
const NUCLEI: [f32; 2] = [3.0, 5.0];

fn series() -> VolumeSeries {
    let count = EXTENT.voxel_count() as usize;
    let mut nuclei = vec![0u16; count];
    let mut membrane = vec![0u16; count];
    for z in 0..EXTENT.z() {
        for y in 0..EXTENT.y() {
            for x in 0..EXTENT.x() {
                let offset = |centre: [f32; 3]| {
                    ((x as f32 - centre[0]).powi(2)
                        + (y as f32 - centre[1]).powi(2)
                        + (z as f32 - centre[2]).powi(2))
                    .sqrt()
                };
                for (index, centre) in CENTRES.iter().enumerate() {
                    if offset(*centre) < NUCLEI[index] {
                        nuclei[EXTENT.index(x, y, z) as usize] = HIGH;
                    }
                    let shell = offset(*centre);
                    if (9.0..10.5).contains(&shell) {
                        membrane[EXTENT.index(x, y, z) as usize] = HIGH;
                    }
                }
            }
        }
    }
    let volume = Volume::new(
        EXTENT,
        Spacing::new(SPACING[0], SPACING[1], SPACING[2]),
        vec![
            Channel::new("nuclei", [0.3, 0.6, 1.0], VoxelData::U16(nuclei)),
            Channel::new("membrane", [0.4, 1.0, 0.5], VoxelData::U16(membrane)),
        ],
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
            "the seeded analysis never settled"
        );
        session.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
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

#[test]
fn sources_seeds_from_another_object_set() {
    let mut session = Session::new();
    session.open_series("seeded cells", series());
    let nuclei = add(&mut session, 0);
    let cells = add(&mut session, 1);
    assert_ne!(nuclei, cells, "the two surface sets are distinct");
    let scene = session.scene().expect("the series opens");
    assert_eq!(
        scene
            .objects
            .get(nuclei)
            .expect("the nuclei exist")
            .channel(),
        0,
        "the nuclei detect the first channel"
    );
    assert_eq!(
        scene.objects.get(cells).expect("the cells exist").channel(),
        1,
        "the cells detect the second channel"
    );

    session.execute(Command::Detect(nuclei));
    settle(&mut session);
    assert_eq!(objects(&session, nuclei), 2, "the two nuclei are detected");

    session.execute(Command::SetSeedSource {
        set: cells,
        source: Some(nuclei),
    });
    assert_eq!(
        session
            .scene()
            .expect("the series opens")
            .seed_source(cells),
        Some(nuclei)
    );
    session.execute(Command::Detect(cells));
    settle(&mut session);
    assert_eq!(
        objects(&session, cells),
        2,
        "the nuclei seeds cut the touching cells apart"
    );
    let centres = session
        .scene()
        .expect("the series opens")
        .objects
        .get(cells)
        .and_then(|set| set.frame(0))
        .expect("the cells are detected")
        .centroids();
    let mut left = centres.iter().map(|centre| centre[0]).collect::<Vec<_>>();
    left.sort_by(|a, b| a.partial_cmp(b).expect("the centroids order"));
    assert!(
        left[0] < 0.0 && left[1] > 0.0,
        "each seeded cell keeps its own side: {left:?}"
    );

    let params = session
        .scene()
        .expect("the series opens")
        .objects
        .get(nuclei)
        .expect("the nuclei exist")
        .params();
    let ObjectParams::Surfaces(mut nuclei_params) = params else {
        panic!("the nuclei set detects surfaces");
    };
    nuclei_params.set_min_volume(0.3);
    session.execute(Command::SetObjectParams {
        set: nuclei,
        params: ObjectParams::Surfaces(nuclei_params),
    });
    settle(&mut session);
    assert_eq!(
        objects(&session, nuclei),
        1,
        "the volume filter drops the small nucleus"
    );
    assert_eq!(
        objects(&session, cells),
        1,
        "the seeded cells follow the re-detected seed source"
    );
}
