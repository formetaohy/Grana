use std::time::{Duration, Instant};

use grana_scene::{ObjectFrame, ObjectKind, ObjectParams};
use grana_session::{Command, Session};

fn settle(session: &mut Session) {
    let deadline = Instant::now() + Duration::from_secs(240);
    while !session.progress().is_empty() {
        assert!(
            Instant::now() < deadline,
            "the sample analysis never settled"
        );
        session.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn objects(session: &Session, set: grana_scene::ObjectSetId) -> usize {
    session
        .scene()
        .expect("the sample opens")
        .objects
        .get(set)
        .and_then(|set| set.frame(0))
        .map(ObjectFrame::object_count)
        .expect("the first frame is detected")
}

#[test]
fn recuts_surfaces_when_a_cut_parameter_changes() {
    let mut session = Session::new();
    session.open_sample();
    settle(&mut session);
    let set = session
        .scene()
        .expect("the sample opens")
        .objects
        .iter()
        .find(|set| set.kind() == ObjectKind::Surfaces)
        .expect("the sample stages surfaces")
        .id();
    let params = session
        .scene()
        .expect("the sample opens")
        .objects
        .get(set)
        .expect("the surfaces exist")
        .params();
    let ObjectParams::Surfaces(mut surfaces) = params else {
        panic!("the surface set detects surfaces");
    };
    let detected = objects(&session, set);
    assert!(detected > 0, "the sample detection produces surfaces");

    let volume = surfaces.min_volume();
    surfaces.set_min_volume(1.0e6);
    session.execute(Command::SetObjectParams {
        set,
        params: ObjectParams::Surfaces(surfaces),
    });
    settle(&mut session);
    assert_eq!(
        objects(&session, set),
        0,
        "the recut applies the volume filter without reading the volume"
    );

    surfaces.set_min_volume(volume);
    session.execute(Command::SetObjectParams {
        set,
        params: ObjectParams::Surfaces(surfaces),
    });
    settle(&mut session);
    assert_eq!(
        objects(&session, set),
        detected,
        "the recut restores every object the first detection kept"
    );
}
