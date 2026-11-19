use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use glam::Vec3;
use grana_animation::Timeline;
use grana_display::{Camera, CameraState};
use grana_engine::{Engine, SETTLE};
use grana_object::{
    IntensityTable, MaskMode, MaskTool, ObjectAnchor, SampleStatistics, Spot, SpotParams, SpotSet,
    SurfaceFilter, SurfaceParams,
};
use grana_project::{Export, NamedView, Project, Source, ViewState};
use grana_protocol::{NamedViewSpec, Protocol, ViewSpec};
use grana_render::Delivery;
use grana_scene::{Dataset, Measure, ObjectFrame, ObjectKind, ObjectParams, Scene};
use grana_session::{Command, Session};
use grana_store::SpillStore;
use grana_volume::{
    BrickCoord, BrickRequest, Channel, Extent, Fingerprint, Region, Spacing, Volume, VolumeSeries,
    VoxelData,
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

    session.request(0, &[BrickRequest::new(0, 0, BrickCoord::new(1, 0, 0))], &[]);
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
    let packed = &bricks[0].1;
    assert_eq!(packed.channels(), 2);
    let mask = packed
        .pack(0)
        .expect("the mask channel shares the first pack");
    assert!(
        mask.rows()
            .iter()
            .skip(1)
            .step_by(2)
            .any(|unit| *unit == HIGH),
        "the mask channel keeps its stored values"
    );

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

#[test]
fn undoes_a_computed_drift_correction() {
    let volume = support::spheres(
        Extent::new(64, 32, 32),
        Spacing::new(0.1, 0.1, 0.1),
        &[([26.0, 16.0, 16.0], 3.0), ([38.0, 16.0, 16.0], 5.0)],
        HIGH,
    );
    let mut session = Session::new();
    session.open_series(
        "drift",
        VolumeSeries::new(vec![volume.clone(), volume], 1.0),
    );
    session.execute(Command::ComputeDrift);
    settle(&mut session);
    let drift = session
        .scene()
        .expect("the series opens")
        .drift(0)
        .cloned()
        .expect("the drift correction installs");
    assert_eq!(drift.frames(), 2, "the correction aligns both frames");
    session.execute(Command::Undo);
    assert!(
        session
            .scene()
            .expect("the series opens")
            .drift(0)
            .is_none(),
        "undo removes the computed drift correction"
    );
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
        .and_then(|set| set.frame_shared(0))
        .map_or(0, |frame| frame.object_count())
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
    let deadline = Instant::now() + Duration::from_secs(60);
    while session.writing() {
        assert!(Instant::now() < deadline, "the graph export never finishes");
        session.poll();
        std::thread::sleep(Duration::from_millis(1));
    }
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
        imported
            .frame_shared(0)
            .map_or(0, |frame| frame.object_count()),
        2,
        "the imported graph restores both surfaces"
    );
    assert!(
        imported
            .frame_shared(0)
            .is_some_and(|frame| frame.geometry().is_ok()),
        "the restored surfaces carry their meshes"
    );
}

