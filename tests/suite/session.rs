use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use glam::Vec3;
use grana_animation::Timeline;
use grana_display::{CameraState, SectionState};
use grana_geometry::GeometryStore;
use grana_object::{
    IntensityTable, SampleStatistics, Spot, SpotParams, SpotSet, SurfaceFilter, SurfaceParams,
};
use grana_project::{Export, Project, Source, ViewState};
use grana_protocol::Protocol;
use grana_scene::{Dataset, Measure, ObjectFrame, ObjectKind, ObjectParams, Scene};
use grana_session::{Command, Session};
use grana_volume::{
    BrickCoord, BrickRequest, Extent, Fingerprint, Region, Spacing, VolumeSeries, VoxelData,
};

use crate::support::{self, Temp};

const HIGH: u16 = 65_535;

#[test]
fn detects_surfaces_masks_a_channel_and_captures() {
    let mut session = Session::new();
    session.open_series("nuclei", series());
    session.execute(Command::SelectChannel(0));
    session.execute(Command::AddObject(ObjectKind::Surfaces));
    let set = last_surfaces(&session);
    session.execute(Command::SetObjectParams {
        set,
        params: ObjectParams::Surfaces(SurfaceParams::new(
            0.5,
            0.0,
            0.0,
            0.0,
            SurfaceFilter::new(0.0, 0.0, 0.0, 0.0),
        )),
    });
    session.execute(Command::Detect(set));
    settle(&mut session);
    assert_eq!(objects(&session, set), 2, "the two nuclei are detected");

    session.execute(Command::AddMaskChannel { set });
    {
        let scene = session.scene().expect("the series opens");
        assert_eq!(scene.dataset.channels().len(), 2);
        assert_eq!(scene.derived().len(), 1);
        assert_eq!(scene.derived()[0].spec().source(), 0);
    }

    session.request(
        0,
        &[BrickRequest {
            time: 0,
            level: 0,
            coord: BrickCoord::new(1, 0, 0),
        }],
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    let bricks = loop {
        session.poll();
        let bricks = session.take_bricks();
        if !bricks.is_empty() {
            break bricks;
        }
        assert!(Instant::now() < deadline, "the masked brick never arrives");
        std::thread::sleep(Duration::from_millis(1));
    };
    let brick = &bricks[0].1;
    assert_eq!(brick.channels().len(), 2);
    let VoxelData::U16(masked) = &brick.channels()[1] else {
        panic!("the mask channel keeps the stored format");
    };
    assert!(masked.contains(&HIGH));
    assert!(masked.contains(&0));

    session.execute(Command::CaptureProtocol);
    let protocol = session.protocol().expect("the protocol captures");
    assert_eq!(protocol.derived().len(), 1);
}

fn series() -> VolumeSeries {
    let volume = support::spheres(
        Extent::new(64, 32, 32),
        Spacing::new(0.1, 0.1, 0.1),
        &[([26.0, 16.0, 16.0], 3.0), ([38.0, 16.0, 16.0], 5.0)],
        HIGH,
    );
    VolumeSeries::new(vec![volume], 1.0)
}

fn last_surfaces(session: &Session) -> grana_scene::ObjectSetId {
    session
        .scene()
        .expect("the series opens")
        .objects
        .iter()
        .filter(|set| set.kind() == ObjectKind::Surfaces)
        .map(|set| set.id())
        .last()
        .expect("the object set is staged")
}

fn objects(session: &Session, set: grana_scene::ObjectSetId) -> usize {
    session
        .scene()
        .expect("the series opens")
        .objects
        .get(set)
        .and_then(|set| set.frame(0))
        .map_or(0, ObjectFrame::object_count)
}

fn settle(session: &mut Session) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while !session.progress().is_empty() || session.pending() > 0 {
        assert!(Instant::now() < deadline, "the analysis never settled");
        session.poll();
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn view_state() -> ViewState {
    ViewState {
        camera: CameraState {
            pivot: [0.0, 0.0, 0.0],
            yaw: 0.0,
            pitch: 0.0,
            distance: 10.0,
            fov_y: 0.6,
        },
        section: SectionState {
            center: [0.0, 0.0, 0.0],
            azimuth: 0.0,
            elevation: 0.0,
            span: 20.0,
        },
        split: 0.5,
        timeline: Timeline::default(),
        export: Export::NATIVE,
    }
}

#[test]
fn undoes_and_replays_object_edits() {
    let mut session = Session::new();
    session.open_series("edits", series());
    let before = session.scene().expect("the series opens").objects.len();
    session.execute(Command::AddObject(ObjectKind::Spots));
    let set = session
        .scene()
        .expect("the series opens")
        .objects
        .iter()
        .last()
        .expect("the added set exists")
        .id();
    let original = session
        .scene()
        .expect("the series opens")
        .objects
        .get(set)
        .expect("the added set exists")
        .color();
    let edited = [0.2, 0.4, 0.8];
    session.execute(Command::SetObjectColor { set, color: edited });
    assert_eq!(
        session
            .scene()
            .expect("the series opens")
            .objects
            .get(set)
            .expect("the added set exists")
            .color(),
        edited
    );
    session.execute(Command::Undo);
    assert_eq!(
        session
            .scene()
            .expect("the series opens")
            .objects
            .get(set)
            .expect("the added set exists")
            .color(),
        original
    );
    session.execute(Command::Redo);
    assert_eq!(
        session
            .scene()
            .expect("the series opens")
            .objects
            .get(set)
            .expect("the added set exists")
            .color(),
        edited
    );
    session.execute(Command::Undo);
    session.execute(Command::Undo);
    assert_eq!(
        session.scene().expect("the series opens").objects.len(),
        before
    );
    assert!(session.history().undo.is_none());
}

#[test]
fn gates_detection_with_an_oriented_region() {
    let mut session = Session::new();
    session.open_series("region", series());
    session.execute(Command::SelectChannel(0));
    session.execute(Command::AddObject(ObjectKind::Surfaces));
    let set = last_surfaces(&session);
    session.execute(Command::SetObjectParams {
        set,
        params: ObjectParams::Surfaces(SurfaceParams::new(
            0.5,
            0.0,
            0.0,
            0.0,
            SurfaceFilter::new(0.0, 0.0, 0.0, 0.0),
        )),
    });
    let across = Vec3::new(1.0, 0.0, 0.7).normalize();
    let down = Vec3::new(-0.7, 0.0, 1.0).normalize();
    let region = Region::new(
        -0.515 * across,
        [across, Vec3::Y, down],
        Vec3::new(0.485, 2.0, 2.0),
    );
    session.execute(Command::SetRegion(region));
    session.execute(Command::Detect(set));
    settle(&mut session);
    assert_eq!(
        objects(&session, set),
        1,
        "the tilted region keeps only the ball it covers"
    );
}

#[test]
fn keeps_an_oriented_region_across_a_project() {
    let volume = support::spheres(
        Extent::new(24, 24, 16),
        Spacing::new(0.5, 0.5, 0.5),
        &[([12.0, 12.0, 8.0], 4.0)],
        HIGH,
    );
    let mut scene = Scene::new(Dataset::synthetic("fixture", volume.pyramid()));
    let region = oriented_region();
    scene.set_region(region);
    let project = Project::capture(
        &scene,
        &Measure::new(),
        Source::Synthetic {
            name: "fixture".to_string(),
        },
        view_state(),
        Fingerprint::new([7; 32]),
    );
    let temp = Temp::new("suite-region");
    let path = temp.path().join("region.grana");
    project
        .write(&path, &AtomicBool::new(false), &mut |_, _| {})
        .expect("the project writes");
    let loaded = Project::read(
        &path,
        GeometryStore::memory(),
        &AtomicBool::new(false),
        &mut |_, _| {},
    )
    .expect("the project reads");
    assert_eq!(loaded, project);
    assert_eq!(loaded.scene.region, region);
}

fn oriented_region() -> Region {
    Region::new(
        Vec3::new(0.5, -0.25, 0.75),
        [
            Vec3::new(1.0, 0.0, 1.0).normalize(),
            Vec3::Y,
            Vec3::new(-1.0, 0.0, 1.0).normalize(),
        ],
        Vec3::new(3.0, 2.5, 4.0),
    )
}
#[test]
fn round_trips_a_project() {
    let volume = support::spheres(
        Extent::new(16, 16, 8),
        Spacing::new(1.0, 1.0, 1.0),
        &[([8.0, 8.0, 4.0], 3.0)],
        HIGH,
    );
    let mut scene = Scene::new(Dataset::synthetic("fixture", volume.pyramid()));
    let spots = scene.add_object(ObjectKind::Spots, 0);
    scene.install(
        spots,
        0,
        ObjectFrame::Spots(SpotSet::new(
            0,
            vec![Spot::new([0.0, 0.0, 0.0], 1.0, 0.5)],
            IntensityTable::new(
                1,
                vec![SampleStatistics::new(500.0, 500.0, 500.0, 0.0, 500.0)],
            ),
        )),
    );
    scene.annotations.add_point([0.1, 0.2, 0.3], Some(0));
    let project = Project::capture(
        &scene,
        &Measure::new(),
        Source::Synthetic {
            name: "fixture".to_string(),
        },
        view_state(),
        Fingerprint::new([7; 32]),
    );
    let temp = Temp::new("suite-project");
    let path = temp.path().join("scene.grana");
    let mut steps = 0;
    project
        .write(&path, &AtomicBool::new(false), &mut |_, _| steps += 1)
        .expect("the project writes");
    assert!(steps > 0, "the save reports its steps");
    let loaded = Project::read(
        &path,
        GeometryStore::memory(),
        &AtomicBool::new(false),
        &mut |_, _| {},
    )
    .expect("the project reads");
    assert_eq!(loaded, project);
}

#[test]
fn replays_a_captured_protocol() {
    let volume = support::spheres(
        Extent::new(16, 16, 8),
        Spacing::new(1.0, 1.0, 1.0),
        &[([8.0, 8.0, 4.0], 3.0)],
        HIGH,
    );
    let mut scene = Scene::new(Dataset::synthetic("fixture", volume.pyramid()));
    let region = oriented_region();
    scene.set_region(region);
    let spots = scene.add_object(ObjectKind::Spots, 0);
    let set = scene.objects.get_mut(spots).expect("the set exists");
    set.set_params(ObjectParams::Spots(SpotParams::new(0.5, 0.1)));
    set.set_color([0.2, 0.4, 0.8]);
    let protocol = Protocol::capture(&scene, &Measure::new(), None).expect("the scene captures");
    let text = protocol.to_json().expect("the protocol serializes");
    let restored = Protocol::from_json(&text).expect("the protocol parses");
    assert_eq!(restored, protocol);
    let mut staged_scene = Scene::new(Dataset::synthetic("fixture", volume.pyramid()));
    let staged = restored
        .stage(&mut staged_scene)
        .expect("the protocol stages");
    assert_eq!(staged_scene.region, region);
    let staged = staged_scene
        .objects
        .get(staged.sets()[0])
        .expect("the staged set exists");
    assert_eq!(staged.kind(), ObjectKind::Spots);
    assert_eq!(staged.color(), [0.2, 0.4, 0.8]);
    assert!(matches!(staged.params(), ObjectParams::Spots(_)));
}

#[test]
fn navigates_a_plate_field_by_field() {
    let temp = Temp::new("suite-plate-session");
    let root = temp.path().join("assay.ome.zarr");
    support::plate(&root);
    let mut session = Session::new();
    session.begin_load(root.clone());
    open_field(&mut session, Extent::new(8, 6, 4));
    {
        let plate = session.plate().expect("the plate is browsable");
        assert_eq!((plate.plate.rows(), plate.plate.columns()), (1, 2));
        assert_eq!(plate.plate.len(), 2);
        assert_eq!(
            plate
                .selected_well()
                .expect("the field lies in a well")
                .label(),
            "A1"
        );
    }
    session.execute(Command::SelectField(1));
    open_field(&mut session, Extent::new(12, 10, 6));
    {
        let plate = session.plate().expect("the plate keeps its layout");
        assert_eq!(plate.selected, 1);
        assert_eq!(
            plate
                .selected_well()
                .expect("the field lies in a well")
                .label(),
            "A2"
        );
    }
}

fn open_field(session: &mut Session, extent: Extent) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        session.poll();
        if session
            .scene()
            .is_some_and(|scene| scene.dataset.extent() == extent)
        {
            return;
        }
        assert!(Instant::now() < deadline, "the field never opened");
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn round_trips_a_surface_project() {
    let volume = support::spheres(
        Extent::new(16, 16, 8),
        Spacing::new(1.0, 1.0, 1.0),
        &[([8.0, 8.0, 4.0], 3.0)],
        HIGH,
    );
    let mut scene = Scene::new(Dataset::synthetic("fixture", volume.pyramid()));
    let set = scene.add_object(ObjectKind::Surfaces, 0);
    scene.install(set, 0, ObjectFrame::Surfaces(surface()));
    let project = Project::capture(
        &scene,
        &Measure::new(),
        Source::Synthetic {
            name: "fixture".to_string(),
        },
        view_state(),
        Fingerprint::new([7; 32]),
    );
    let temp = Temp::new("suite-surface-project");
    let path = temp.path().join("surface.grana");
    project
        .write(&path, &AtomicBool::new(false), &mut |_, _| {})
        .expect("the surface project writes");
    let loaded = Project::read(
        &path,
        GeometryStore::memory(),
        &AtomicBool::new(false),
        &mut |_, _| {},
    )
    .expect("the surface project reads");
    assert_eq!(loaded, project);
}

fn surface() -> grana_object::SurfaceSet {
    let corners = [
        [-1.0f32, -1.0, -1.0],
        [1.0, -1.0, -1.0],
        [1.0, 1.0, -1.0],
        [-1.0, 1.0, -1.0],
        [-1.0, -1.0, 1.0],
        [1.0, -1.0, 1.0],
        [1.0, 1.0, 1.0],
        [-1.0, 1.0, 1.0],
    ];
    let vertices = corners
        .into_iter()
        .map(|corner| grana_geometry::MeshVertex::new(corner, [0.0, 0.0, 1.0], 0))
        .collect();
    let indices = vec![
        0u32, 3, 2, 0, 2, 1, 4, 5, 6, 4, 6, 7, 0, 1, 5, 0, 5, 4, 1, 2, 6, 1, 6, 5, 2, 3, 7, 2, 7,
        6, 3, 0, 4, 3, 4, 7,
    ];
    let shape = grana_geometry::SurfaceShape::of(&corners, indices.as_chunks::<3>().0)
        .expect("a box carries a shape");
    let statistics = grana_object::SurfaceStatistics::new(
        8,
        8.0,
        24.0,
        [0.0; 3],
        grana_geometry::Bounds::new([-1.0; 3], [1.0; 3]),
        Some(shape),
    );
    grana_object::SurfaceSet::new(
        0,
        grana_geometry::Mesh::new(vertices, indices),
        vec![statistics],
        IntensityTable::new(
            1,
            vec![SampleStatistics::new(500.0, 500.0, 500.0, 0.0, 500.0)],
        ),
    )
}
