use grana_interop::Crate;
use grana_object::{Algorithm, IntensityTable, SampleStatistics, Spot, SpotSet};
use grana_protocol::Protocol;
use grana_scene::{Dataset, Measure, ObjectFrame, ObjectKind, Scene};
use grana_volume::{Extent, Fingerprint, Spacing};

use crate::support::{Temp, spheres};

fn scene() -> Scene {
    let volume = spheres(
        Extent::new(16, 16, 4),
        Spacing::new(1.0, 1.0, 1.0),
        &[([8.0, 8.0, 2.0], 3.0)],
        1_000,
    );
    let mut scene = Scene::new(Dataset::synthetic("sample", volume.pyramid()));
    let set = scene.add_object(ObjectKind::Spots, 0);
    scene.install(
        set,
        0,
        ObjectFrame::Spots(SpotSet::new(
            0,
            vec![Spot::new([8.0, 8.0, 2.0], 1.0, 0.5)],
            IntensityTable::new(
                1,
                vec![SampleStatistics::new(
                    1_000.0, 1_000.0, 1_000.0, 0.0, 1_000.0,
                )],
            ),
        )),
    );
    scene
}

#[test]
fn records_the_origin_of_a_captured_protocol() {
    let scene = scene();
    let fingerprint = Fingerprint::new([7u8; 32]);
    let hex = fingerprint.hex();
    let protocol = Protocol::capture(&scene, &Measure::new(), None, Vec::new(), Some(fingerprint))
        .expect("the scene captures");
    let origin = protocol
        .origin()
        .expect("a captured protocol records its origin");
    assert_eq!(origin.fingerprint(), Some(hex.as_str()));
    assert!(
        origin
            .algorithms()
            .iter()
            .any(|spec| spec.name == Algorithm::Detection.name() && spec.version == 1)
    );
    assert_eq!(origin.sets().len(), 1);
    assert_eq!(origin.sets()[0].algorithm, Algorithm::Detection.name());
    assert_eq!(origin.sets()[0].version, 1);
    assert_eq!(origin.sets()[0].objects, 1);
    assert!(origin.verify(&scene, Some(fingerprint)).is_empty());

    let other = Fingerprint::new([8u8; 32]);
    let drift = origin.verify(&scene, Some(other));
    assert!(
        drift.iter().any(|line| line.contains("another dataset")),
        "the origin notices another dataset: {drift:?}"
    );

    let text = protocol.to_json().expect("the protocol serializes");
    let restored = Protocol::from_json(&text).expect("the protocol parses");
    assert_eq!(restored.origin(), Some(origin));
}

#[test]
fn writes_an_ro_crate_metadata_file() {
    let temp = Temp::new("grana-crate");
    let mut record = Crate::new("sample", "The analysis of a sample", "Grana", "0.1.1");
    record
        .fingerprint(&Fingerprint::new([7u8; 32]).hex(), "a fixture")
        .part(
            "measurements/spots-objects.csv",
            "spots-objects.csv",
            "text/csv",
            "the objects of Spots",
        )
        .action(
            "#create-spots",
            "Detection of Spots",
            "Detection v1 over 1 objects",
            &["measurements/spots-objects.csv".to_string()],
        );
    let path = record.write(temp.path()).expect("the crate writes");
    let text = std::fs::read_to_string(&path).expect("the metadata reads");
    let json = serde_json::from_str::<serde_json::Value>(&text).expect("the metadata is json");
    assert_eq!(
        json["@context"],
        serde_json::json!("https://w3id.org/ro/crate/1.2/context")
    );
    let graph = json["@graph"].as_array().expect("the graph lists entities");
    assert!(graph.iter().any(|entity| {
        entity["@id"] == "ro-crate-metadata.json"
            && entity["conformsTo"]["@id"] == "https://w3id.org/ro/crate/1.2"
    }));
    let root = graph
        .iter()
        .find(|entity| entity["@id"] == "./")
        .expect("the root dataset exists");
    assert!(
        root["hasPart"]
            .as_array()
            .expect("the root lists its parts")
            .iter()
            .any(|part| part["@id"] == "measurements/spots-objects.csv")
    );
    let action = graph
        .iter()
        .find(|entity| entity["@id"] == "#create-spots")
        .expect("the create action exists");
    assert_eq!(action["instrument"]["@id"], "#software");
    assert!(
        action["result"]
            .as_array()
            .expect("the action lists its results")
            .iter()
            .any(|result| result["@id"] == "measurements/spots-objects.csv")
    );
    assert!(path.ends_with("ro-crate-metadata.json"));
}
