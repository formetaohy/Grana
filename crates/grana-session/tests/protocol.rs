use glam::Vec3;
use grana_measure::{
    ColocCost, ColocFrame, ColocPair, ColocSeries, ColocSeriesInfo, ColocSignificance,
    ColocThresholds,
};
use grana_scene::{Dataset, Measure, ObjectKind, Scene};
use grana_session::{DetectionSpec, Protocol};
use grana_volume::{Channel, Extent, Region, Spacing, Volume, VolumeSeries, VoxelData};

const PROTOCOL: &str = r#"
{
  "region": {"low": [0.1, 0.2, 0.0], "high": [0.9, 0.8, 1.0]},
  "sets": [
    {
      "name": "Nuclei",
      "channel": "DAPI",
      "color": [1.0, 0.7, 0.2],
      "detection": {"kind": "surfaces", "threshold": 0.25, "grain": 0.1, "seed": 0.6, "min_volume": 2.0, "opening": 0.2, "closing": 0.3, "min_sphericity": 0.4, "min_mean_intensity": 0.05},
      "track": {
        "max_step": 0.8,
        "max_gap": 1,
        "division": true,
        "division_range": 2.0,
        "thickness": 0.2
      },
      "measure": {
        "group": "tracks",
        "aggregate": "max",
        "filters": [{"column": "volume_um3", "minimum": 1.0, "maximum": 9.0}]
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
  "coloc": {"settings": {"permutations": 200, "block": 3}, "pairs": [{"a": "GFP", "b": "RFP", "thresholds": [0.4, 0.6]}, {"a": "GFP", "b": "DAPI", "thresholds": [0.5, 0.5]}], "mask": null}
}
"#;

#[test]
fn reads_and_writes_a_protocol() {
    let protocol = Protocol::from_json(PROTOCOL).expect("the protocol is valid");
    assert_eq!(protocol.sets().len(), 3);
    assert_eq!(protocol.sets()[0].kind(), ObjectKind::Surfaces);
    assert_eq!(protocol.sets()[1].kind(), ObjectKind::Spots);
    assert_eq!(protocol.sets()[2].kind(), ObjectKind::Filaments);
    let region = protocol
        .region()
        .expect("the protocol restricts its region");
    assert_eq!(region.low, [0.1, 0.2, 0.0]);
    assert_eq!(region.high, [0.9, 0.8, 1.0]);
    let surface = protocol.sets()[0].params();
    assert_eq!(surface.value(0), 0.25);
    assert_eq!(surface.value(1), 0.1);
    assert_eq!(surface.value(2), 0.6);
    assert_eq!(surface.value(3), 2.0);
    assert_eq!(surface.value(4), 0.2);
    assert_eq!(surface.value(5), 0.3);
    assert_eq!(surface.value(6), 0.4);
    assert_eq!(surface.value(7), 0.05);
    let track = protocol.sets()[0]
        .track_params()
        .expect("the surface set is tracked");
    assert_eq!(track.max_step(), 0.8);
    assert_eq!(track.max_gap(), 1);
    assert!(track.division());
    assert_eq!(track.division_range(), 2.0);
    let measure = &protocol.sets()[0].measure;
    assert_eq!(measure.group(), grana_measure::Group::Track);
    assert_eq!(measure.aggregate(), grana_measure::Aggregate::Maximum);
    assert_eq!(measure.filters.len(), 1);
    assert_eq!(measure.filters[0].column, "volume_um3");
    assert_eq!(measure.filters[0].minimum, 1.0);
    assert_eq!(measure.filters[0].maximum, 9.0);
    assert_eq!(
        protocol.sets()[1].measure.group(),
        grana_measure::Group::Object
    );
    let coloc = protocol.coloc().expect("the protocol colocalizes");
    assert_eq!(coloc.settings.permutations, 200);
    assert_eq!(coloc.settings.block, 3);
    assert_eq!(coloc.pairs.len(), 2);
    assert_eq!(coloc.pairs[0].a, "GFP");
    assert_eq!(coloc.pairs[0].b, "RFP");
    assert_eq!(coloc.pairs[0].thresholds, [0.4, 0.6]);
    assert_eq!(coloc.pairs[1].a, "GFP");
    assert_eq!(coloc.pairs[1].b, "DAPI");
    assert_eq!(coloc.mask, None);
    assert_eq!(coloc.settings().permutations(), 200);
    assert!(matches!(
        protocol.sets()[1].detection,
        DetectionSpec::Spots { .. }
    ));

    let text = protocol.to_json().expect("the protocol serializes");
    let restored = Protocol::from_json(&text).expect("the protocol round trips");
    assert_eq!(protocol, restored);
    assert_eq!(protocol.digest(), restored.digest());
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
            "\"pairs\": [{\"a\": \"GFP\", \"b\": \"RFP\", \"thresholds\": [0.4, 0.6]}",
            "\"pairs\": [{\"a\": \"GFP\", \"b\": \"GFP\", \"thresholds\": [0.4, 0.6]}"
        )),
        "a colocalization pair names two channels"
    );
    assert!(
        invalid(&PROTOCOL.replace(
            "\"a\": \"GFP\", \"b\": \"DAPI\", \"thresholds\": [0.5, 0.5]",
            "\"a\": \"GFP\", \"b\": \"RFP\", \"thresholds\": [0.5, 0.5]"
        )),
        "a colocalization pair appears once"
    );
    assert!(
        invalid(&PROTOCOL.replace("\"thresholds\": [0.4, 0.6]", "\"thresholds\": [0.4, 1.6]")),
        "a colocalization threshold is a unit fraction"
    );
    assert!(
        invalid(&PROTOCOL.replace("\"permutations\": 200", "\"permutations\": 0")),
        "a colocalization significance test runs permutations"
    );
    assert!(
        invalid(&PROTOCOL.replace("\"block\": 3", "\"block\": 0")),
        "a colocalization randomization block is positive"
    );
    assert!(
        invalid(&PROTOCOL.replace("\"mask\": null", "\"mask\": \"Missing\"")),
        "a colocalization mask names an object set of the protocol"
    );
    assert!(
        invalid(&PROTOCOL.replace(
            "\"pairs\": [{\"a\": \"GFP\", \"b\": \"RFP\", \"thresholds\": [0.4, 0.6]}, {\"a\": \"GFP\", \"b\": \"DAPI\", \"thresholds\": [0.5, 0.5]}]",
            "\"pairs\": []"
        )),
        "a colocalization specification names a pair"
    );
    assert!(
        invalid(&PROTOCOL.replace(", \"mask\": null", "")),
        "a colocalization specification carries a mask field"
    );
    assert!(
        invalid(&PROTOCOL.replace("\"group\": \"tracks\"", "\"group\": \"steps\"")),
        "an unknown grouping is rejected"
    );
    assert!(
        invalid(&PROTOCOL.replace("\"minimum\": 1.0", "\"minimum\": 10.0")),
        "a filter range is ordered"
    );
    assert!(
        invalid(&PROTOCOL.replace("[0.1, 0.2, 0.0]", "[0.1, 0.2, -0.1]")),
        "a region box lies inside the dataset"
    );
    assert!(
        invalid(&PROTOCOL.replace(
            "\"color\": [1.0, 0.7, 0.2],",
            "\"color\": [1.0, 0.7, 0.2], \"mask\": \"Puncta\","
        )),
        "an analysis mask names a set that runs before it"
    );
    assert!(
        invalid(&PROTOCOL.replace(
            "\"color\": [1.0, 0.7, 0.2],",
            "\"color\": [1.0, 0.7, 0.2], \"mask\": \"Nuclei\","
        )),
        "a set cannot be masked by itself"
    );
    assert!(
        invalid(&PROTOCOL.replace(
            "\"name\": \"Puncta\",\n      \"channel\": \"GFP\",\n      \"color\": [0.3, 0.8, 1.0],\n      \"detection\": {\"kind\": \"spots\", \"diameter\": 0.45, \"contrast\": 0.1}",
            "\"name\": \"Puncta\",\n      \"channel\": \"GFP\",\n      \"color\": [0.3, 0.8, 1.0],\n      \"detection\": {\"kind\": \"spots\", \"diameter\": 0.45, \"contrast\": 0.1},\n      \"measure\": {\"group\": \"tracks\", \"aggregate\": \"mean\"}"
        )),
        "an untracked set does not group by tracks"
    );
}

