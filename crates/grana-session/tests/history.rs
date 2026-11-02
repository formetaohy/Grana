use std::time::{Duration, Instant};

use grana_scene::ObjectKind;
use grana_session::{Command, Session};
use grana_volume::Intensity;

fn session() -> Session {
    let mut session = Session::new();
    session.open_sample();
    session
}

fn settle(session: &mut Session) {
    let deadline = Instant::now() + Duration::from_secs(120);
    while !session.progress().is_empty() {
        assert!(
            Instant::now() < deadline,
            "the sample analysis never settled"
        );
        session.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn color(session: &Session, set: grana_scene::ObjectSetId) -> [f32; 3] {
    session
        .scene()
        .expect("the sample opens")
        .objects
        .get(set)
        .expect("the object set exists")
        .color()
}

#[test]
fn undoes_and_replays_document_edits() {
    let mut session = session();
    let sets = session.scene().expect("the sample opens").objects.len();
    session.execute(Command::AddObject(ObjectKind::Surfaces));
    let scene = session.scene().expect("the set is added");
    assert_eq!(scene.objects.len(), sets + 1);
    let set = scene
        .objects
        .iter()
        .last()
        .expect("the added set exists")
        .id();
    let original = color(&session, set);
    let edited = [0.2, 0.4, 0.8];
    session.execute(Command::SetObjectColor { set, color: edited });
    assert_eq!(color(&session, set), edited);

    session.execute(Command::Undo);
    assert_eq!(color(&session, set), original);
    assert_eq!(
        session.scene().expect("the set exists").objects.len(),
        sets + 1
    );
    session.execute(Command::Undo);
    assert_eq!(
        session.scene().expect("the set is gone").objects.len(),
        sets
    );
    assert!(session.history().undo.is_none());

    session.execute(Command::Redo);
    assert_eq!(
        session.scene().expect("the set returns").objects.len(),
        sets + 1
    );
    session.execute(Command::Redo);
    assert_eq!(color(&session, set), edited);
    assert!(session.history().redo.is_none());
}

#[test]
fn coalesces_a_continuous_edit() {
    let mut session = session();
    let channel = 0;
    let original = session.scene().expect("the sample opens").channels[channel].window;
    let first = Intensity::new(original.min(), original.value(0.5));
    let second = Intensity::new(original.min(), original.value(0.8));
    session.execute(Command::SetWindow {
        channel,
        window: first,
    });
    session.execute(Command::SetWindow {
        channel,
        window: second,
    });
    assert_eq!(
        session.scene().expect("the sample opens").channels[channel].window,
        second
    );
    session.execute(Command::Undo);
    assert_eq!(
        session.scene().expect("the sample opens").channels[channel].window,
        original
    );
    assert!(session.history().undo.is_none());
}

#[test]
fn drops_the_redo_stack_after_a_new_edit() {
    let mut session = session();
    session.execute(Command::AddObject(ObjectKind::Spots));
    session.execute(Command::Undo);
    assert!(session.history().redo.is_some());
    session.execute(Command::AddObject(ObjectKind::Filaments));
    assert!(session.history().redo.is_none());
}

#[test]
fn restores_detections_tracks_and_measurements() {
    let mut session = session();
    settle(&mut session);
    let set = session
        .scene()
        .expect("the sample opens")
        .objects
        .iter()
        .find(|set| set.kind() == ObjectKind::Spots)
        .expect("the sample stages spots")
        .id();
    let detected = session
        .scene()
        .expect("the sample opens")
        .objects
        .get(set)
        .expect("the spots exist")
        .detected();
    let tracked = session
        .scene()
        .expect("the sample opens")
        .objects
        .get(set)
        .expect("the spots exist")
        .tracks()
        .is_some();
    assert!(detected > 0, "the sample detection produces spots");

    session.execute(Command::SetMeasureSet(Some(set)));
    session.execute(Command::AddFilter);
    let filtered = session.measure().filters().len();
    assert!(filtered > 0);
    let original = color(&session, set);
    session.execute(Command::SetObjectColor {
        set,
        color: [0.1, 0.2, 0.3],
    });

    session.execute(Command::Undo);
    assert_eq!(color(&session, set), original);
    assert_eq!(session.measure().filters().len(), filtered);
    assert!(session.measure().table().is_some());

    session.execute(Command::Detect(set));
    settle(&mut session);
    session.execute(Command::Undo);
    let restored = session
        .scene()
        .expect("the sample opens")
        .objects
        .get(set)
        .expect("the spots exist");
    assert_eq!(restored.detected(), detected);
    assert_eq!(restored.tracks().is_some(), tracked);
}

#[test]
fn keeps_the_history_consistent_after_removing_the_measured_set() {
    let mut session = session();
    settle(&mut session);
    let scene = session.scene().expect("the sample opens");
    let set = scene
        .objects
        .iter()
        .find(|candidate| candidate.kind() == ObjectKind::Spots)
        .expect("the sample stages spots")
        .id();
    let other = scene
        .objects
        .iter()
        .find(|candidate| candidate.id() != set)
        .expect("another object set exists")
        .id();
    session.execute(Command::SetMeasureSet(Some(set)));
    assert!(session.measure().table().is_some());
    session.execute(Command::RemoveObject(set));
    session.execute(Command::SetObjectColor {
        set: other,
        color: [0.1, 0.1, 0.1],
    });
    session.execute(Command::Undo);
    assert!(
        session
            .scene()
            .expect("the sample opens")
            .objects
            .get(set)
            .is_none()
    );
    session.execute(Command::Undo);
    assert!(
        session
            .scene()
            .expect("the sample opens")
            .objects
            .get(set)
            .is_some()
    );
    assert!(session.measure().table().is_some());
}
