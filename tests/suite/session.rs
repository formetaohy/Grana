use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use glam::Vec3;
use grana_animation::Timeline;
use grana_display::CameraState;
use grana_geometry::GeometryStore;
use grana_measure::MeasureView;
use grana_object::{
    CellParams, IntensityTable, MaskMode, MaskTool, ObjectAnchor, RelationFrame, RelationPair,
    SampleStatistics, Spot, SpotParams, SpotSet, SurfaceFilter, SurfaceParams,
};
use grana_project::{Export, NamedView, Project, Source, ViewState};
use grana_protocol::{NamedViewSpec, Protocol, ViewSpec};
use grana_scene::{Dataset, Drift, Measure, ObjectFrame, ObjectKind, ObjectParams, Scene};
use grana_session::{Command, Session};
use grana_volume::{
    BrickCoord, BrickRequest, Channel, Extent, Fingerprint, Region, Spacing, Volume, VolumeSeries,
    VoxelData,
};
use grana_warp::{Grid, Warp};

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

#[test]
fn round_trips_a_bleach_channel_through_protocol_and_project() {
    let mut session = Session::new();
    session.open_series("nuclei", series());
    session.execute(Command::AddBleachChannel { source: 0 });
    let deadline = Instant::now() + Duration::from_secs(30);
    while session
        .scene()
        .is_none_or(|scene| scene.derived().is_empty())
    {
        assert!(
            Instant::now() < deadline,
            "the bleach channel never calibrates"
        );
        session.poll();
        std::thread::sleep(Duration::from_millis(1));
    }
    {
        let scene = session.scene().expect("the series opens");
        assert!(matches!(
            scene.derived()[0].spec().operation(),
            grana_scene::Operation::Bleach
        ));
    }
    session.execute(Command::AddObject(ObjectKind::Spots));
    session.execute(Command::CaptureProtocol);
    let protocol = session.protocol().expect("the protocol captures");
    assert_eq!(protocol.derived().len(), 1);
    let text = protocol.to_json().expect("the protocol serializes");
    assert!(text.contains("\"bleach\""), "the protocol names the bleach");
    let restored = Protocol::from_json(&text).expect("the protocol parses");
    assert_eq!(&restored, protocol);
    let scene = session.scene().expect("the series stays open");
    let project = Project::capture(
        scene,
        session.measure(),
        Source::Synthetic {
            name: "nuclei".to_string(),
        },
        view_state(),
        Vec::new(),
        Fingerprint::new([7; 32]),
    );
    let temp = Temp::new("suite-bleach");
    let path = temp.path().join("bleach.grana");
    project
        .write(&path, &AtomicBool::new(false), &mut |_, _| {})
        .expect("the bleach project writes");
    let loaded = Project::read(
        &path,
        GeometryStore::memory(),
        &AtomicBool::new(false),
        &mut |_, _| {},
    )
    .expect("the bleach project reads");
    assert_eq!(loaded, project);
}

#[test]
fn derives_operator_channels_through_a_project() {
    let mut session = Session::new();
    session.open_series("nuclei", series());
    session.execute(Command::AddSmoothChannel {
        source: 0,
        sigma: 0.1,
    });
    session.execute(Command::AddBackgroundChannel {
        source: 1,
        radius: 1.0,
    });
    session.execute(Command::AddRatioChannel {
        numerator: 0,
        denominator: 1,
    });
    let deadline = Instant::now() + Duration::from_secs(30);
    while session
        .scene()
        .is_none_or(|scene| scene.derived().len() < 3)
    {
        assert!(
            Instant::now() < deadline,
            "the ratio channel never calibrates"
        );
        session.poll();
        std::thread::sleep(Duration::from_millis(1));
    }
    {
        let scene = session.scene().expect("the series opens");
        assert_eq!(scene.dataset.channels().len(), 4);
        assert_eq!(scene.derived().len(), 3);
        assert!(matches!(
            scene.derived()[0].spec().operation(),
            grana_scene::Operation::Smooth(_)
        ));
        assert!(matches!(
            scene.derived()[1].spec().operation(),
            grana_scene::Operation::Background(_)
        ));
        assert!(matches!(
            scene.derived()[2].spec().operation(),
            grana_scene::Operation::Ratio(_)
        ));
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
        assert!(
            Instant::now() < deadline,
            "the operator brick never arrives"
        );
        std::thread::sleep(Duration::from_millis(1));
    };
    let channels = bricks[0].1.channels();
    assert_eq!(channels.len(), 4);
    for channel in &channels[1..] {
        let VoxelData::F32(values) = channel else {
            panic!("an operator channel computes single precision values");
        };
        assert!(
            values
                .iter()
                .all(|value| value.is_finite() && *value >= 0.0),
            "an operator channel holds finite non negative values"
        );
    }

    session.execute(Command::SelectChannel(1));
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
    assert_eq!(
        objects(&session, set),
        2,
        "the smoothed channel detects both nuclei"
    );

    session.execute(Command::CaptureProtocol);
    let protocol = session.protocol().expect("the protocol captures");
    assert_eq!(protocol.derived().len(), 3);
    let text = protocol.to_json().expect("the protocol serializes");
    let restored = Protocol::from_json(&text).expect("the protocol parses");
    assert_eq!(&restored, protocol);

    let scene = session.scene().expect("the series stays open");
    let project = Project::capture(
        scene,
        session.measure(),
        Source::Synthetic {
            name: "nuclei".to_string(),
        },
        view_state(),
        Vec::new(),
        Fingerprint::new([7; 32]),
    );
    let temp = Temp::new("suite-operators");
    let path = temp.path().join("operators.grana");
    project
        .write(&path, &AtomicBool::new(false), &mut |_, _| {})
        .expect("the operator project writes");
    let loaded = Project::read(
        &path,
        GeometryStore::memory(),
        &AtomicBool::new(false),
        &mut |_, _| {},
    )
    .expect("the operator project reads");
    assert_eq!(loaded, project, "the operator channels survive a project");
}

