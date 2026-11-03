use std::time::{Duration, Instant};

use grana_display::{CameraState, SectionState};
use grana_project::ViewState;
use grana_session::Session;
use grana_volume::Axis;

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
