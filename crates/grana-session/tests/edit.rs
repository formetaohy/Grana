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
        objects: vec![0, 1],
    });
    let derived = edited(&session);
    let scene = session.scene().expect("the sample opens");
    for frame in 0..scene.frame_count() {
        assert_eq!(
            objects(&session, derived, frame),
            objects(&session, set, frame) - 1,
            "the merge follows the tracks into frame {frame}"
        );
    }
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

#[test]
fn editing_the_edited_set_extends_its_plan() {
    let mut session = opened();
    let set = surfaces(&session);
    let time = session.scene().expect("the sample opens").time();
    let frames = session.scene().expect("the sample opens").frame_count();
    let before = objects(&session, set, time);
    assert!(before >= 3);
    session.execute(Command::MergeSurfaces {
        set,
        time,
        objects: vec![0, 1],
    });
    settle(&mut session);
    let derived = edited(&session);
    assert_eq!(
        session
            .scene()
            .expect("the sample opens")
            .objects
            .get(derived)
            .expect("the edit derives an object set")
            .provenance()
            .edits()
            .len(),
        frames as usize,
        "the merge records one edit per frame"
    );
    session.execute(Command::SplitSurface {
        set: derived,
        time,
        object: 0,
    });
    settle(&mut session);
    let scene = session.scene().expect("the sample opens");
    let edited_sets = scene
        .objects
        .iter()
        .filter(|set| matches!(set.provenance(), Provenance::Edited { .. }))
        .collect::<Vec<_>>();
    assert_eq!(edited_sets.len(), 1, "an edit extends the same object set");
    assert_eq!(
        edited_sets[0].provenance().edits().len(),
        2 * frames as usize
    );
    assert_eq!(
        edited_sets[0]
            .frame(time)
            .map_or(0, ObjectFrame::object_count),
        before
    );
}