#[test]
fn samples_an_intensity_profile_through_project_and_protocol() {
    let mut session = Session::new();
    session.open_series("nuclei", series());
    session.execute(Command::AddSmoothChannel {
        source: 0,
        sigma: 0.2,
    });
    session.execute(Command::AddObject(ObjectKind::Spots));
    session.execute(Command::AddDistance {
        from: [-3.0, 0.05, 0.05],
        to: [3.0, 0.05, 0.05],
    });
    session.execute(Command::SampleProfile { index: 0 });
    let sample_count = {
        let scene = session.scene().expect("the series opens");
        let profile = scene
            .annotations
            .get(0)
            .and_then(|annotation| annotation.profile())
            .expect("the line carries a profile");
        assert_eq!(profile.names().len(), 2);
        assert_eq!(profile.names()[0], "signal");
        assert!(profile.samples().len() > 50);
        let peak = profile
            .samples()
            .iter()
            .map(|sample| sample.values()[0])
            .fold(0.0f32, f32::max);
        assert!(peak > 60_000.0, "the profile misses the nuclei: {peak}");
        let edges = [
            profile.samples()[0].values()[0],
            profile.samples().last().expect("a sample").values()[0],
        ];
        assert!(
            edges.iter().all(|value| *value < 1_000.0),
            "the profile starts outside the nuclei: {edges:?}"
        );

        let project = Project::capture(
            scene,
            session.measure(),
            Source::Synthetic {
                name: "profile".to_string(),
            },
            view_state(),
            Vec::new(),
            Fingerprint::new([11; 32]),
        );
        let temp = Temp::new("suite-profile");
        let path = temp.path().join("profile.grana");
        project
            .write(&path, &AtomicBool::new(false), &mut |_, _| {})
            .expect("the profile project writes");
        let loaded = Project::read(
            &path,
            GeometryStore::memory(),
            &AtomicBool::new(false),
            &mut |_, _| {},
        )
        .expect("the profile project reads");
        let restored = loaded.scene.annotations.entries[0]
            .profile()
            .expect("the project carries the profile");
        assert_eq!(restored.samples().len(), profile.samples().len());
        profile.samples().len()
    };

    session.execute(Command::CaptureProtocol);
    let protocol = session.protocol().expect("the protocol captures");
    let spec = &protocol.annotations()[0];
    let captured = spec.profile().expect("the protocol carries the profile");
    assert_eq!(captured.channels.len(), 2);
    assert_eq!(captured.channels[0], "signal");
    assert_eq!(captured.samples.len(), sample_count);
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

#[test]
fn round_trips_detected_surfaces_through_geff() {
    let temp = Temp::new("session-geff-surfaces");
    let path = temp.path().join("objects.geff");
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
    session
        .export_geff(set, &path)
        .expect("the surfaces export");
    assert_eq!(
        grana_interop::geff::kind(&path).expect("the graph kind reads"),
        grana_interop::Kind::Surfaces
    );
    assert!(session.import_tracks(&path), "the shaped graph imports");
    settle(&mut session);
    let imported = session
        .scene()
        .expect("the series stays open")
        .objects
        .iter()
        .find(|object| object.kind() == ObjectKind::Surfaces && object.id() != set)
        .expect("the imported surface set lands");
    assert_eq!(
        imported.frame(0).map_or(0, ObjectFrame::object_count),
        2,
        "the imported graph restores both surfaces"
    );
    assert!(
        imported
            .frame(0)
            .is_some_and(|frame| frame.geometry().is_ok()),
        "the restored surfaces carry their meshes"
    );
}

#[test]
fn imports_imaris_filaments() {
    let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/imaris-filaments.ims");
    let mut session = Session::new();
    session.begin_load(fixture);
    let deadline = Instant::now() + Duration::from_secs(60);
    while session.scene().is_none() {
        assert!(Instant::now() < deadline, "the Imaris fixture never opens");
        session.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
    let scene = session.scene().expect("the fixture opens");
    let sets = scene.objects.iter().collect::<Vec<_>>();
    assert_eq!(sets.len(), 1);
    let set = sets[0];
    assert_eq!(set.kind(), grana_scene::ObjectKind::Filaments);
    let frame = set.frame(0).expect("the filament holds its first frame");
    let grana_scene::ObjectFrame::Filaments(filaments) = frame else {
        panic!("the frame holds filaments");
    };
    assert_eq!(filaments.object_count(), 1);
    let statistics = filaments.object(0).expect("the filament is measured");
    assert!((statistics.length() - 4.0).abs() <= 1.0e-5);
    assert_eq!(statistics.nodes(), 3);
    assert_eq!(statistics.terminals(), 2);
    assert_eq!(statistics.junctions(), 0);
    assert_eq!(filaments.intensities().object(0, 0).mean(), 5000.0);
    assert!(session.import().is_empty());
}

#[test]
fn imports_imaris_filament_tracks() {
    let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/imaris-tracks.ims");
    let mut session = Session::new();
    session.begin_load(fixture);
    let deadline = Instant::now() + Duration::from_secs(60);
    while session.scene().is_none() {
        assert!(Instant::now() < deadline, "the Imaris fixture never opens");
        session.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
    let scene = session.scene().expect("the fixture opens");
    let set = scene
        .objects
        .iter()
        .next()
        .expect("the filament object is attached");
    assert_eq!(set.kind(), ObjectKind::Filaments);
    let tracks = set
        .tracks()
        .expect("the Imaris track edges become a track set");
    assert_eq!(tracks.set().len(), 1, "the two filaments share one track");
    assert_eq!(tracks.set().node_count(), 2);
    let track = tracks.set().track(0).expect("the track is recorded");
    assert_eq!(track.start(), 0);
    assert_eq!(track.end(), 1);
    assert!(tracks.set().statistics(0).is_some());
}

#[test]
fn measures_imported_objects_without_statistics() {
    let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/imaris-no-statistics.ims");
    let mut session = Session::new();
    session.begin_load(fixture.clone());
    let deadline = Instant::now() + Duration::from_secs(60);
    while session.scene().is_none() {
        assert!(Instant::now() < deadline, "the Imaris fixture never opens");
        session.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
    let scene = session.scene().expect("the fixture opens");
    let sets = scene.objects.iter().collect::<Vec<_>>();
    assert_eq!(sets.len(), 2, "both Imaris objects are attached");
    for set in &sets {
        let frame = set.frame(0).expect("the object holds its first frame");
        let intensities = frame.intensities();
        assert_eq!(intensities.objects(), 1);
        assert_eq!(intensities.channels(), 1);
        assert_eq!(intensities.object(0, 0).mean(), 60_000.0);
        assert!(
            matches!(
                set.provenance().intensities(),
                Some(grana_scene::IntensitySource::Sampled)
            ),
            "the object without statistics is sampled from its voxels"
        );
    }
    let project = Project::capture(
        scene,
        session.measure(),
        Source::File {
            name: "imaris-no-statistics.ims".to_string(),
            path: fixture,
            series: 0,
        },
        view_state(),
        Vec::new(),
        Fingerprint::new([9; 32]),
    );
    let temp = Temp::new("suite-imaris-import");
    let path = temp.path().join("import.grana");
    project
        .write(&path, &AtomicBool::new(false), &mut |_, _| {})
        .expect("the imported objects write a project");
    let loaded = Project::read(
        &path,
        GeometryStore::memory(),
        &AtomicBool::new(false),
        &mut |_, _| {},
    )
    .expect("the imported objects read back");
    assert_eq!(loaded, project, "the sampled intensities survive a project");
}

fn view_state() -> ViewState {
    ViewState {
        camera: CameraState {
            pivot: [0.0, 0.0, 0.0],
            yaw: 0.0,
            pitch: 0.0,
            distance: 10.0,
            fov_y: 0.6,
            projection: grana_display::CameraProjection::Perspective,
            aperture: 0.0,
            focus: 10.0,
            slice: None,
        },
        timeline: Timeline::default(),
        export: Export::NATIVE,
    }
}

#[test]
fn round_trips_a_slice_camera_through_project_and_protocol() {
    let mut session = Session::new();
    session.open_series("nuclei", series());
    let level = session
        .scene()
        .expect("the series opens")
        .dataset
        .pyramid()
        .level(0);
    let frame = level.slice_frame(2);
    let slice = grana_display::Slice::new(2, frame.right(), frame.up(), frame.step());
    let mut view = view_state();
    view.camera.slice = Some(slice);
    view.camera.projection = grana_display::CameraProjection::Orthographic;
    let spec = ViewSpec::of(&view, 0);
    let text = serde_json::to_string(&spec).expect("the view serializes");
    let restored: ViewSpec = serde_json::from_str(&text).expect("the view parses");
    assert_eq!(
        restored.camera().slice,
        Some(slice),
        "the protocol drops the slice plane"
    );
    let scene = session.scene().expect("the series stays open");
    let project = Project::capture(
        scene,
        session.measure(),
        Source::Synthetic {
            name: "nuclei".to_string(),
        },
        view,
        Vec::new(),
        Fingerprint::new([11; 32]),
    );
    let temp = Temp::new("suite-slice");
    let path = temp.path().join("slice.grana");
    project
        .write(&path, &AtomicBool::new(false), &mut |_, _| {})
        .expect("the slice project writes");
    let loaded = Project::read(
        &path,
        GeometryStore::memory(),
        &AtomicBool::new(false),
        &mut |_, _| {},
    )
    .expect("the slice project reads");
    assert_eq!(loaded, project, "the slice camera survives a project");
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
fn undoes_derived_channels_and_keeps_every_selection() {
    let mut session = Session::new();
    session.open_series("derived undo", series());
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
    for object in 0..2 {
        session.execute(Command::ToggleSelect(grana_scene::Selection {
            set,
            item: grana_scene::Item::Object(object),
        }));
    }

    session.execute(Command::AddSmoothChannel {
        source: 0,
        sigma: 0.2,
    });
    {
        let scene = session.scene().expect("the derived channel opens");
        assert_eq!(scene.dataset.channels().len(), 2);
        assert_eq!(scene.derived().len(), 1);
    }
    assert!(session.history().undo.is_some());

    session.execute(Command::Undo);
    {
        let scene = session.scene().expect("the series stays open");
        assert_eq!(scene.dataset.channels().len(), 1, "the channel is undone");
        assert!(scene.derived().is_empty(), "the layout is undone");
    }
    session.execute(Command::Redo);
    {
        let scene = session.scene().expect("the series stays open");
        assert_eq!(scene.dataset.channels().len(), 2, "the channel is redone");
        assert_eq!(scene.selections().len(), 2, "every selection survives undo");
    }

    let scene = session.scene().expect("the series stays open");
    let project = Project::capture(
        scene,
        session.measure(),
        Source::Synthetic {
            name: "derived undo".to_string(),
        },
        view_state(),
        Vec::new(),
        Fingerprint::new([9; 32]),
    );
    let temp = Temp::new("suite-derived-undo");
    let path = temp.path().join("derived.grana");
    project
        .write(&path, &AtomicBool::new(false), &mut |_, _| {})
        .expect("the derived project writes");
    let loaded = Project::read(
        &path,
        GeometryStore::memory(),
        &AtomicBool::new(false),
        &mut |_, _| {},
    )
    .expect("the derived project reads");
    assert_eq!(
        loaded, project,
        "the derived layout and selections survive a project"
    );
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
        Vec::new(),
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
    scene.set_volume_mode(0, grana_display::VolumeMode::Isosurface);
    scene.set_isovalue(0, 0.35);
    let mut view = view_state();
    view.camera.yaw = 0.55;
    view.camera.distance = 12.0;
    let project = Project::capture(
        &scene,
        &Measure::new(),
        Source::Synthetic {
            name: "fixture".to_string(),
        },
        view.clone(),
        vec![NamedView::new("Overview", view.clone())],
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
fn round_trips_a_deformation_through_protocol_and_project() {
    let volume = support::spheres(
        Extent::new(16, 16, 8),
        Spacing::new(1.0, 1.0, 1.0),
        &[([8.0, 8.0, 4.0], 3.0)],
        HIGH,
    );
    let pyramid = VolumeSeries::new(vec![volume.clone(), volume], 1.0)
        .pyramid()
        .clone();
    let mut scene = Scene::new(Dataset::synthetic("fixture", pyramid.clone()));
    let grid = Grid::new(
        [2, 2, 1],
        [-8.0, -8.0, -4.0],
        [16.0, 16.0, 1.0],
        vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [1.0, 1.0, 0.0],
        ],
    );
    let warps = vec![Warp::IDENTITY, Warp::field(grid.clone())];
    assert!(
        scene.set_drift(Drift::new(0, 0, warps.clone())),
        "the scene accepts a deformation"
    );
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
    let protocol = Protocol::capture(&scene, &Measure::new(), None, Vec::new(), None)
        .expect("the protocol captures");
    let text = protocol.to_json().expect("the protocol serializes");
    assert!(text.contains("\"field\""), "the protocol records the field");
    let restored = Protocol::from_json(&text).expect("the protocol parses");
    assert_eq!(restored, protocol);
    let mut staged = Scene::new(Dataset::synthetic("fixture", pyramid));
    let steps = restored.stage(&mut staged).expect("the protocol stages");
    let staged_warps = steps
        .steps()
        .iter()
        .find_map(|step| match step {
            grana_protocol::Step::Drift { warps, .. } => Some(warps.clone()),
            _ => None,
        })
        .expect("the protocol stages its drift");
    assert_eq!(staged_warps, warps);

    let project = Project::capture(
        &scene,
        &Measure::new(),
        Source::Synthetic {
            name: "fixture".to_string(),
        },
        view_state(),
        Vec::new(),
        Fingerprint::new([7; 32]),
    );
    let temp = Temp::new("suite-deformation");
    let path = temp.path().join("field.grana");
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
}

#[test]
fn saves_and_applies_named_views() {
    let mut session = Session::new();
    session.open_series("nuclei", series());
    session.observe_view(view_state());
    session.execute(Command::SaveView);
    assert_eq!(session.views().len(), 1, "the view is saved");
    assert_eq!(session.views()[0].name, "View 1");
    let mut moved = view_state();
    moved.camera.yaw = 0.5;
    session.observe_view(moved.clone());
    session.execute(Command::SaveView);
    assert_eq!(session.views()[1].name, "View 2");
    session.execute(Command::ApplyView(0));
    let applied = session.take_view().expect("the saved view restores");
    assert_eq!(applied.camera.yaw, 0.0, "the first view keeps its camera");
    assert_eq!(applied.camera.focus, view_state().camera.focus);
    session.execute(Command::RemoveView(0));
    assert_eq!(session.views().len(), 1, "the view is removed");
    assert_eq!(session.views()[0].name, "View 2");
    assert_eq!(session.views()[0].view, moved);
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
    scene.set_label_export(
        spots,
        Some(grana_scene::LabelExport::new("/fixture.ome.zarr", 1, 5)),
    );
    scene.set_volume_mode(0, grana_display::VolumeMode::Maximum);
    let view = ViewSpec::of(&view_state(), 0);
    let named = NamedViewSpec::of("Overview", view.clone());
    let protocol = Protocol::capture(
        &scene,
        &Measure::new(),
        Some(view),
        vec![named.clone()],
        None,
    )
    .expect("the scene captures");
    let text = protocol.to_json().expect("the protocol serializes");
    let restored = Protocol::from_json(&text).expect("the protocol parses");
    assert_eq!(restored, protocol);
    assert_eq!(restored.label_images().len(), 1);
    assert_eq!(restored.views(), [named]);
    assert_eq!(
        restored.snapshot().expect("the protocol snapshots").views(),
        ["Overview"]
    );
    let mut broken: serde_json::Value = serde_json::from_str(&text).expect("the protocol is json");
    broken["snapshot"]["views"][0] = serde_json::Value::String("Missing".to_string());
    assert!(
        Protocol::from_json(&broken.to_string()).is_err(),
        "a snapshot rejects a view the protocol does not carry"
    );
    let mut filmed: serde_json::Value = serde_json::from_str(&text).expect("the protocol is json");
    filmed["snapshot"]["movies"] = serde_json::Value::Bool(true);
    filmed["snapshot"]["sequences"] = serde_json::Value::Bool(true);
    let filmed = Protocol::from_json(&filmed.to_string()).expect("a snapshot films its views");
    let snapshot = filmed.snapshot().expect("the protocol snapshots");
    assert!(
        snapshot.movies() && snapshot.sequences(),
        "a snapshot records that it films and frames its views"
    );
    let mut staged_scene = Scene::new(Dataset::synthetic("fixture", volume.pyramid()));
    let staged = restored
        .stage(&mut staged_scene)
        .expect("the protocol stages");
    assert!(
        staged
            .steps()
            .iter()
            .any(|step| matches!(step, grana_protocol::Step::Labels { .. })),
        "the protocol stages the label image export"
    );
    assert_eq!(staged_scene.region, region);
    assert_eq!(
        staged_scene.channels[0].volume().mode(),
        grana_display::VolumeMode::Maximum,
        "the protocol stages the volume mode"
    );
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
    let first = preview(&mut session, 0);
    assert_eq!(
        first.size(),
        [8, 6],
        "a preview never stretches past its level"
    );
    assert!(
        first.pixels().chunks(4).any(|pixel| pixel[2] > pixel[0]),
        "the preview carries the blue channel tint"
    );
    session.execute(Command::SelectField(0));
    open_field(&mut session, Extent::new(8, 6, 4));
    assert_eq!(
        preview(&mut session, 1).size(),
        [12, 10],
        "switching fields keeps the plate previews"
    );
    let montage = temp.path().join("plate.png");
    assert!(session.export_plate_montage(montage.clone()));
    let deadline = Instant::now() + Duration::from_secs(30);
    while session.writing() {
        assert!(Instant::now() < deadline, "the montage never finished");
        session.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
    let sheet = grana_raster::read(&montage).expect("the montage reads back");
    assert!(
        sheet.width() > 2 * 192 && sheet.height() > 144,
        "the montage lays the two wells side by side"
    );
}

#[test]
fn keeps_each_well_scene_until_the_well_returns() {
    let temp = Temp::new("suite-plate-state");
    let root = temp.path().join("assay.ome.zarr");
    support::plate(&root);
    let mut session = Session::new();
    session.begin_load(root);
    open_field(&mut session, Extent::new(8, 6, 4));
    session.execute(Command::SelectChannel(0));
    session.execute(Command::AddObject(ObjectKind::Spots));
    let first = session
        .scene()
        .expect("the first well opens")
        .objects
        .iter()
        .last()
        .expect("the first set")
        .id();
    session.execute(Command::RenameObject {
        set: first,
        name: "well one".to_string(),
    });
    session.execute(Command::SetMeasureView(MeasureView::Histogram));
    session.execute(Command::SetTimelineFps(24));
    session.observe_view(view_state());
    assert!(session.history().undo.is_some());

    session.execute(Command::SelectField(1));
    open_field(&mut session, Extent::new(12, 10, 6));
    assert_eq!(
        session
            .scene()
            .expect("the second well opens")
            .objects
            .len(),
        0,
        "a new well starts without objects"
    );
    assert!(
        session.history().undo.is_none(),
        "a new well starts without history"
    );
    assert!(
        session.take_view().is_none(),
        "a new well fits its own camera"
    );
    session.execute(Command::AddObject(ObjectKind::Surfaces));
    let second = session
        .scene()
        .expect("the second well opens")
        .objects
        .iter()
        .last()
        .expect("the second set")
        .id();
    session.execute(Command::RenameObject {
        set: second,
        name: "well two".to_string(),
    });
    session.observe_view(view_state());

    session.execute(Command::SelectField(0));
    open_field(&mut session, Extent::new(8, 6, 4));
    {
        let scene = session.scene().expect("the first well returns");
        assert_eq!(
            scene.objects.len(),
            1,
            "the first well keeps its object set"
        );
        let held = scene.objects.iter().next().expect("the first set returns");
        assert_eq!(held.name(), "well one");
        assert_eq!(held.id(), first);
    }
    assert_eq!(session.measure().view(), MeasureView::Histogram);
    assert_eq!(session.timeline().fps(), 24);
    let view = session
        .take_view()
        .expect("the first well keeps its camera");
    assert_eq!(view.camera.distance, 10.0);
    assert_eq!(view.camera.focus, 10.0);
    assert!(
        session.history().undo.is_some(),
        "the first well keeps its history"
    );

    session.execute(Command::SelectField(1));
    open_field(&mut session, Extent::new(12, 10, 6));
    let scene = session.scene().expect("the second well returns");
    assert_eq!(
        scene.objects.len(),
        1,
        "the second well keeps its object set"
    );
    assert_eq!(
        scene
            .objects
            .iter()
            .next()
            .expect("the second set returns")
            .name(),
        "well two"
    );
}

#[test]
fn paints_erases_and_undoes_a_voxel_mask() {
    let mut session = Session::new();
    session.open_series("masking", series());
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
    let anchors = frame_anchors(&session, set);
    let volume = frame_volume(&session, set);

    session.execute(Command::PaintMask {
        set,
        frame: 0,
        anchor: anchors[0],
        tool: MaskTool::Brush,
        mode: MaskMode::Remove,
        radius: 0.25,
        tolerance: 0.0,
        points: vec![anchors[0].position()],
    });
    let masked = masked_set(&session, set);
    assert!(
        frame_volume(&session, masked) < volume,
        "the eraser removes voxels from the painted surface"
    );

    session.execute(Command::PaintMask {
        set: masked,
        frame: 0,
        anchor: anchors[1],
        tool: MaskTool::Wand,
        mode: MaskMode::Remove,
        radius: 0.0,
        tolerance: 0.05,
        points: vec![anchors[1].position()],
    });
    assert_eq!(
        objects(&session, masked),
        1,
        "the wand erases the connected surface it starts in"
    );

    session.execute(Command::Undo);
    assert_eq!(
        objects(&session, masked_set(&session, set)),
        2,
        "undo restores the erased surface"
    );

    session.execute(Command::CaptureProtocol);
    let protocol = session.protocol().expect("the protocol captures");
    assert!(
        protocol.summary().contains("mask strokes"),
        "the protocol records the painted strokes"
    );

    let scene = session.scene().expect("the series opens");
    let project = Project::capture(
        scene,
        session.measure(),
        Source::Synthetic {
            name: "masking".to_string(),
        },
        view_state(),
        Vec::new(),
        Fingerprint::new([9; 32]),
    );
    let temp = Temp::new("suite-mask-project");
    let path = temp.path().join("mask.grana");
    project
        .write(&path, &AtomicBool::new(false), &mut |_, _| {})
        .expect("the mask project writes");
    let loaded = Project::read(
        &path,
        GeometryStore::memory(),
        &AtomicBool::new(false),
        &mut |_, _| {},
    )
    .expect("the mask project reads");
    assert_eq!(loaded, project, "the painted strokes survive a project");
}

fn frame_anchors(session: &Session, set: grana_scene::ObjectSetId) -> Vec<ObjectAnchor> {
    let surfaces = surfaces_of(session, set);
    surfaces
        .objects()
        .iter()
        .map(|statistics| ObjectAnchor::new(statistics.centroid(), statistics.bounds()))
        .collect()
}

fn frame_volume(session: &Session, set: grana_scene::ObjectSetId) -> u64 {
    surfaces_of(session, set)
        .objects()
        .iter()
        .map(|statistics| statistics.voxel_count())
        .sum()
}

fn surfaces_of(session: &Session, set: grana_scene::ObjectSetId) -> grana_object::SurfaceSet {
    let scene = session.scene().expect("the series opens");
    let frame = scene
        .objects
        .get(set)
        .and_then(|object| object.frame(0))
        .expect("the object set holds a frame");
    match frame {
        ObjectFrame::Surfaces(surfaces) => surfaces.clone(),
        _ => panic!("the object set holds surfaces"),
    }
}

fn masked_set(session: &Session, source: grana_scene::ObjectSetId) -> grana_scene::ObjectSetId {
    session
        .scene()
        .expect("the series opens")
        .objects
        .iter()
        .find(|set| {
            set.provenance()
                .masking()
                .is_some_and(|(masked, _)| masked == source)
        })
        .map(|set| set.id())
        .expect("the mask edits derive an object set")
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

fn preview(session: &mut Session, series: usize) -> grana_raster::Bitmap {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        session.poll();
        if let Some(bitmap) = session
            .plate()
            .and_then(|plate| plate.thumbnail(series))
            .cloned()
        {
            return bitmap;
        }
        assert!(Instant::now() < deadline, "the plate preview never arrived");
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
    assert!(scene.add_relation(set, set, 0), "a set relates to itself");
    scene.install_relation(
        set,
        set,
        0,
        0,
        RelationFrame::new(
            vec![1.5],
            vec![2.5],
            vec![0.25],
            vec![0.75],
            vec![1],
            vec![1],
            vec![
                RelationPair::new(0, 0, 0.75, 0.0, 0.25, 2.5),
                RelationPair::new(0, 1, 0.0, 1.5, 0.0, 4.0),
            ],
        ),
    );
    scene.set_label_export(
        set,
        Some(grana_scene::LabelExport::new("/fixture.ome.zarr", 1, 12)),
    );
    let project = Project::capture(
        &scene,
        &Measure::new(),
        Source::Synthetic {
            name: "fixture".to_string(),
        },
        view_state(),
        Vec::new(),
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

#[test]
fn colocalizes_every_object_and_carries_it_through_project_and_protocol() {
    let extent = Extent::new(24, 24, 8);
    let spacing = Spacing::new(1.0, 1.0, 1.0);
    let spheres = [([7.0, 12.0, 4.0], 3.0), ([17.0, 12.0, 4.0], 3.0)];
    let mut values = vec![0u16; extent.voxel_count() as usize];
    for z in 0..extent.z() {
        for y in 0..extent.y() {
            for x in 0..extent.x() {
                let point = [x as f32, y as f32, z as f32];
                let inside = spheres.iter().any(|(centre, radius)| {
                    (0..3)
                        .map(|axis| (point[axis] - centre[axis]).powi(2))
                        .sum::<f32>()
                        <= radius * radius
                });
                if inside {
                    values[extent.index(x, y, z) as usize] = HIGH / 2 + (x as u16) * 400;
                }
            }
        }
    }
    let channel = |name: &str, tint: [f32; 3]| {
        grana_volume::Channel::new(name, tint, VoxelData::U16(values.clone()))
    };
    let volume = grana_volume::Volume::new(
        extent,
        spacing,
        vec![
            channel("green", [0.0, 1.0, 0.0]),
            channel("red", [1.0, 0.0, 0.0]),
        ],
    );
    let mut session = Session::new();
    session.open_series("coloc", VolumeSeries::new(vec![volume], 1.0));
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
    assert_eq!(objects(&session, set), 2, "two nuclei are detected");
    session.execute(Command::ComputeObjectColoc(set));
    settle(&mut session);
    let pair = grana_measure::ColocPair::new(0, 1);
    let object = session
        .scene()
        .expect("the series opens")
        .objects
        .get(set)
        .expect("the object set exists");
    let coloc = object.object_coloc().expect("the objects colocalize");
    assert_eq!(coloc.pair(), pair);
    assert_eq!(
        coloc.metric(0, 0, grana_measure::ColocMetric::Pearson),
        Some(1.0),
        "identical channels correlate perfectly inside an object"
    );
    let scene = session.scene().expect("the series opens");
    let table = scene
        .table(
            set,
            grana_measure::Group::Object,
            grana_measure::Aggregate::MEAN,
        )
        .expect("the measurement table builds");
    let pearson = table
        .column("pearson_green_red")
        .expect("the table carries the pearson column");
    let overlap = table
        .column("overlap_green_red")
        .expect("the table carries the overlap column");
    let rows = table.matching(&[grana_measure::Filter::new(pearson, 0.5, 1.0)], None, None);
    assert_eq!(rows.len(), table.rows(), "the filter keeps every object");
    assert_eq!(table.value(rows[0], overlap), 1.0);

    let mut coloring = grana_display::Coloring::new();
    coloring.set_metric(Some(grana_scene::ObjectColor::Coloc {
        pair,
        metric: grana_measure::ColocMetric::Pearson,
    }));
    session.execute(Command::SetObjectColoring { set, coloring });
    assert!(
        session
            .scene()
            .expect("the series opens")
            .object_colors(
                session
                    .scene()
                    .expect("the series opens")
                    .objects
                    .get(set)
                    .expect("the object set exists")
            )
            .is_some(),
        "the objects color by their correlation"
    );

    let pyramid = session
        .scene()
        .expect("the series opens")
        .dataset
        .pyramid()
        .clone();
    let project = {
        let scene = session.scene().expect("the series opens");
        Project::capture(
            scene,
            session.measure(),
            Source::Synthetic {
                name: "coloc".to_string(),
            },
            view_state(),
            Vec::new(),
            Fingerprint::new([13; 32]),
        )
    };
    let temp = Temp::new("suite-object-coloc");
    let path = temp.path().join("coloc.grana");
    project
        .write(&path, &AtomicBool::new(false), &mut |_, _| {})
        .expect("the colocalization project writes");
    let loaded = Project::read(
        &path,
        GeometryStore::memory(),
        &AtomicBool::new(false),
        &mut |_, _| {},
    )
    .expect("the colocalization project reads");
    assert_eq!(
        loaded, project,
        "the object colocalization survives a project"
    );

    session.execute(Command::CaptureProtocol);
    let protocol = session.protocol().expect("the protocol captures");
    assert!(
        protocol.summary().contains("colocalizes green with red"),
        "the protocol records the object colocalization"
    );
    let text = protocol.to_json().expect("the protocol serializes");
    let parsed = Protocol::from_json(&text).expect("the protocol reads");
    assert_eq!(&parsed, protocol);
    let mut staged = Scene::new(Dataset::synthetic("coloc", pyramid));
    let steps = parsed.stage(&mut staged).expect("the protocol stages");
    assert!(
        steps.steps().iter().any(|step| matches!(
            step,
            grana_protocol::Step::ObjectColoc { pair: staged_pair, .. }
                if *staged_pair == pair
        )),
        "the protocol stages the object colocalization"
    );
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

#[test]
fn reads_a_batch_run_record() {
    let temp = Temp::new("run-record");
    let datasets = [
        ("A1", "control", 12, 1.0, 120.0),
        ("A2", "control", 14, 2.0, 130.0),
        ("B1", "treated", 8, 10.0, 90.0),
        ("B2", "treated", 10, 11.0, 80.0),
    ]
    .into_iter()
    .map(|(title, group, objects, volume, intensity)| {
        let drift = if group == "control" { 0.2 } else { -1.0 };
        serde_json::json!({
            "path": format!("E:\\data\\plate\\well-{title}.tif"),
            "group": group,
            "title": title,
            "scene": 1,
            "scenes": 3,
            "scene_name": "second",
            "well": {"row": 0, "column": 0, "field": 1},
            "plate": "A1",
            "folder": format!("{group}\\{title}"),
            "status": "ok",
            "objects": objects,
            "detail": format!("Nuclei={objects}"),
            "content": 424242,
            "bytes": 1024,
            "modified": 1700000000,
            "protocol": 7,
            "quality": [
                {
                    "dataset": title,
                    "group": group,
                    "key": "nuclei",
                    "name": "Nuclei",
                    "kind": "Surfaces",
                    "objects": objects,
                    "metrics": [
                        {"key": "objects", "label": "objects", "unit": "count", "decimals": 0, "level": "image"},
                        {"key": "volume_um3", "label": "volume", "unit": "cubic_micrometer", "decimals": 3, "level": "object"},
                        {"key": "area_um2", "label": "area", "unit": "square_micrometer", "decimals": 3, "level": "object"},
                        {"key": "diameter_um", "label": "diameter", "unit": "micrometer", "decimals": 3, "level": "object"},
                        {"key": "nuclei_intensity_mean", "label": "Nuclei · mean", "unit": "count", "decimals": 2, "level": "object"}
                    ],
                    "values": [
                        {"objects": 1, "mean": objects},
                        {"objects": objects, "mean": volume, "deviation": 0.5},
                        {"objects": objects, "mean": 1.0},
                        {"objects": objects, "mean": 2.0, "deviation": 0.1},
                        {"objects": objects, "mean": intensity, "deviation": 4.0}
                    ],
                    "frames": [
                        {
                            "time": 0,
                            "values": [null, {"count": objects, "mean": volume, "deviation": 0.5}, null, null, null]
                        },
                        {
                            "time": 1,
                            "values": [null, {"count": objects, "mean": volume + drift, "deviation": 0.5}, null, null, null]
                        }
                    ]
                }
            ]
        })
    })
    .collect::<Vec<_>>();
    let record = serde_json::json!({
        "format": 5,
        "build": "0.1.1/12",
        "protocol": 7,
        "jobs": 2,
        "project": true,
        "channels": false,
        "datasets": datasets
    });
    std::fs::write(temp.path().join("run.json"), record.to_string())
        .expect("the run record is written");
    let run = grana_pipeline::read_run(temp.path())
        .expect("the run record reads")
        .expect("the folder holds a run record");
    assert_eq!(run.jobs(), 2);
    assert!(run.project());
    assert!(!run.channels());
    let entry = run.datasets()[0].entry().expect("the dataset reopens");
    assert_eq!(entry.title(), "A1");
    assert_eq!(entry.scene(), 1);
    assert_eq!(entry.scenes(), 3);
    assert_eq!(entry.well_key().map(|well| well.field()), Some(1));
    let report = run.comparison();
    assert_eq!(report.groups.len(), 2);
    assert_eq!(report.groups[0].group, "control");
    assert_eq!(report.groups[0].images, 2);
    assert_eq!(report.groups[0].objects, 26);
    assert_eq!(report.groups[0].samples[0].dataset, "A1");
    assert_eq!(
        report.groups[0].samples[1].values[0].map(grana_pipeline::QualitySummary::mean),
        Some(14.0)
    );
    assert_eq!(
        report
            .metrics
            .iter()
            .map(grana_pipeline::Metric::key)
            .collect::<Vec<_>>(),
        [
            "objects",
            "volume_um3",
            "area_um2",
            "diameter_um",
            "nuclei_intensity_mean"
        ]
    );
    assert_eq!(report.tests.len(), 7);
    assert!(
        report
            .tests
            .iter()
            .all(|test| test.test.clusters_a() == 2 && test.test.clusters_b() == 2)
    );
    assert!(
        report
            .tests
            .iter()
            .filter(|test| test.metric.key() != "objects")
            .all(|test| test.test.samples_a() == 26 && test.test.samples_b() == 18)
    );
    assert!(
        report
            .tests
            .iter()
            .all(|test| test.adjusted >= test.p() && test.p() > 0.0)
    );
    assert!(report.tests.iter().all(|test| test.rank.n_a() == 2
        && test.rank.n_b() == 2
        && test.rank_adjusted >= test.rank.p()));
    assert!(
        report
            .tests
            .iter()
            .all(|test| test.interval.is_some() && test.rank.delta().abs() <= 1.0)
    );
    assert!(
        report
            .tests
            .iter()
            .any(grana_pipeline::Significance::significant),
        "a separated group tests significant"
    );
    let table = grana_pipeline::significance_table(&report.tests);
    assert!(table.starts_with("group_a,group_b,set_key,set,kind,metric,frame,level,"));
    assert!(
        table
            .lines()
            .next()
            .is_some_and(|header| header.contains("clusters_a") && header.contains("rank_p"))
    );
    let interaction = report
        .interactions
        .first()
        .expect("the per-frame values test their slopes");
    assert_eq!(interaction.metric.key(), "volume_um3");
    assert_eq!(interaction.samples(), (2, 2));
    assert!((interaction.slope_a() - 0.2).abs() < 1.0e-1);
    assert!((interaction.slope_b() + 1.0).abs() < 1.0e-1);
    assert!(interaction.q() <= interaction.p());
    assert!(interaction.significant());
    let table = grana_pipeline::interaction_table(&report.interactions);
    assert!(table.starts_with("group_a,group_b,set_key,set,kind,metric,level,"));
    assert!(table.contains("slope_a") && table.contains("rank_q"));
}

#[test]
fn records_a_batch_correction() {
    let temp = Temp::new("run-correction");
    let record = serde_json::json!({
        "format": 5,
        "build": "0.1.1/12",
        "protocol": 7,
        "jobs": 1,
        "project": false,
        "channels": false,
        "datasets": [
            {
                "path": "E:\\data\\plate\\well-a1.tif",
                "group": "control",
                "title": "A1",
                "scene": 0,
                "scenes": 1,
                "scene_name": null,
                "well": null,
                "plate": null,
                "folder": "control\\a1",
                "status": "ok",
                "objects": 12,
                "detail": "Nuclei=12",
                "content": 424242,
                "bytes": 1024,
                "modified": 1700000000,
                "protocol": 7,
                "quality": []
            }
        ]
    });
    std::fs::write(temp.path().join("run.json"), record.to_string())
        .expect("the run record is written");
    let protocol = Protocol::from_json(
        r#"{
            "sets": [
                {
                    "key": "spots",
                    "name": "Spots",
                    "color": [1.0, 0.5, 0.2],
                    "source": {
                        "kind": "spots",
                        "channel": "Channel 1",
                        "diameter": 2.0,
                        "contrast": 0.2
                    }
                }
            ]
        }"#,
    )
    .expect("the correction protocol is valid");
    let dataset = std::path::PathBuf::from("E:\\data\\plate\\well-a1.tif");
    grana_pipeline::record_correction(temp.path(), &dataset, 0, &protocol)
        .expect("the correction records");
    let run = grana_pipeline::read_run(temp.path())
        .expect("the run record reads")
        .expect("the folder holds a run record");
    assert_eq!(run.datasets()[0].correction(), Some(protocol.digest()));
    let path = temp.path().join("control\\a1").join("correction.json");
    assert!(
        path.exists(),
        "the correction is written next to the dataset"
    );
    let reread = Protocol::read(&path).expect("the correction reads");
    assert_eq!(reread, protocol);
    grana_pipeline::clear_correction(temp.path(), &dataset, 0).expect("the correction clears");
    assert!(!path.exists(), "the correction file is removed");
    let run = grana_pipeline::read_run(temp.path())
        .expect("the run record reads")
        .expect("the folder holds a run record");
    assert_eq!(run.datasets()[0].correction(), None);
}

#[test]
fn marks_edits_unsaved_and_recovers_an_autosaved_analysis() {
    let temp = Temp::new("grana-recovery");
    let root = temp.path().join("recovery");
    let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/imaris-filaments.ims");
    let mut session = Session::new();
    session.attach_recovery_at(root.clone());
    session.begin_load(fixture);
    wait_for_scene(&mut session, 1);
    assert!(
        !session.dirty(),
        "a freshly opened dataset carries no unsaved work"
    );
    session.execute(Command::SetChannelOpacity {
        channel: 0,
        opacity: 0.4,
    });
    assert!(session.dirty(), "an edit leaves the analysis unsaved");
    assert!(session.autosave(view_state()), "the autosave starts");
    let deadline = Instant::now() + Duration::from_secs(120);
    while session.recovery().is_none() {
        assert!(Instant::now() < deadline, "the autosave never finishes");
        session.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(session.dirty(), "an autosave does not pass for a save");

    let mut recovered = Session::new();
    recovered.attach_recovery_at(root);
    let offered = recovered.recovery().expect("the recovery is offered");
    assert_eq!(offered.name, "imaris-filaments");
    recovered.execute(Command::RecoverAnalysis);
    wait_for_scene(&mut recovered, 1);
    assert!(
        recovered.dirty(),
        "recovered work is unsaved until it is saved again"
    );
    assert_eq!(
        recovered.scene().expect("the fixture opens").channels[0].opacity,
        0.4,
        "the recovered analysis keeps its display state"
    );
    assert!(
        recovered.recovery().is_none(),
        "the recovery is spent once it is loaded"
    );
}

#[test]
fn refuses_a_project_written_by_another_schema() {
    let temp = Temp::new("grana-project-schema");
    let mut session = Session::new();
    session.open_series("nuclei", series());
    let scene = session.scene().expect("the series opens");
    let project = Project::capture(
        scene,
        session.measure(),
        Source::Synthetic {
            name: "nuclei".to_string(),
        },
        view_state(),
        Vec::new(),
        Fingerprint::new([3; 32]),
    );
    let path = temp.path().join("schema.grana");
    project
        .write(&path, &AtomicBool::new(false), &mut |_, _| {})
        .expect("the project writes");
    let mut bytes = std::fs::read(&path).expect("the project reads back");
    bytes[9..13].copy_from_slice(&1u32.to_le_bytes());
    std::fs::write(&path, &bytes).expect("the header is patched");
    let error = Project::read(
        &path,
        GeometryStore::memory(),
        &AtomicBool::new(false),
        &mut |_, _| {},
    )
    .expect_err("another schema is refused");
    assert!(
        format!("{error}").contains("schema"),
        "the error names the schema: {error}"
    );
}

#[test]
fn grows_cells_around_nuclei_through_a_project_and_protocol() {
    let mut session = Session::new();
    session.open_series("neuromast", cells_series());
    session.execute(Command::SelectChannel(0));
    session.execute(Command::AddObject(ObjectKind::Surfaces));
    let nuclei = last_surfaces(&session);
    session.execute(Command::SetObjectParams {
        set: nuclei,
        params: ObjectParams::Surfaces(SurfaceParams::new(
            0.5,
            0.0,
            0.0,
            0.0,
            SurfaceFilter::new(0.0, 0.0, 0.0, 0.0),
        )),
    });
    session.execute(Command::Detect(nuclei));
    settle(&mut session);
    assert_eq!(objects(&session, nuclei), 2, "the two nuclei are detected");

    session.execute(Command::SelectChannel(1));
    session.execute(Command::AddObject(ObjectKind::Cells));
    let cells = last_cells(&session);
    session.execute(Command::SetSeedSource {
        set: cells,
        source: Some(nuclei),
    });
    session.execute(Command::SetObjectParams {
        set: cells,
        params: ObjectParams::Cells(CellParams::new(2.0, 0.0)),
    });
    session.execute(Command::Detect(cells));
    settle(&mut session);

    let scene = session.scene().expect("the series stays open");
    let set = scene.objects.get(cells).expect("the cells stay staged");
    assert_eq!(set.kind(), ObjectKind::Cells);
    let frame = set.frame(0).expect("the cells grow on the first frame");
    let ObjectFrame::Cells(frame) = frame else {
        panic!("the growth installs a cell frame");
    };
    assert_eq!(frame.object_count(), 2, "each nucleus grows one cell");
    for index in 0..2 {
        let body = frame.body(index);
        let nucleus = frame.nucleus(index);
        assert!(
            nucleus.volume() > 0.0 && body.volume() > nucleus.volume(),
            "a cell body holds its nucleus"
        );
        assert!(
            body.volume() < 4.0,
            "the membrane bounds the cell body: {}",
            body.volume()
        );
        assert!(
            nucleus.volume() / body.volume() < 1.0,
            "a nucleus never fills its cell"
        );
    }

    session.execute(Command::CaptureProtocol);
    let protocol = session.protocol().expect("the protocol captures");
    let text = protocol.to_json().expect("the protocol serializes");
    assert!(
        text.contains("\"cells\""),
        "the protocol names the cell growth: {text}"
    );
    let restored = Protocol::from_json(&text).expect("the protocol parses");
    assert_eq!(&restored, protocol);

    session.execute(Command::ApplyProtocol);
    settle(&mut session);
    let scene = session.scene().expect("the series stays open");
    let rebuilt = scene
        .objects
        .iter()
        .find(|set| set.kind() == ObjectKind::Cells)
        .and_then(|set| set.frame(0))
        .map(ObjectFrame::object_count);
    assert_eq!(
        rebuilt,
        Some(2),
        "the protocol replay grows the cells again"
    );

    let scene = session.scene().expect("the series stays open");
    let project = Project::capture(
        scene,
        session.measure(),
        Source::Synthetic {
            name: "neuromast".to_string(),
        },
        view_state(),
        Vec::new(),
        Fingerprint::new([9; 32]),
    );
    let temp = Temp::new("suite-cells");
    let path = temp.path().join("cells.grana");
    project
        .write(&path, &AtomicBool::new(false), &mut |_, _| {})
        .expect("the cell project writes");
    let loaded = Project::read(
        &path,
        GeometryStore::memory(),
        &AtomicBool::new(false),
        &mut |_, _| {},
    )
    .expect("the cell project reads");
    assert_eq!(loaded, project, "the cells survive a project");
}

#[test]
fn a_viewer_keeps_a_recorded_analysis_read_only() {
    let mut session = Session::viewer();
    session.open_series("neuromast", cells_series());
    session.execute(Command::SelectChannel(1));
    session.execute(Command::AddObject(ObjectKind::Spots));
    assert!(
        session.message().is_some(),
        "the viewer explains why the change was refused"
    );
    session.execute(Command::SetChannelEnabled {
        channel: 0,
        enabled: false,
    });
    let scene = session.scene().expect("the series opens");
    assert_eq!(
        scene.selected, 1,
        "the viewer follows the requested channel"
    );
    assert!(
        scene.objects.iter().next().is_none(),
        "the viewer stages no object set"
    );
    assert!(
        !scene.channels[0].enabled,
        "the viewer keeps the display controls"
    );
}

#[test]
fn a_project_finds_its_dataset_beside_itself() {
    let temp = Temp::new("suite-beside");
    let fixture =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ome.tif");
    let elsewhere = temp.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).expect("the dataset folder is created");
    let moved = elsewhere.join("dataset.ome.tif");
    std::fs::copy(&fixture, &moved).expect("the fixture copies");
    let mut session = Session::new();
    session.begin_load(moved);
    let deadline = Instant::now() + Duration::from_secs(60);
    while session.scene().is_none() {
        assert!(Instant::now() < deadline, "the dataset never opens");
        session.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
    let shared = temp.path().join("shared");
    std::fs::create_dir_all(&shared).expect("the shared folder is created");
    let project = shared.join("shared.grana");
    session.save_project(project.clone(), view_state());
    let deadline = Instant::now() + Duration::from_secs(60);
    while session.project().is_none() {
        assert!(Instant::now() < deadline, "the project never saves");
        session.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
    std::fs::remove_dir_all(&elsewhere).expect("the recorded folder is removed");
    std::fs::copy(&fixture, shared.join("dataset.ome.tif")).expect("the dataset moves beside it");
    let mut viewer = Session::viewer();
    viewer.begin_project(project);
    let deadline = Instant::now() + Duration::from_secs(60);
    while viewer.scene().is_none() {
        assert!(
            Instant::now() < deadline,
            "the project never finds its dataset"
        );
        viewer.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn exports_a_share_package_that_opens_read_only() {
    let temp = Temp::new("suite-package");
    let fixture =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ome.tif");
    let dataset = temp.path().join("sample.ome.tif");
    std::fs::copy(&fixture, &dataset).expect("the fixture copies");
    let mut session = Session::new();
    session.begin_load(dataset);
    let deadline = Instant::now() + Duration::from_secs(60);
    while session.scene().is_none() {
        assert!(Instant::now() < deadline, "the dataset never opens");
        session.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
    session.execute(Command::AddObject(ObjectKind::Spots));
    session.observe_view(view_state());
    let shared = temp.path().join("shared");
    std::fs::create_dir_all(&shared).expect("the shared folder is created");
    assert!(
        session.package_into(shared.clone()),
        "the package starts: {:?}",
        session.message().map(|message| message.text().to_string())
    );
    let find = || {
        std::fs::read_dir(&shared)
            .ok()?
            .flatten()
            .map(|entry| entry.path())
            .find(|path| grana_project::is_package(path))
    };
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut package = None;
    while package.is_none() {
        assert!(Instant::now() < deadline, "the package never appears");
        session.poll();
        package = find();
        std::thread::sleep(Duration::from_millis(2));
    }
    let package = package.expect("the package is written");
    let deadline = Instant::now() + Duration::from_secs(60);
    while session.writing() {
        assert!(Instant::now() < deadline, "the package never finishes");
        session.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
    let detail = session
        .message()
        .map(|message| message.text().to_string())
        .unwrap_or_default();
    assert!(
        package.join("analysis.grana").is_file(),
        "the package holds no analysis: {detail}"
    );
    assert!(
        package.join("data/sample.ome.tif").is_file(),
        "the package holds no dataset: {detail}"
    );
    assert!(
        package.join("protocol.json").is_file(),
        "the package holds no protocol: {detail}"
    );
    assert!(
        package.join("ro-crate-metadata.json").is_file(),
        "the package holds no report: {detail}"
    );
    let mut viewer = Session::new();
    viewer.open_path(package);
    let deadline = Instant::now() + Duration::from_secs(60);
    while viewer.scene().is_none() {
        assert!(Instant::now() < deadline, "the package never opens");
        viewer.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(
        viewer.project().is_some(),
        "the package opens its recorded analysis"
    );
    assert!(viewer.access().viewer(), "a share package opens read only");
    viewer.execute(Command::AddObject(ObjectKind::Spots));
    assert!(
        viewer.message().is_some(),
        "the packaged analysis refuses edits"
    );
}

fn cells_series() -> VolumeSeries {
    let extent = Extent::new(64, 32, 32);
    let mut nuclei = vec![0u16; extent.voxel_count() as usize];
    let mut membranes = vec![0u16; extent.voxel_count() as usize];
    let cells = [
        ([16.0f32, 16.0, 16.0], 3.0f32, 7.0f32),
        ([40.0f32, 16.0, 16.0], 5.0f32, 10.0f32),
    ];
    for z in 0..extent.z() {
        for y in 0..extent.y() {
            for x in 0..extent.x() {
                let point = [x as f32, y as f32, z as f32];
                let index = extent.index(x, y, z) as usize;
                for (centre, nucleus, body) in cells {
                    let offset = [0, 1, 2]
                        .map(|axis| (point[axis] - centre[axis]).powi(2))
                        .into_iter()
                        .sum::<f32>()
                        .sqrt();
                    if offset <= nucleus {
                        nuclei[index] = HIGH;
                    }
                    if (offset - body).abs() <= 1.0 {
                        membranes[index] = HIGH;
                    }
                }
            }
        }
    }
    let volume = Volume::new(
        extent,
        Spacing::new(0.1, 0.1, 0.1),
        vec![
            Channel::new("nuclei", [0.3, 0.6, 1.0], VoxelData::U16(nuclei)),
            Channel::new("membranes", [1.0, 0.6, 0.3], VoxelData::U16(membranes)),
        ],
    );
    VolumeSeries::new(vec![volume], 1.0)
}

fn last_cells(session: &Session) -> grana_scene::ObjectSetId {
    session
        .scene()
        .expect("the series opens")
        .objects
        .iter()
        .filter(|set| set.kind() == ObjectKind::Cells)
        .map(|set| set.id())
        .last()
        .expect("the cell set is staged")
}

fn wait_for_scene(session: &mut Session, frames: u32) {
    let deadline = Instant::now() + Duration::from_secs(120);
    while session.scene().is_none() {
        assert!(Instant::now() < deadline, "the dataset never opens");
        session.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
    let scene = session.scene().expect("the dataset opens");
    assert_eq!(scene.frame_count(), frames, "the dataset keeps its frames");
}
