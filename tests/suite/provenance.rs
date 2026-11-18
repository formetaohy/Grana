use grana_interop::Crate;
use grana_volume::Fingerprint;

use crate::support::Temp;
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
