use grana_scene::{Dataset, ObjectKind, Scene};
use grana_session::{DetectionSpec, Protocol};
use grana_volume::{Channel, Extent, Spacing, Volume, VolumeSeries, VoxelData};

const PROTOCOL: &str = r#"
{
  "sets": [
    {
      "name": "Nuclei",
      "channel": "DAPI",
      "color": [1.0, 0.7, 0.2],
      "detection": {"kind": "surfaces", "threshold": 0.25, "min_volume": 2.0},
      "track": {
        "max_step": 0.8,
        "max_gap": 1,
        "division": true,
        "division_range": 2.0,
        "thickness": 0.2
      }
    },
    {
      "name": "Puncta",
      "channel": "GFP",
      "color": [0.3, 0.8, 1.0],
      "detection": {"kind": "spots", "diameter": 0.45, "contrast": 0.1}
    },
    {
      "name": "Vessels",
      "channel": "RFP",
      "color": [0.4, 0.9, 0.6],
      "detection": {"kind": "filaments", "threshold": 0.6, "min_length": 3.0}
    }
  ],
  "coloc": {"a": "GFP", "b": "RFP", "thresholds": [0.4, 0.6]}
}
"#;

#[test]
fn reads_and_writes_a_protocol() {
    let protocol = Protocol::from_json(PROTOCOL).expect("the protocol is valid");
    assert_eq!(protocol.sets().len(), 3);
    assert_eq!(protocol.sets()[0].kind(), ObjectKind::Surfaces);
    assert_eq!(protocol.sets()[1].kind(), ObjectKind::Spots);
    assert_eq!(protocol.sets()[2].kind(), ObjectKind::Filaments);
    let surface = protocol.sets()[0].params();
    assert_eq!(surface.value(0), 0.25);
    assert_eq!(surface.value(1), 2.0);
    let track = protocol.sets()[0]
        .track_params()
        .expect("the surface set is tracked");
    assert_eq!(track.max_step(), 0.8);
    assert_eq!(track.max_gap(), 1);
    assert!(track.division());
    assert_eq!(track.division_range(), 2.0);
    let coloc = protocol.coloc().expect("the protocol colocalizes");
    assert_eq!(coloc.a, "GFP");
    assert_eq!(coloc.b, "RFP");
    assert_eq!(coloc.thresholds, [0.4, 0.6]);
    assert!(matches!(
        protocol.sets()[1].detection,
        DetectionSpec::Spots { .. }
    ));

    let text = protocol.to_json().expect("the protocol serializes");
    let restored = Protocol::from_json(&text).expect("the protocol round trips");
    assert_eq!(protocol, restored);
}

#[test]
fn rejects_an_invalid_protocol() {
    let invalid = |text: &str| Protocol::from_json(text).is_err();
    assert!(invalid(r#"{"sets": []}"#), "an empty protocol is rejected");
    assert!(
        invalid(&PROTOCOL.replace("\"threshold\": 0.25", "\"threshold\": 1.25")),
        "a surface threshold above one is rejected"
    );
    assert!(
        invalid(&PROTOCOL.replace("\"min_length\": 3.0", "\"min_length\": 0.0")),
        "a filament length filter is positive"
    );
    assert!(
        invalid(&PROTOCOL.replace(
            "\"detection\": {\"kind\": \"spots\", \"diameter\": 0.45, \"contrast\": 0.1}",
            "\"detection\": {\"kind\": \"spots\", \"diameter\": 0.45, \"contrast\": 0.1}, \"extra\": 1"
        )),
        "an unknown field is rejected"
    );
    assert!(
        invalid(&PROTOCOL.replace("\"name\": \"Nuclei\"", "\"name\": \"puncta\"")),
        "two sets cannot write to one output name"
    );
    assert!(
        invalid(&PROTOCOL.replace(
            "\"detection\": {\"kind\": \"filaments\", \"threshold\": 0.6, \"min_length\": 3.0}",
            "\"detection\": {\"kind\": \"filaments\", \"threshold\": 0.6, \"min_length\": 3.0}, \"track\": {\"max_step\": 1.0, \"max_gap\": 1, \"division\": false, \"division_range\": 1.0, \"thickness\": 0.2}"
        )),
        "a filament set cannot be tracked"
    );
    assert!(
        invalid(&PROTOCOL.replace(
            "\"coloc\": {\"a\": \"GFP\", \"b\": \"RFP\"",
            "\"coloc\": {\"a\": \"GFP\", \"b\": \"GFP\""
        )),
        "a colocalization pair names two channels"
    );
}

#[test]
fn captures_the_analysis_of_a_scene() {
    let extent = Extent::new(4, 4, 4);
    let volume = Volume::new(
        extent,
        Spacing::new(1.0, 1.0, 1.0),
        vec![Channel::new(
            "DAPI",
            [0.3, 0.4, 1.0],
            VoxelData::U16(vec![0; extent.voxel_count() as usize]),
        )],
    );
    let movie = VolumeSeries::new(vec![volume], 1.0);
    let mut scene = Scene::new(Dataset::synthetic("fixture", movie.pyramid().clone()));
    let set = scene.add_object(ObjectKind::Spots, 0);
    if let Some(set) = scene.objects.get_mut(set) {
        set.set_color([0.2, 0.7, 0.9]);
    }
    let protocol = Protocol::capture(&scene).expect("the scene analysis is capturable");
    assert_eq!(protocol.sets().len(), 1);
    let spec = &protocol.sets()[0];
    assert_eq!(spec.name, "Spots 1");
    assert_eq!(spec.channel, "DAPI");
    assert_eq!(spec.kind(), ObjectKind::Spots);
    assert_eq!(spec.color, [0.2, 0.7, 0.9]);
    assert!(spec.track_params().is_some());
    assert!(protocol.coloc().is_none());
}
