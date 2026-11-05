use std::time::{Duration, Instant};

use grana_measure::{Filter, Group};
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

fn nodes(session: &Session, set: ObjectSetId) -> Option<usize> {
    tracks(session, set).map(|tracks| tracks.node_count())
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

fn gate(session: &mut Session, set: ObjectSetId) -> usize {
    session.execute(Command::SetMeasureSet(Some(set)));
    let (column, low, maximum) = {
        let table = session
            .measure()
            .table()
            .expect("the detected set measures every object");
        let column = table
            .column("volume_um3")
            .expect("surfaces measure their volume");
        let (low, high) = table
            .extent(column)
            .expect("the measured volumes are finite");
        (column, low, low + 0.5 * (high - low))
    };
    session.execute(Command::AddFilter);
    session.execute(Command::SetFilter {
        index: 0,
        filter: Filter::new(column, low, maximum),
    });
    let kept = session.measure().rows().len();
    let rows = session.measure().table().expect("the set measures").rows();
    let table = session.measure().table().expect("the set measures");
    assert_eq!(
        kept,
        (0..rows)
            .filter(|row| table.value(*row, column) <= maximum)
            .count(),
        "the gate keeps every object inside its range"
    );
    kept
}

#[test]
fn deletes_the_excluded_objects_and_undoes_them() {
    let mut session = opened();
    let set = set_of(&session, ObjectKind::Surfaces);
    let before = total(&session, set);
    assert_eq!(
        nodes(&session, set),
        Some(before),
        "every detected surface belongs to a track"
    );
    let kept = gate(&mut session, set);
    assert!(
        kept > 0 && kept < before,
        "the gate keeps a strict subset of the {before} surfaces"
    );

    session.execute(Command::DeleteFiltered);
    assert_eq!(total(&session, set), kept);
    assert_eq!(
        nodes(&session, set),
        Some(kept),
        "the deleted objects leave their tracks"
    );
    assert_eq!(session.history().undo, Some(Action::DeleteFiltered));

    session.execute(Command::Undo);
    assert_eq!(total(&session, set), before);
    assert_eq!(nodes(&session, set), Some(before));
    session.execute(Command::Redo);
    assert_eq!(total(&session, set), kept);
}

#[test]
fn recuts_do_not_revive_deleted_surfaces() {
    let mut session = opened();
    let set = set_of(&session, ObjectKind::Surfaces);
    let before = total(&session, set);
    let kept = gate(&mut session, set);
    session.execute(Command::DeleteFiltered);
    assert_eq!(total(&session, set), kept);
    let time = (0..session.scene().expect("the sample opens").frame_count())
        .find(|time| {
            session
                .scene()
                .expect("the sample opens")
                .objects
                .get(set)
                .expect("the object set exists")
                .frame(*time)
                .is_some_and(|frame| frame.object_count() > 0)
        })
        .expect("a frame keeps an object");
    session.execute(Command::SetObjectHidden {
        set,
        time,
        objects: vec![0],
        hidden: true,
    });

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
        kept,
        "the relaxed recut keeps the deleted objects out"
    );
    assert!(kept < before);
    assert_eq!(surfaces(&session, set), params);
    assert_eq!(
        session
            .scene()
            .expect("the sample opens")
            .objects
            .get(set)
            .expect("the object set exists")
            .hidden_count(),
        1,
        "the recut keeps the object the user hid"
    );
}

