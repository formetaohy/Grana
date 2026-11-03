use std::time::{Duration, Instant};

use grana_display::{CameraState, SectionState};
use grana_project::ViewState;
use grana_scene::{ObjectFrame, ObjectKind, ObjectParams};
use grana_session::{Command, Session};
use grana_volume::Axis;

#[test]
fn reopens_a_project_with_its_surface_topology() {
    let mut session = Session::new();
    session.open_sample();
    wait(&mut session, Duration::from_secs(240));
    let surfaces = surface_set(&session);
    let params = session
        .scene()
        .expect("the sample opens")
        .objects
        .get(surfaces)
        .expect("the sample stages surfaces")
        .params();
    let ObjectParams::Surfaces(mut surfaces_params) = params else {
        panic!("the sample stages a surface set");
    };
    let detected = objects(&session, surfaces);
    assert!(detected > 0, "the sample detection produces surfaces");
    let path = std::env::temp_dir().join(format!("grana-topology-{}.grana", std::process::id()));
    std::fs::remove_file(&path).ok();
    session.save_project(path.clone(), view(&session));
    wait(&mut session, Duration::from_secs(60));

    let mut reopened = Session::new();
    reopened.begin_project(path.clone());
    wait(&mut reopened, Duration::from_secs(240));
    std::fs::remove_file(&path).ok();
    let surfaces = surface_set(&reopened);
    let frame = reopened
        .scene()
        .expect("the project opens its dataset")
        .objects
        .get(surfaces)
        .and_then(|set| set.frame(0))
        .expect("the project keeps its first surface frame");
    let ObjectFrame::Surfaces(frame) = frame else {
        panic!("the surface frame restores as surfaces");
    };
    assert!(
        frame.cuttable(),
        "the reopened surface keeps its watershed topology"
    );

    surfaces_params.set_min_volume(1.0e6);
    reopened.execute(Command::SetObjectParams {
        set: surfaces,
        params: ObjectParams::Surfaces(surfaces_params),
    });
    wait(&mut reopened, Duration::from_secs(60));
    assert_eq!(
        objects(&reopened, surfaces),
        0,
        "the reopened surface recuts its filters without reading the volume"
    );
    surfaces_params.set_min_volume(0.0);
    reopened.execute(Command::SetObjectParams {
        set: surfaces,
        params: ObjectParams::Surfaces(surfaces_params),
    });
    wait(&mut reopened, Duration::from_secs(60));
    assert_eq!(
        objects(&reopened, surfaces),
        detected,
        "the reopened surface restores every object it detected"
    );
}

#[test]
fn recuts_a_surface_set_after_a_hand_deleted_object() {
    let mut session = Session::new();
    session.open_sample();
    wait(&mut session, Duration::from_secs(240));
    let surfaces = surface_set(&session);
    let detected = objects(&session, surfaces);
    assert!(detected > 1, "the sample detects several surfaces");
    session.execute(Command::DeleteObject {
        set: surfaces,
        time: 0,
        object: 0,
    });
    assert_eq!(
        objects(&session, surfaces),
        detected - 1,
        "the hand deleted object leaves the frame"
    );
    let params = session
        .scene()
        .expect("the sample opens")
        .objects
        .get(surfaces)
        .expect("the sample stages surfaces")
        .params();
    let ObjectParams::Surfaces(mut surfaces_params) = params else {
        panic!("the sample stages a surface set");
    };
    let frame = session
        .scene()
        .expect("the scene exists")
        .objects
        .get(surfaces)
        .and_then(|set| set.frame(0))
        .expect("the first frame is detected");
    let ObjectFrame::Surfaces(frame) = frame else {
        panic!("the first frame holds surfaces");
    };
    assert!(
        frame.cuttable(),
        "a hand deleted object keeps the watershed topology"
    );

    surfaces_params.set_min_volume(1.0e6);
    session.execute(Command::SetObjectParams {
        set: surfaces,
        params: ObjectParams::Surfaces(surfaces_params),
    });
    wait(&mut session, Duration::from_secs(60));
    assert_eq!(
        objects(&session, surfaces),
        0,
        "the hand edited surface recuts its filters"
    );
    surfaces_params.set_min_volume(0.0);
    session.execute(Command::SetObjectParams {
        set: surfaces,
        params: ObjectParams::Surfaces(surfaces_params),
    });
    wait(&mut session, Duration::from_secs(60));
    assert_eq!(
        objects(&session, surfaces),
        detected - 1,
        "the hand deleted object never returns from a recut"
    );
}

fn surface_set(session: &Session) -> grana_scene::ObjectSetId {
    session
        .scene()
        .expect("the scene exists")
        .objects
        .iter()
        .find(|set| set.kind() == ObjectKind::Surfaces)
        .expect("the scene stages surfaces")
        .id()
}

fn objects(session: &Session, set: grana_scene::ObjectSetId) -> usize {
    session
        .scene()
        .expect("the scene exists")
        .objects
        .get(set)
        .and_then(|set| set.frame(0))
        .map(ObjectFrame::object_count)
        .expect("the first frame is detected")
}

#[test]
fn transfers_a_project_off_the_caller() {
    let mut session = Session::new();
    session.open_sample();
    let name = session
        .scene()
        .expect("the sample opens a dataset")
        .dataset
        .name()
        .to_string();
    wait(&mut session, Duration::from_secs(30));
    let path = std::env::temp_dir().join(format!("grana-session-{}.grana", std::process::id()));
    std::fs::remove_file(&path).ok();

    session.save_project(path.clone(), view(&session));
    assert!(
        session.transferring(),
        "the save leaves the calling thread before it finishes"
    );
    wait(&mut session, Duration::from_secs(30));
    assert_eq!(session.project(), Some(path.as_path()));
    assert!(!session.dirty());

    let mut reopened = Session::new();
    reopened.begin_project(path.clone());
    wait(&mut reopened, Duration::from_secs(30));
    assert_eq!(
        reopened
            .scene()
            .expect("the project opens its dataset")
            .dataset
            .name(),
        name
    );
    assert_eq!(reopened.project(), Some(path.as_path()));
    std::fs::remove_file(&path).ok();
}

fn view(session: &Session) -> ViewState {
    ViewState {
        camera: CameraState {
            pivot: [0.0, 0.0, 0.0],
            yaw: 0.0,
            pitch: 0.0,
            distance: 10.0,
            fov_y: 0.6,
        },
        section: SectionState {
            axis: Axis::Z,
            span: 20.0,
        },
        split: 0.5,
        timeline: session.timeline().clone(),
        export: grana_session::Export::NATIVE,
    }
}

fn wait(session: &mut Session, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while session.transferring() || !session.progress().is_empty() {
        assert!(Instant::now() < deadline, "the session settles");
        session.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
}
