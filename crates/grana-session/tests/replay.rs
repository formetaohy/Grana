use std::time::{Duration, Instant};

use grana_measure::Measurement;
use grana_protocol::Protocol;
use grana_scene::ObjectColor;
use grana_session::{Command, Session};
use grana_volume::{Channel, Extent, Spacing, Volume, VolumeSeries, VoxelData};

const EXTENT: Extent = Extent::new(32, 32, 8);
const SIGNAL: u16 = 60_000;

const PROTOCOL: &str = r#"
{
  "channels": [
    {"name": "Cells", "enabled": true, "opacity": 0.9, "window": {"low": 0.0, "high": 65535.0}},
    {"name": "Signal", "enabled": true, "opacity": 0.6, "window": {"low": 0.0, "high": 65535.0}}
  ],
  "sets": [
    {
      "name": "Nuclei",
      "color": [1.0, 0.7, 0.2],
      "opacity": 0.5,
      "source": {"kind": "surfaces", "channel": "Cells", "threshold": 0.5, "grain": 0.0, "split": 0.0, "min_volume": 0.0, "opening": 0.0, "closing": 0.0, "min_sphericity": 0.0, "min_mean_intensity": 0.0}
    },
    {
      "name": "Beads",
      "color": [0.3, 0.8, 1.0],
      "color_by": {"key": "diameter_um", "palette": "inferno"},
      "source": {"kind": "spots", "channel": "Cells", "diameter": 5.0, "contrast": 0.05},
      "mask": "Nuclei",
      "measure": {"group": "objects", "aggregate": "mean", "sort": {"column": "diameter_um", "descending": true}}
    }
  ],
  "relations": [{"source": "Beads", "target": "Nuclei"}],
  "coloc": {"settings": {"permutations": 8, "block": 3}, "pairs": [{"a": "Cells", "b": "Signal", "thresholds": [0.5, 0.5]}], "mask": "Nuclei"},
  "view": {"camera": {"pivot": [0.0, 0.0, 0.0], "yaw": 0.5, "pitch": 0.3, "distance": 90.0, "fov_y": 0.61}, "section": {"axis": "z", "span": 8.0}, "split": 0.5, "time": 1, "timeline": {"fps": 24, "duration": 4.0}}
}
"#;

fn series() -> VolumeSeries {
    let count = EXTENT.voxel_count() as usize;
    let frames = (0..2)
        .map(|time: u32| {
            let mut values = vec![0u16; count];
            for z in 0..EXTENT.z() {
                for y in 0..EXTENT.y() {
                    for x in 0..EXTENT.x() {
                        let centre = [16.0 + time as f32, 16.0, 4.0];
                        let distance = ((x as f32 - centre[0]).powi(2)
                            + (y as f32 - centre[1]).powi(2)
                            + (z as f32 - centre[2]).powi(2))
                        .sqrt();
                        if distance < 4.0 {
                            values[EXTENT.index(x, y, z) as usize] = SIGNAL;
                        }
                    }
                }
            }
            Volume::new(
                EXTENT,
                Spacing::new(0.25, 0.25, 0.5),
                vec![
                    Channel::new("Cells", [0.3, 0.8, 1.0], VoxelData::U16(values.clone())),
                    Channel::new("Signal", [1.0, 0.4, 0.3], VoxelData::U16(values)),
                ],
            )
        })
        .collect::<Vec<_>>();
    VolumeSeries::new(frames, 1.0)
}