#[test]
fn deletes_the_objects_of_excluded_tracks() {
    let mut session = opened();
    let set = set_of(&session, ObjectKind::Spots);
    let before = total(&session, set);
    session.execute(Command::SetMeasureSet(Some(set)));
    session.execute(Command::SetMeasureGroup(Group::Track));
    let (column, low, maximum, kept_tracks, kept_nodes) = {
        let table = session
            .measure()
            .table()
            .expect("the tracked spots measure every track");
        let column = table
            .column("length_um")
            .expect("tracks measure their length");
        let (low, high) = table
            .extent(column)
            .expect("the measured track lengths are finite");
        let maximum = low + 0.5 * (high - low);
        let tracks = tracks(&session, set).expect("the detected spots are tracked");
        let mut kept_tracks = 0;
        let mut kept_nodes = 0;
        for row in 0..table.rows() {
            let value = table.value(row, column);
            if value < low || value > maximum {
                continue;
            }
            let track = table
                .observation(row)
                .track()
                .expect("a track row names its track");
            kept_nodes += tracks.track(track).expect("the track exists").nodes().len();
            kept_tracks += 1;
        }
        (column, low, maximum, kept_tracks, kept_nodes)
    };
    assert!(
        kept_tracks > 0 && kept_tracks < session.measure().rows().len(),
        "the gate keeps a strict subset of the tracks"
    );
    session.execute(Command::AddFilter);
    session.execute(Command::SetFilter {
        index: 0,
        filter: Filter::new(column, low, maximum),
    });
    assert_eq!(
        session.measure().rows().len(),
        kept_tracks,
        "the gate keeps every track inside its range"
    );

    session.execute(Command::DeleteFiltered);
    assert_eq!(
        total(&session, set),
        kept_nodes,
        "only the objects of the kept tracks remain"
    );
    session.execute(Command::Undo);
    assert_eq!(total(&session, set), before);
}

#[test]
fn hides_objects_through_a_redetection() {
    let mut session = opened();
    let set = set_of(&session, ObjectKind::Surfaces);
    let (time, count) = {
        let scene = session.scene().expect("the sample opens");
        let object = scene.objects.get(set).expect("the set exists");
        (0..scene.frame_count())
            .find(|time| {
                object
                    .frame(*time)
                    .is_some_and(|frame| frame.object_count() > 1)
            })
            .map(|time| {
                (
                    time,
                    object.frame(time).map_or(0, ObjectFrame::object_count),
                )
            })
            .expect("a frame holds two surfaces")
    };
    session.execute(Command::SetObjectHidden {
        set,
        time,
        objects: vec![0],
        hidden: true,
    });
    let hidden_position = {
        let scene = session.scene().expect("the sample opens");
        let object = scene.objects.get(set).expect("the set exists");
        assert_eq!(object.hidden_count(), 1);
        assert!(object.hidden_object(time, 0));
        object
            .frame(time)
            .expect("the frame exists")
            .anchors()
            .first()
            .expect("the hidden object has an anchor")
            .position()
    };
    assert_eq!(
        session
            .scene()
            .expect("the sample opens")
            .objects
            .get(set)
            .expect("the set exists")
            .curation()
            .hidden_count(),
        1,
        "hiding an object records it for replay"
    );

    session.execute(Command::Detect(set));
    settle(&mut session);
    let object = session
        .scene()
        .expect("the sample opens")
        .objects
        .get(set)
        .expect("the set exists");
    assert_eq!(object.hidden_count(), 1, "the hidden object stays hidden");
    let n = object.frame(time).map_or(0, ObjectFrame::object_count);
    let hidden = (0..n)
        .find(|object| {
            session
                .scene()
                .expect("the sample opens")
                .objects
                .get(set)
                .expect("the set exists")
                .hidden_object(time, *object)
        })
        .expect("the hidden object matches the redetected frame");
    assert!(
        n >= count,
        "the redetection keeps at least the objects of the frame"
    );
    let position = object.frame(time).expect("the frame exists").anchors()[hidden].position();
    assert!(
        (0..3).all(|axis| (position[axis] - hidden_position[axis]).abs() < 2.0),
        "the hidden object stands where the user hid it"
    );
}