#[test]
fn imports_anisotropic_localizations_and_round_trips_them() {
    let temp = Temp::new("session-localizations");
    let path = temp.path().join("localizations.csv");
    std::fs::write(
        &path,
        "frame,\"x [nm]\",\"y [nm]\",\"z [nm]\",\"uncertainty_xy [nm]\",\"uncertainty_z [nm]\",\"intensity [photon]\"\n\
         0,1000,500,1600,20,40,3000\n\
         0,2000,1000,800,25,45,2000\n\
         1,1000,500,1600,20,40,2500\n",
    )
    .expect("the localization table writes");
    let volume = support::spheres(
        Extent::new(64, 32, 32),
        Spacing::new(0.1, 0.1, 0.1),
        &[([10.0, 5.0, 16.0], 4.0)],
        HIGH,
    );
    let mut session = Session::new();
    session.open_series(
        "localizations",
        VolumeSeries::new(vec![volume.clone(), volume], 1.0),
    );
    assert!(session.import_localizations(&path), "the table imports");
    settle(&mut session);
    let scene = session.scene().expect("the series stays open");
    let set = scene
        .objects
        .iter()
        .find(|set| set.kind() == ObjectKind::Spots)
        .expect("the localization set lands")
        .id();
    let object = scene.objects.get(set).expect("the localization set stays");
    let frame = object.frame_shared(0).expect("the first frame lands");
    let spots = frame.spots().expect("the frame carries spots");
    assert_eq!(spots.len(), 2, "the first frame carries its two spots");
    assert_eq!(
        object.frames().count(),
        2,
        "the table spans both frames of the series"
    );
    let spot = spots.spots()[0];
    for (axis, expected) in spot.center().iter().zip([-2.2, -1.1, 0.0]) {
        assert!(
            (axis - expected).abs() < 1.0e-4,
            "the localization lands at {axis} instead of {expected}"
        );
    }
    for (axis, expected) in spot.sigma().iter().zip([0.02, 0.02, 0.04]) {
        assert!(
            (axis - expected).abs() < 1.0e-5,
            "the localization carries the precision {axis} instead of {expected}"
        );
    }
    assert!((spot.quality() - 3000.0).abs() < 1.0e-3);
    assert!(
        (spots.intensities().object(0, 0).mean() - f32::from(HIGH)).abs() < 1.0,
        "the localization samples the voxel it lands on"
    );
    assert!(
        object
            .tracks()
            .is_some_and(|tracks| !tracks.set().is_empty()),
        "the imported localizations track across their frames"
    );
    let protocol = Protocol::capture(scene, &Measure::new(), None, Vec::new(), None)
        .expect("the localization scene captures");
    let text = protocol.to_toml().expect("the protocol serializes");
    assert!(
        text.contains("localizations.csv"),
        "the protocol records the localization table"
    );
    assert!(
        text.contains("kind = \"localizations\""),
        "the protocol names the localization source"
    );
    assert_eq!(
        Protocol::from_toml(&text).expect("the protocol parses"),
        protocol
    );
    let molecules = protocol.sets()[0]
        .source
        .molecules()
        .expect("the protocol carries the molecule parameters");
    assert_eq!(molecules.frames, [0, 1]);
    assert_eq!((molecules.pixel, molecules.blur), (10.0, 20.0));
    assert!(!molecules.projected && !molecules.clustered);
    let project = Project::capture(
        scene,
        &Measure::new(),
        Source::Synthetic {
            name: "localizations".to_string(),
        },
        view_state(),
        Vec::new(),
        Fingerprint::new([5; 32]),
    );
    let document = temp.path().join("localizations.grana");
    project
        .write(&document, &AtomicBool::new(false), &mut |_, _| {})
        .expect("the project writes");
    let loaded = Project::read(
        &document,
        SpillStore::memory(),
        &AtomicBool::new(false),
        &mut |_, _| {},
    )
    .expect("the project reads");
    assert_eq!(loaded, project);
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
    let frame = set
        .frame_shared(0)
        .expect("the filament holds its first frame");
    let grana_scene::ObjectFrame::Filaments(filaments) = frame.as_ref() else {
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
fn undoes_a_named_view_with_its_timeline() {
    let mut session = Session::new();
    session.open_series("views", series());
    let saved = view_state();
    session.observe_view(saved.clone());
    session.execute(Command::SaveView);
    assert_eq!(session.views().len(), 1, "the view joins the document");
    session.execute(Command::SetTimelineFps(12));
    assert_eq!(session.timeline().fps(), 12);
    let later = ViewState {
        timeline: Timeline::new(12, 10.0),
        camera: CameraState {
            yaw: 0.9,
            ..saved.camera
        },
        ..view_state()
    };
    session.observe_view(later.clone());
    session.execute(Command::ApplyView(0));
    assert_eq!(
        session.history().undo,
        Some(grana_session::Action::NamedView),
        "applying a view is an undoable edit"
    );
    assert_eq!(session.timeline().fps(), saved.timeline.fps());
    let applied = session
        .take_view()
        .expect("the applied view reaches the renderer");
    assert_eq!(applied.camera.yaw, saved.camera.yaw);
    session.observe_view(applied);
    session.execute(Command::Undo);
    assert_eq!(
        session.timeline().fps(),
        12,
        "undo restores the timeline the view replaced"
    );
    let restored = session
        .take_view()
        .expect("undo returns the camera the view replaced");
    assert_eq!(restored.camera.yaw, later.camera.yaw);
    session.observe_view(restored);
    session.execute(Command::Redo);
    assert_eq!(session.timeline().fps(), saved.timeline.fps());
    let reapplied = session.take_view().expect("redo applies the view again");
    assert_eq!(reapplied.camera.yaw, saved.camera.yaw);
    session.execute(Command::Undo);
    session.execute(Command::Undo);
    assert_eq!(session.timeline().fps(), 30, "the frame rate reverts too");
    session.execute(Command::Undo);
    assert!(session.views().is_empty(), "undo drops the saved view");
    session.execute(Command::Redo);
    assert_eq!(session.views().len(), 1, "redo saves the view again");
    let posed = ViewState {
        camera: CameraState {
            yaw: 1.4,
            ..saved.camera
        },
        ..saved
    };
    session.observe_view(posed.clone());
    session.execute(Command::SaveView);
    assert_eq!(session.views().len(), 2);
    let elsewhere = ViewState {
        camera: CameraState {
            yaw: -0.6,
            ..posed.camera
        },
        ..posed
    };
    session.observe_view(elsewhere.clone());
    session.execute(Command::ApplyView(1));
    assert_eq!(
        session.history().undo,
        Some(grana_session::Action::NamedView),
        "a view that only moves the camera edits the document too"
    );
    let moved = session
        .take_view()
        .expect("the applied camera reaches the renderer");
    assert_eq!(moved.camera.yaw, posed.camera.yaw);
    session.observe_view(moved);
    session.execute(Command::Undo);
    let back = session
        .take_view()
        .expect("undo returns the camera the view replaced");
    assert_eq!(back.camera.yaw, elsewhere.camera.yaw);
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
        ObjectFrame::from_spots(SpotSet::new(
            0,
            vec![
                Spot::new([0.0, 0.0, 0.0], 1.0, 0.5),
                Spot::new([1.0, 0.0, 0.0], 1.0, 0.5),
            ],
            IntensityTable::new(
                1,
                vec![SampleStatistics::new(500.0, 500.0, 500.0, 0.0, 500.0); 2],
            ),
        )),
    );
    let visibility = grana_scene::Visibility::Objects {
        mask: std::sync::Arc::new(grana_measure::Mask::of(2, |row| row == 0)),
        frames: std::sync::Arc::new(vec![0u32]),
    };
    assert!(
        scene.objects.set_visibility(spots, visibility.clone()),
        "a statistics filter excludes the second spot"
    );
    scene.restore(scene.state());
    assert_eq!(
        scene
            .objects
            .get(spots)
            .map(grana_scene::ObjectSet::visibility),
        Some(&visibility),
        "a restored scene keeps the objects its filters excluded"
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
        SpillStore::memory(),
        &AtomicBool::new(false),
        &mut |_, _| {},
    )
    .expect("the project reads");
    assert_eq!(loaded, project);
    assert_ne!(
        loaded.scene.sets[0].visibility,
        grana_scene::Visibility::All,
        "a project keeps the objects its statistics filters exclude"
    );
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
    let text = protocol.to_toml().expect("the protocol serializes");
    let restored = Protocol::from_toml(&text).expect("the protocol parses");
    assert_eq!(restored, protocol);
    assert_eq!(restored.label_images().len(), 1);
    assert_eq!(restored.views(), [named]);
    assert_eq!(
        restored.snapshot().expect("the protocol snapshots").views(),
        ["Overview"]
    );
    let mut broken: toml::Value = toml::from_str(&text).expect("the protocol is toml");
    broken["snapshot"]["views"][0] = toml::Value::String("Missing".to_string());
    assert!(
        Protocol::from_toml(&toml::to_string(&broken).expect("the protocol serializes")).is_err(),
        "a snapshot rejects a view the protocol does not carry"
    );
    let mut filmed: toml::Value = toml::from_str(&text).expect("the protocol is toml");
    let snapshot = filmed["snapshot"]
        .as_table_mut()
        .expect("the protocol snapshots");
    snapshot.insert("movies".to_string(), toml::Value::Boolean(true));
    snapshot.insert("sequences".to_string(), toml::Value::Boolean(true));
    let filmed = Protocol::from_toml(&toml::to_string(&filmed).expect("the protocol serializes"))
        .expect("a snapshot films its views");
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
    session.execute(Command::AddObject(ObjectKind::Spots));
    assert!(session.dirty(), "an edit leaves the field unsaved");
    session.execute(Command::SelectField(0));
    open_field(&mut session, Extent::new(8, 6, 4));
    assert!(!session.dirty(), "the untouched field stays saved");
    session.execute(Command::SelectField(1));
    open_field(&mut session, Extent::new(12, 10, 6));
    assert!(
        session.dirty(),
        "a field keeps its unsaved work after switching away and back"
    );
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
        SpillStore::memory(),
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
        .and_then(|object| object.frame_shared(0))
        .expect("the object set holds a frame");
    match frame.as_ref() {
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
        SpillStore::memory(),
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
    session.set_capture_factory(std::sync::Arc::new(|| std::sync::Arc::new(StillCapture)));
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
        package.join("protocol.toml").is_file(),
        "the package holds no protocol: {detail}"
    );
    assert!(
        package.join("ro-crate-metadata.json").is_file(),
        "the package holds no manifest: {detail}"
    );
    let report =
        std::fs::read_to_string(package.join("report.html")).expect("the package writes a report");
    assert!(
        report.starts_with("<!doctype html>"),
        "the report is not a page: {detail}"
    );
    assert!(
        report.contains("data:image/png;base64,"),
        "the report embeds no thumbnail: {detail}"
    );
    assert!(
        grana_raster::read(&package.join("thumbnail.png")).is_ok(),
        "the package holds no thumbnail: {detail}"
    );
    let manifest = std::fs::read_to_string(package.join("ro-crate-metadata.json"))
        .expect("the manifest reads")
        .replace(char::is_whitespace, "");
    assert!(
        manifest.contains("\"report.html\"") && manifest.contains("\"thumbnail.png\""),
        "the manifest names the report: {detail}"
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

struct StillCapture;

impl grana_pipeline::Capture for StillCapture {
    fn open(&self) -> Result<(), String> {
        Ok(())
    }

    fn capture(
        &self,
        _channels: &grana_pipeline::Channels,
        _scene: &Scene,
        request: &grana_pipeline::CaptureRequest,
        _cancel: &std::sync::atomic::AtomicBool,
        path: &std::path::Path,
    ) -> Result<(), String> {
        let [width, height] = request.size;
        let pixels = (0..width as usize * height as usize)
            .flat_map(|index| [(index % 251) as u8, 32, 96, 255])
            .collect::<Vec<u8>>();
        let raster = grana_raster::Raster::new(width, height, pixels);
        let png = grana_raster::encode(&raster).map_err(|error| error.to_string())?;
        std::fs::write(path, png).map_err(|error| error.to_string())
    }
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

#[test]
fn a_mask_radius_restreams_the_engine_without_rebuilding_the_dataset() {
    let Some(gpu) = support::gpu() else {
        return;
    };
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
    session.execute(Command::AddMaskChannel { set });
    session.poll();
    let mut engine = Engine::new(
        gpu.device(),
        gpu.queue(),
        wgpu::TextureFormat::Rgba8Unorm,
        gpu.pipelines(),
    );
    engine.set_dataset(
        gpu.device(),
        session.scene().expect("the series opens").dataset.pyramid(),
    );
    let mut camera = Camera::new(35.0);
    camera.fit(
        session
            .scene()
            .expect("the series opens")
            .dataset
            .world_size(),
        1.0,
    );
    let size = [160, 160];
    let mut streamed = 0usize;
    let settle_engine =
        |engine: &mut Engine, session: &mut Session, streamed: &mut usize, until: Option<u64>| {
            let deadline = Instant::now() + Duration::from_secs(60);
            loop {
                assert!(
                    Instant::now() < deadline,
                    "the engine never resolved the mask"
                );
                session.poll();
                let time = session.scene().expect("the series opens").time();
                if let Some(requests) = engine
                    .poll_requests(gpu.device(), time)
                    .expect("the demand polls")
                {
                    *streamed += requests.volume.len();
                    session.request(requests.time, &requests.volume, &requests.prefetch);
                }
                let bricks = session.take_bricks();
                if !bricks.is_empty() {
                    engine.upload(&bricks);
                }
                engine
                    .draw(
                        gpu.device(),
                        gpu.queue(),
                        grana_engine::View {
                            scene: session.scene().expect("the series opens"),
                            camera: &camera,
                            size,
                            delivery: Delivery::Immediate,
                            level_cap: 99,
                            overlays: &[],
                        },
                    )
                    .expect("the viewport draws");
                let waiting = match until {
                    Some(revision) => engine.image_revision() <= revision,
                    None => engine.info().age < SETTLE,
                };
                if !waiting {
                    break;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
        };
    settle_engine(&mut engine, &mut session, &mut streamed, None);
    let pyramid = session
        .scene()
        .expect("the series opens")
        .dataset
        .pyramid()
        .clone();
    let content = session.content();
    session.execute(Command::SetMaskRadius(0.4));
    session.poll();
    let damages = session.take_damage();
    assert!(!damages.is_empty(), "a mask radius damaged no mask");
    assert_eq!(
        session.content(),
        content,
        "a mask radius reloaded the streamed content"
    );
    assert!(
        engine.holds(&pyramid),
        "a mask radius rebuilt the engine dataset"
    );
    let revision = engine.image_revision();
    engine.damage(&damages);
    streamed = 0;
    settle_engine(&mut engine, &mut session, &mut streamed, Some(revision));
    assert!(
        engine.image_revision() > revision,
        "the damaged bricks were never redrawn"
    );
    assert!(
        streamed > 0,
        "the damaged mask channel was never restreamed"
    );
}

#[test]
fn keeps_the_replay_when_an_applied_protocol_does_not_fit() {
    let mut session = Session::new();
    session.open_series("protocol", series());
    session.execute(Command::AddObject(ObjectKind::Spots));
    let scene = session.scene().expect("the series opens");
    let channels = scene.channels.len();
    let objects = scene.objects.iter().count();
    let protocol = Protocol::capture(scene, &Measure::new(), None, Vec::new(), None)
        .expect("the scene captures");
    session.set_protocol(protocol.clone());
    session.execute(Command::ApplyProtocol);
    assert!(session.replaying(), "the protocol replay starts");
    let mut value: toml::Value =
        toml::from_str(&protocol.to_toml().expect("the protocol serializes"))
            .expect("the protocol is toml");
    let recorded = value["channels"]
        .as_array_mut()
        .expect("the protocol records its channels");
    recorded[0]["name"] = toml::Value::String("Absent".to_string());
    let broken = Protocol::from_toml(&toml::to_string(&value).expect("the protocol serializes"))
        .expect("the protocol parses");
    session.set_protocol(broken);
    session.execute(Command::ApplyProtocol);
    assert_eq!(
        session.message().map(|message| message.kind()),
        Some(grana_session::Kind::Error),
        "a protocol that does not fit the dataset reports the mismatch"
    );
    assert!(
        session.replaying(),
        "a rejected protocol keeps the replay it cannot replace"
    );
    let scene = session.scene().expect("the dataset stays open");
    assert_eq!(
        scene.channels.len(),
        channels,
        "a rejected protocol leaves no derived channel behind"
    );
    assert_eq!(
        scene.objects.iter().count(),
        objects,
        "a rejected protocol keeps the analysis it cannot replay"
    );
}

#[test]
fn drops_volume_reads_that_predate_a_content_change() {
    let mut session = Session::new();
    session.open_series("stale bricks", series());
    session.execute(Command::SelectChannel(0));
    session.execute(Command::AddObject(ObjectKind::Surfaces));
    let bricks = (0..64)
        .map(|_| BrickRequest::new(0, 0, BrickCoord::new(0, 0, 0)))
        .collect::<Vec<_>>();
    session.request(0, &bricks, &[]);
    assert!(session.pending() > 0, "the brick reads are in flight");
    session.execute(Command::Undo);
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        session.poll();
        assert!(
            session.take_bricks().is_empty(),
            "a volume read from before the content change never lands"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    session.request(0, &bricks[..1], &[]);
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        session.poll();
        if !session.take_bricks().is_empty() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the volume read after the content change never completes"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}