#[test]
fn replays_a_protocol_on_the_open_dataset() {
    let mut session = Session::new();
    session.open_series("fixture", series());
    let protocol = Protocol::from_json(PROTOCOL).expect("the protocol is valid");
    let view = protocol
        .view()
        .expect("the protocol carries a view")
        .clone();
    session.set_protocol(protocol);
    session.execute(Command::ApplyProtocol);
    assert!(session.replaying(), "the protocol replays its analysis");
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        session.poll();
        if !session.replaying() && session.progress().is_empty() {
            break;
        }
        assert!(Instant::now() < deadline, "the replay did not finish");
        std::thread::sleep(Duration::from_millis(2));
    }
    while session.poll() {}

    let beads_id = {
        let scene = session.scene().expect("the dataset stays open");
        assert_eq!(scene.time(), 1, "the protocol moves to its frame");
        let names = scene
            .objects
            .iter()
            .map(|set| set.name().to_string())
            .collect::<Vec<_>>();
        assert_eq!(names, ["Nuclei", "Beads"]);
        assert_eq!(scene.channels[0].opacity, 0.9);
        let mut sets = scene.objects.iter();
        let nuclei = sets.next().expect("the surface set");
        let beads = sets.next().expect("the spot set");
        assert_eq!(nuclei.detected(), 2, "the surfaces cover every frame");
        assert_eq!(beads.detected(), 2, "the spots cover every frame");
        assert_eq!(nuclei.opacity(), 0.5);
        assert_eq!(beads.color(), [0.3, 0.8, 1.0]);
        assert_eq!(
            beads.object_coloring().metric(),
            Some(ObjectColor::Measurement(Measurement::Diameter))
        );
        let relation = beads
            .relation(nuclei.id())
            .expect("the relation is measured");
        assert_eq!(relation.computed(), 2, "the relation covers every frame");
        assert_eq!(scene.mask, Some(nuclei.id()), "the mask settles on its set");
        let coloc = scene
            .coloc
            .result()
            .expect("the colocalization is measured");
        assert_eq!(coloc.pair(), grana_measure::ColocPair::new(0, 1));
        assert_eq!(
            coloc.thresholds(),
            grana_measure::ColocThresholds::new(0.5, 0.5)
        );
        beads.id()
    };
    assert_eq!(
        session.measure().set(),
        Some(beads_id),
        "the statistics panel shows the measured set"
    );
    let sort = session
        .measure()
        .sort()
        .expect("the protocol orders the statistics");
    let table = session
        .measure()
        .table()
        .expect("the measured set carries a table");
    assert_eq!(table.columns()[sort.0].key(), "diameter_um");
    assert!(sort.1, "the statistics descend");
    let applied = session.take_view().expect("the protocol applies its view");
    assert_eq!(applied.camera, view.camera());
    assert_eq!(applied.section, view.section());
    assert_eq!(applied.split, 0.5);
    assert_eq!(applied.timeline.fps(), 24);
    assert_eq!(applied.timeline.duration(), 4.0);
    assert!(
        session.history().undo.is_some(),
        "the replay is a single undoable edit"
    );
    assert!(session.execute(Command::Undo).repaint);
    let scene = session.scene().expect("the dataset stays open");
    assert_eq!(scene.objects.len(), 0, "undo returns to the empty dataset");
}

#[test]
fn replays_the_manual_removals_of_a_captured_protocol() {
    let mut session = Session::new();
    session.open_series("fixture", series());
    session.set_protocol(Protocol::from_json(PROTOCOL).expect("the protocol is valid"));
    session.execute(Command::ApplyProtocol);
    settle(&mut session);
    let nuclei = named(&session, "Nuclei");
    assert_eq!(count(&session, nuclei, 0), 1);
    session.execute(Command::SetTime(0));
    session.execute(Command::Select(Some(grana_scene::Selection {
        set: nuclei,
        item: grana_scene::Item::Object(0),
    })));
    assert!(session.delete_selected(), "the object leaves its frame");
    assert_eq!(count(&session, nuclei, 0), 0);

    session.execute(Command::CaptureProtocol);
    let captured = session
        .protocol()
        .expect("the protocol is captured")
        .clone();
    assert!(
        captured
            .to_json()
            .expect("the protocol serializes")
            .contains("\"expected\""),
        "the capture records the object counts of the manual edits"
    );
    session.set_protocol(captured);

    session.execute(Command::ApplyProtocol);
    settle(&mut session);
    let replayed = named(&session, "Nuclei");
    assert_eq!(
        count(&session, replayed, 0),
        0,
        "the replay removes the object the user deleted"
    );
    assert_eq!(count(&session, replayed, 1), 1);
    assert_eq!(
        session
            .scene()
            .expect("the dataset stays open")
            .objects
            .get(replayed)
            .expect("the replayed set exists")
            .curation()
            .removal_count(),
        1
    );
}

fn settle(session: &mut Session) {
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        session.poll();
        if !session.replaying() && session.progress().is_empty() {
            break;
        }
        assert!(Instant::now() < deadline, "the replay did not finish");
        std::thread::sleep(Duration::from_millis(2));
    }
    while session.poll() {}
}

fn named(session: &Session, name: &str) -> grana_scene::ObjectSetId {
    session
        .scene()
        .expect("the dataset stays open")
        .objects
        .iter()
        .find(|set| set.name() == name)
        .unwrap_or_else(|| panic!("the replay stages {name}"))
        .id()
}

fn count(session: &Session, set: grana_scene::ObjectSetId, time: u32) -> usize {
    session
        .scene()
        .expect("the dataset stays open")
        .objects
        .get(set)
        .and_then(|object| object.frame(time))
        .map_or(0, grana_scene::ObjectFrame::object_count)
}

#[test]
fn refuses_a_protocol_the_dataset_cannot_replay() {
    let mut session = Session::new();
    session.open_series("fixture", series());
    let protocol = Protocol::from_json(&PROTOCOL.replace("\"Cells\"", "\"Missing\""))
        .expect("the protocol is valid");
    session.set_protocol(protocol);
    session.execute(Command::ApplyProtocol);
    assert!(!session.replaying());
    let message = session.message().expect("the replay reports its refusal");
    assert!(message.text().contains("Missing"), "{}", message.text());
    let scene = session.scene().expect("the dataset stays open");
    assert_eq!(scene.objects.len(), 0);
}
