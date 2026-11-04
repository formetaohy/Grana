use std::time::{Duration, Instant};

use grana_object::{SurfaceParams, TrackSet};
use grana_scene::{ObjectFrame, ObjectKind, ObjectParams, ObjectSetId};
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

fn set_of(session: &Session, kind: ObjectKind) -> ObjectSetId {
    session
        .scene()
        .expect("the sample opens")
        .objects
        .iter()
        .find(|set| set.kind() == kind)
        .expect("the sample stages the object kind")
        .id()
}

fn find(session: &Session, name: impl Fn(&str) -> bool) -> Option<ObjectSetId> {
    session
        .scene()
        .expect("the sample opens")
        .objects
        .iter()
        .find(|set| name(set.name()))
        .map(|set| set.id())
}

fn sets(session: &Session) -> usize {
    session.scene().expect("the sample opens").objects.len()
}

fn total(session: &Session, set: ObjectSetId) -> usize {
    session
        .scene()
        .expect("the sample opens")
        .objects
        .get(set)
        .expect("the object set exists")
        .frames()
        .map(|(_, frame)| frame.object_count())
        .sum()
}

fn tracks(session: &Session, set: ObjectSetId) -> Option<TrackSet> {
    session
        .scene()
        .expect("the sample opens")
        .objects
        .get(set)
        .expect("the object set exists")
        .tracks()
        .map(|layer| layer.set().clone())
}

fn surfaces(session: &Session, set: ObjectSetId) -> SurfaceParams {
    match session
        .scene()
        .expect("the sample opens")
        .objects
        .get(set)
        .expect("the object set exists")
        .params()
    {
        ObjectParams::Surfaces(params) => params,
        _ => panic!("the set detects surfaces"),
    }
}

#[test]
fn merges_two_detections_and_keeps_the_watershed_of_both() {
    let mut session = opened();
    let set = set_of(&session, ObjectKind::Surfaces);
    let before = total(&session, set);
    assert!(before > 0, "the sample detects surfaces");
    session.execute(Command::DuplicateObject(set));
    let copy = find(&session, |name| name.ends_with("copy")).expect("the copied set exists");
    assert_eq!(total(&session, copy), before);

    session.execute(Command::MergeObjects {
        source: copy,
        target: set,
    });
    assert!(
        find(&session, |name| name.ends_with("copy")).is_none(),
        "the absorbed set leaves the scene"
    );
    assert_eq!(session.history().undo, Some(Action::MergeObjects));
    assert_eq!(total(&session, set), 2 * before);
    assert_eq!(
        tracks(&session, set).map(|tracks| tracks.node_count()),
        Some(before),
        "the absorbed copy carries no tracks"
    );
    {
        let scene = session.scene().expect("the sample opens");
        let object = scene.objects.get(set).expect("the merged set exists");
        for (_, frame) in object.frames() {
            let ObjectFrame::Surfaces(surfaces) = frame else {
                panic!("the merged set detects surfaces");
            };
            assert!(
                surfaces.topology().is_some(),
                "the merged frame carries the watershed of both halves"
            );
            assert_eq!(
                surfaces.leaves().len(),
                surfaces.object_count(),
                "every merged surface names the basins it holds"
            );
        }
    }

    session.execute(Command::Undo);
    assert_eq!(total(&session, set), before);
    assert!(
        find(&session, |name| name.ends_with("copy")).is_some(),
        "undo restores the absorbed set"
    );
    session.execute(Command::Redo);
    assert_eq!(total(&session, set), 2 * before);

    let params = surfaces(&session, set);
    let mut strict = params;
    strict.set_min_volume(1.0e9);
    session.execute(Command::SetObjectParams {
        set,
        params: ObjectParams::Surfaces(strict),
    });
    settle(&mut session);
    assert_eq!(total(&session, set), 0, "the strict filter keeps no object");
    session.execute(Command::SetObjectParams {
        set,
        params: ObjectParams::Surfaces(params),
    });
    settle(&mut session);
    assert_eq!(
        total(&session, set),
        2 * before,
        "the merged topology recuts both halves"
    );
}

#[test]
fn merges_the_tracks_of_the_absorbed_set() {
    let mut session = opened();
    let set = set_of(&session, ObjectKind::Spots);
    let before = total(&session, set);
    let source = tracks(&session, set).expect("the spots are tracked");
    assert_eq!(
        source.node_count(),
        before,
        "every detected spot belongs to a track"
    );

    session.execute(Command::DuplicateObject(set));
    let copy = find(&session, |name| name.ends_with("copy")).expect("the copied set exists");
    session.execute(Command::Track(copy));
    settle(&mut session);
    assert_eq!(
        tracks(&session, copy).map(|set| set.node_count()),
        Some(before),
        "the copy tracks its own objects"
    );

    session.execute(Command::MergeObjects {
        source: set,
        target: copy,
    });
    let merged = tracks(&session, copy).expect("the merged set carries both track layers");
    assert_eq!(merged.len(), 2 * source.len());
    assert_eq!(merged.node_count(), 2 * before);
    assert_eq!(
        total(&session, copy),
        2 * before,
        "every track node names one merged spot"
    );
}

#[test]
fn refuses_to_merge_other_kinds_and_itself() {
    let mut session = opened();
    let spots = set_of(&session, ObjectKind::Spots);
    let surfaces = set_of(&session, ObjectKind::Surfaces);
    let before = sets(&session);

    session.execute(Command::SelectChannel(0));
    session.execute(Command::AddObject(ObjectKind::Spots));
    let elsewhere = session
        .scene()
        .expect("the sample opens")
        .objects
        .iter()
        .filter(|set| set.kind() == ObjectKind::Spots && set.channel() == 0)
        .map(|set| set.id())
        .next()
        .expect("the added set detects the first channel");
    assert_eq!(sets(&session), before + 1);

    session.execute(Command::MergeObjects {
        source: elsewhere,
        target: spots,
    });
    assert_eq!(
        session.message().map(grana_session::Message::text),
        None,
        "a merge combines detection channels of one kind"
    );
    assert_eq!(sets(&session), before);
    let merged = session
        .scene()
        .expect("the sample opens")
        .objects
        .get(spots)
        .expect("the merged set exists");
    assert!(merged.provenance().is_merged());

    session.execute(Command::MergeObjects {
        source: surfaces,
        target: spots,
    });
    assert!(
        session
            .message()
            .is_some_and(|message| message.text().contains("kind")),
        "a merge refuses two object kinds"
    );
    assert_eq!(sets(&session), before);
    session.execute(Command::MergeObjects {
        source: spots,
        target: spots,
    });
    assert!(
        session
            .message()
            .is_some_and(|message| message.text().contains("another object set")),
        "a merge refuses itself"
    );
    assert_eq!(sets(&session), before);
}