#[test]
fn captures_the_analysis_of_a_scene() {
    let extent = Extent::new(4, 4, 4);
    let volume = Volume::new(
        extent,
        Spacing::new(1.0, 1.0, 1.0),
        vec![
            Channel::new(
                "DAPI",
                [0.3, 0.4, 1.0],
                VoxelData::U16(vec![0; extent.voxel_count() as usize]),
            ),
            Channel::new(
                "GFP",
                [0.3, 1.0, 0.4],
                VoxelData::U16(vec![0; extent.voxel_count() as usize]),
            ),
        ],
    );
    let movie = VolumeSeries::new(vec![volume], 1.0);
    let mut scene = Scene::new(Dataset::synthetic("fixture", movie.pyramid().clone()));
    scene.region = Region::new(Vec3::new(-1.0, -1.0, -1.0), Vec3::new(1.0, 1.0, 1.0));
    let set = scene.add_object(ObjectKind::Spots, 0);
    if let Some(set) = scene.objects.get_mut(set) {
        set.set_color([0.2, 0.7, 0.9]);
    }
    let mut frame = ColocFrame::new(32);
    frame.push(0.25, 0.75);
    frame.push(0.5, 0.5);
    let mut joint = vec![0; 32 * 32];
    joint[4 + 32 * 9] = 2;
    scene.coloc.install(ColocSeries::from_parts(
        ColocPair::new(0, 1),
        ColocSeriesInfo::new(0, 0, 1, None),
        32,
        vec![frame],
        joint,
        Some(ColocSignificance::new(0.004_975, 200, 1.0)),
        ColocCost::new(2, 32, 1, 200, 4),
    ));
    scene.coloc.set_thresholds(ColocThresholds::new(0.2, 0.8));
    let measure = Measure::new();
    let protocol = Protocol::capture(&scene, &measure).expect("the scene analysis is capturable");
    assert_eq!(protocol.sets().len(), 1);
    let spec = &protocol.sets()[0];
    assert_eq!(spec.name, "Spots 1");
    assert_eq!(spec.channel, "DAPI");
    assert_eq!(spec.kind(), ObjectKind::Spots);
    assert_eq!(spec.color, [0.2, 0.7, 0.9]);
    assert!(spec.track_params().is_some());
    assert_eq!(spec.measure.group(), grana_measure::Group::Object);
    let coloc = protocol.coloc().expect("the scene colocalization captures");
    assert_eq!(coloc.pairs.len(), 1);
    assert_eq!(coloc.pairs[0].a, "DAPI");
    assert_eq!(coloc.pairs[0].b, "GFP");
    assert_eq!(coloc.pairs[0].thresholds, [0.2, 0.8]);
    assert_eq!(coloc.settings.permutations, 200);
    assert_eq!(coloc.settings.block, 3);
    assert_eq!(coloc.mask, None);
    let region = protocol.region().expect("the scene region is captured");
    assert_eq!(region.low, [0.25, 0.25, 0.25]);
    assert_eq!(region.high, [0.75, 0.75, 0.75]);
}

#[test]
fn captures_an_analysis_mask() {
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
    let series = VolumeSeries::new(vec![volume], 1.0);
    let mut scene = Scene::new(Dataset::synthetic("fixture", series.pyramid().clone()));
    let first = scene.add_object(ObjectKind::Surfaces, 0);
    scene.add_object(ObjectKind::Spots, 0);
    scene.set_mask(Some(first));
    let protocol =
        Protocol::capture(&scene, &Measure::new()).expect("the masked analysis captures");
    assert_eq!(protocol.sets().len(), 2);
    assert_eq!(protocol.sets()[0].name, "Surfaces 1");
    assert_eq!(protocol.sets()[0].mask(), None);
    assert_eq!(protocol.sets()[1].name, "Spots 1");
    assert_eq!(protocol.sets()[1].mask(), Some("Surfaces 1"));
    let text = protocol.to_json().expect("the protocol serializes");
    let restored = Protocol::from_json(&text).expect("the protocol round trips");
    assert_eq!(protocol, restored);
}