#[test]
fn extracts_the_gated_objects_and_copies_a_set() {
    let mut session = opened();
    let source = set_of(&session, ObjectKind::Surfaces);
    let before = total(&session, source);
    let kept = gate(&mut session, source);
    session.execute(Command::ExtractFiltered);
    let derived = find(&session, |name| name.ends_with("filtered"))
        .expect("the gated objects become their own set");
    assert_eq!(total(&session, derived), kept);
    assert_eq!(
        total(&session, source),
        before,
        "the source keeps every object it detected"
    );
    let order = session
        .scene()
        .expect("the sample opens")
        .objects
        .iter()
        .map(|set| set.id())
        .collect::<Vec<_>>();
    let position = order
        .iter()
        .position(|id| *id == source)
        .expect("the source is ordered");
    assert_eq!(
        order.get(position + 1),
        Some(&derived),
        "the derived set follows its source"
    );
    {
        let scene = session.scene().expect("the sample opens");
        let Some(ObjectFrame::Surfaces(derived_zero)) =
            scene.objects.get(derived).and_then(|set| set.frame(0))
        else {
            panic!("the derived set covers the first frame");
        };
        assert!(
            derived_zero.topology().is_some(),
            "the derived surfaces keep their watershed topology"
        );
        assert_eq!(
            derived_zero.leaves().len(),
            derived_zero.object_count(),
            "every derived surface names the basins it holds"
        );
        assert!(
            derived_zero
                .leaves()
                .iter()
                .all(|leaves| !leaves.is_empty()),
            "every derived surface holds at least one basin"
        );
    }

    session.execute(Command::Undo);
    assert!(
        find(&session, |name| name.ends_with("filtered")).is_none(),
        "the derived set is undone"
    );
    session.execute(Command::Redo);
    assert!(
        find(&session, |name| name.ends_with("filtered")).is_some(),
        "the derived set returns"
    );

    session.execute(Command::DuplicateObject(source));
    let copy = find(&session, |name| name.ends_with("copy")).expect("the copied set exists");
    assert_eq!(total(&session, copy), before);
    assert!(
        tracks(&session, copy).is_none(),
        "a copied set carries no tracks"
    );
    assert_eq!(surfaces(&session, copy), surfaces(&session, source));

    let provenance = session
        .scene()
        .expect("the sample opens")
        .objects
        .get(derived)
        .expect("the derived set exists")
        .provenance()
        .clone();
    assert!(matches!(
        provenance,
        grana_scene::Provenance::Filtered { .. }
    ));
    assert!(
        provenance.depends_on(source),
        "the derived set records the object set it filters"
    );
    let params = surfaces(&session, source);
    let mut strict = params;
    strict.set_min_volume(1.0e9);
    session.execute(Command::SetObjectParams {
        set: source,
        params: ObjectParams::Surfaces(strict),
    });
    settle(&mut session);
    assert_eq!(
        total(&session, derived),
        0,
        "the derived set follows the source cut"
    );
    session.execute(Command::SetObjectParams {
        set: source,
        params: ObjectParams::Surfaces(params),
    });
    settle(&mut session);
    assert_eq!(
        total(&session, derived),
        kept,
        "the derived set re-derives the kept objects"
    );

    let frame = {
        let scene = session.scene().expect("the sample opens");
        let set = scene.objects.get(derived).expect("the derived set exists");
        (0..scene.frame_count())
            .find(|time| {
                set.frame(*time)
                    .is_some_and(|frame| frame.object_count() > 0)
            })
            .expect("the derived set holds an object")
    };
    session.execute(Command::DeleteObject {
        set: derived,
        time: frame,
        object: 0,
    });
    assert_eq!(total(&session, derived), kept - 1);
    session.execute(Command::Detect(source));
    settle(&mut session);
    assert_eq!(
        total(&session, derived),
        kept - 1,
        "a re-detected source keeps the objects the user removed from its filtered set"
    );
    assert_eq!(
        session
            .scene()
            .expect("the sample opens")
            .objects
            .get(derived)
            .expect("the derived set exists")
            .curation()
            .removal_count(),
        1
    );
}
