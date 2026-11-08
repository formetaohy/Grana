use std::time::{Duration, Instant};

use grana_scene::{ObjectFrame, ObjectKind, ObjectSetId, Provenance};
use grana_session::{Action, Command, Session};

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

fn opened() -> Session {
    let mut session = Session::new();
    session.open_sample();
    settle(&mut session);
    session
}

fn surfaces(session: &Session) -> ObjectSetId {
    session
        .scene()
        .expect("the sample opens")
        .objects
        .iter()
        .find(|set| set.kind() == ObjectKind::Surfaces)
        .expect("the sample detects surfaces")
        .id()
}

fn edited(session: &Session) -> ObjectSetId {
    session
        .scene()
        .expect("the sample opens")
        .objects
        .iter()
        .find(|set| matches!(set.provenance(), Provenance::Edited { .. }))
        .expect("the edit derives an object set")
        .id()
}

fn objects(session: &Session, set: ObjectSetId, time: u32) -> usize {
    session
        .scene()
        .expect("the sample opens")
        .objects
        .get(set)
        .and_then(|set| set.frame(time))
        .map_or(0, ObjectFrame::object_count)
}

#[test]
fn merging_two_surfaces_derives_an_edited_set() {
    let mut session = opened();
    let set = surfaces(&session);
    let time = session.scene().expect("the sample opens").time();
    let before = objects(&session, set, time);
    assert!(before >= 2, "the sample detects two surfaces");
    session.execute(Command::MergeSurfaces {
        set,
        time,
        source: 0,
        target: 1,
    });
    let derived = edited(&session);
    assert_eq!(objects(&session, derived, time), before - 1);
    assert_eq!(session.history().undo, Some(Action::EditObjects));
    assert_eq!(session.measure().set(), Some(derived));
    {
        let scene = session.scene().expect("the sample opens");
        let source = scene.objects.get(set).expect("the edited set remains");
        assert!(!source.visible(), "the source set hides behind the edit");
        assert_eq!(source.frames().count(), scene.frame_count() as usize);
    }
    session.execute(Command::Undo);
    assert!(
        session
            .scene()
            .expect("the sample opens")
            .objects
            .iter()
            .all(|set| !matches!(set.provenance(), Provenance::Edited { .. })),
        "undo removes the derived set"
    );
}
