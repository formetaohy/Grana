use glam::Vec3;
use grana_animation::{Keyframe, Timeline};
use grana_display::{
    CameraState, Coloring, Palette, SectionState, Shading, Stop, TransferFunction,
};
use grana_measure::{
    Aggregate, ColocCost, ColocFrame, ColocPair, ColocSeries, ColocSeriesInfo, ColocSignificance,
    ColocThresholds, Group, Lasso, MeasureView, Measurement, RelationField, Scope,
    TrackMeasurement,
};
use grana_object::{IntensityTable, SampleAccumulator, Spot, SpotSet};
use grana_project::{Export, ViewState};
use grana_protocol::{Protocol, SnapshotSpec, SourceSpec, Step, ViewSpec};
use grana_scene::{
    Criteria, Dataset, FilterRange, Measure, ObjectColor, ObjectFrame, ObjectKind, Scene,
    TrackColor,
};
use grana_volume::{Axis, Channel, Clip, Extent, Region, Spacing, Volume, VolumeSeries, VoxelData};

const PROTOCOL: &str = r#"
{
  "region": {"low": [0.1, 0.2, 0.0], "high": [0.9, 0.8, 1.0]},
  "snapshot": {"width": 1200, "height": 750},
  "sets": [
    {
      "name": "Nuclei",
      "color": [1.0, 0.7, 0.2],
      "source": {"kind": "surfaces", "channel": "DAPI", "threshold": 0.25, "grain": 0.1, "split": 0.6, "min_volume": 2.0, "opening": 0.2, "closing": 0.3, "min_sphericity": 0.4, "min_mean_intensity": 0.05},
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
      "color": [0.3, 0.8, 1.0],
      "source": {"kind": "spots", "channel": "GFP", "diameter": 0.45, "contrast": 0.1}
    },
    {
      "name": "Vessels",
      "color": [0.4, 0.9, 0.6],
      "source": {"kind": "filaments", "channel": "RFP", "threshold": 0.6, "min_length": 3.0}
    }
  ],
  "coloc": {"settings": {"permutations": 200, "block": 3}, "pairs": [{"a": "GFP", "b": "RFP", "thresholds": [0.4, 0.6]}, {"a": "GFP", "b": "DAPI", "thresholds": [0.5, 0.5]}], "mask": null}
}
"#;

#[test]
fn captures_the_manual_edits_of_a_merged_part() {
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
    let target = scene.add_object(ObjectKind::Spots, 0);
    let absorbed = scene.add_object(ObjectKind::Spots, 0);
    let mut accumulator = SampleAccumulator::new();
    accumulator.push(0.5);
    let statistics = accumulator.statistics();
    let frame = |centres: &[[f32; 3]]| {
        ObjectFrame::Spots(SpotSet::new(
            0,
            centres
                .iter()
                .map(|centre| Spot::new(*centre, 1.0, 0.5))
                .collect(),
            IntensityTable::new(1, vec![statistics; centres.len()]),
        ))
    };
    scene.install(target, 0, frame(&[[0.0, 0.0, 0.0], [2.0, 0.0, 0.0]]));
    scene.install(absorbed, 0, frame(&[[4.0, 0.0, 0.0]]));
    assert!(
        scene
            .delete_objects(target, 0, &[1])
            .expect("the spot is pruned")
    );
    scene
        .merge_objects(absorbed, target)
        .expect("the two spot sets merge");

    let protocol =
        Protocol::capture(&scene, &Measure::new(), None).expect("the merged analysis captures");
    let SourceSpec::Merged { parts } = &protocol.sets()[0].source else {
        panic!("the merged set holds its parts");
    };
    assert_eq!(parts.len(), 2, "the merged set keeps both parts");
    let curated = parts
        .iter()
        .find(|part| part.curation.is_some())
        .expect("a part keeps its manual edits");
    assert_eq!(
        curated
            .curation
            .as_ref()
            .expect("the part is curated")
            .expected,
        [Some(2)],
        "the part records the object count of its edits"
    );
    let text = protocol.to_json().expect("the protocol serializes");
    assert!(text.contains("anchors"), "{text}");

    let mut staged_scene = Scene::new(Dataset::synthetic("fixture", series.pyramid().clone()));
    let staged = protocol
        .stage(&mut staged_scene)
        .expect("the curated merge stages");
    let merged = staged_scene
        .objects
        .get(staged.set(0))
        .expect("the merged set is staged");
    assert_eq!(
        merged.curation().removal_count(),
        1,
        "the staged part carries the object the user removed"
    );
}

#[test]
fn reads_and_writes_a_protocol() {
    let protocol = Protocol::from_json(PROTOCOL).expect("the protocol is valid");
    assert_eq!(protocol.sets().len(), 3);
    assert_eq!(protocol.kind_of("Nuclei"), Some(ObjectKind::Surfaces));
    assert_eq!(protocol.kind_of("Puncta"), Some(ObjectKind::Spots));
    assert_eq!(protocol.kind_of("Vessels"), Some(ObjectKind::Filaments));
    let region = protocol
        .region()
        .expect("the protocol restricts its region");
    assert_eq!(region.low, [0.1, 0.2, 0.0]);
    assert_eq!(region.high, [0.9, 0.8, 1.0]);
    let snapshot = protocol
        .snapshot()
        .expect("the protocol renders a snapshot");
    assert_eq!(snapshot.size(), [1200, 750]);
    assert!(!snapshot.transparent);
    let surface = protocol.sets()[0]
        .source
        .params()
        .expect("a surface set carries its detection parameters");
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
    let measure = protocol.sets()[0]
        .measure()
        .expect("the surface set is measured");
    assert_eq!(measure.group(), grana_measure::Group::Track);
    assert_eq!(measure.aggregate(), grana_measure::Aggregate::Maximum);
    assert_eq!(measure.filters.len(), 1);
    assert_eq!(measure.filters[0].column, "volume_um3");
    assert_eq!(measure.filters[0].minimum, 1.0);
    assert_eq!(measure.filters[0].maximum, 9.0);
    assert!(protocol.sets()[1].measure().is_none());
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
        protocol.sets()[1].source,
        SourceSpec::Spots { .. }
    ));

    let text = protocol.to_json().expect("the protocol serializes");
    let restored = Protocol::from_json(&text).expect("the protocol round trips");
    assert_eq!(protocol, restored);
    assert_eq!(protocol.digest(), restored.digest());
}

#[test]
fn a_captured_snapshot_follows_the_export_width() {
    assert_eq!(
        SnapshotSpec::of(Some(Export::new(2400, true))).size(),
        [2400, 1500]
    );
    assert_eq!(SnapshotSpec::of(Some(Export::NATIVE)).size(), [1600, 1000]);
    assert_eq!(
        SnapshotSpec::of(Some(Export::new(64, false))).size(),
        [64, 64]
    );
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
            "\"snapshot\": {\"width\": 1200, \"height\": 750}",
            "\"snapshot\": {\"width\": 8, \"height\": 750}"
        )),
        "a snapshot covers a usable number of pixels"
    );
    assert!(
        invalid(&PROTOCOL.replace(
            "\"source\": {\"kind\": \"spots\", \"channel\": \"GFP\", \"diameter\": 0.45, \"contrast\": 0.1}",
            "\"source\": {\"kind\": \"spots\", \"channel\": \"GFP\", \"diameter\": 0.45, \"contrast\": 0.1}, \"extra\": 1"
        )),
        "an unknown field is rejected"
    );
    assert!(
        invalid(&PROTOCOL.replace("\"name\": \"Nuclei\"", "\"name\": \"puncta\"")),
        "two sets cannot write to one output name"
    );
    assert!(
        invalid(&PROTOCOL.replace(
            "\"source\": {\"kind\": \"filaments\", \"channel\": \"RFP\", \"threshold\": 0.6, \"min_length\": 3.0}",
            "\"source\": {\"kind\": \"filaments\", \"channel\": \"RFP\", \"threshold\": 0.6, \"min_length\": 3.0}, \"track\": {\"max_step\": 1.0, \"max_gap\": 1, \"division\": false, \"division_range\": 1.0, \"thickness\": 0.2}"
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
            "\"name\": \"Puncta\",\n      \"color\": [0.3, 0.8, 1.0],\n      \"source\": {\"kind\": \"spots\", \"channel\": \"GFP\", \"diameter\": 0.45, \"contrast\": 0.1}",
            "\"name\": \"Puncta\",\n      \"color\": [0.3, 0.8, 1.0],\n      \"source\": {\"kind\": \"spots\", \"channel\": \"GFP\", \"diameter\": 0.45, \"contrast\": 0.1},\n      \"measure\": {\"group\": \"tracks\", \"aggregate\": \"mean\"}"
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
    let protocol =
        Protocol::capture(&scene, &measure, None).expect("the scene analysis is capturable");
    assert_eq!(protocol.sets().len(), 1);
    let spec = &protocol.sets()[0];
    assert_eq!(spec.name, "Spots 1");
    assert_eq!(protocol.channel_of("Spots 1"), Some("DAPI"));
    assert_eq!(protocol.kind_of("Spots 1"), Some(ObjectKind::Spots));
    assert_eq!(spec.color, [0.2, 0.7, 0.9]);
    assert!(spec.track_params().is_some());
    assert!(spec.measure().is_none(), "no statistics set is selected");
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
    let snapshot = protocol
        .snapshot()
        .expect("a captured protocol renders a snapshot");
    assert_eq!(snapshot.size(), [1600, 1000]);
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
    assert!(scene.add_seed(first, [0.25, -0.5, 0.75]));
    let protocol =
        Protocol::capture(&scene, &Measure::new(), None).expect("the masked analysis captures");
    assert_eq!(protocol.sets().len(), 2);
    assert_eq!(protocol.sets()[0].name, "Surfaces 1");
    assert_eq!(protocol.sets()[0].mask(), None);
    assert_eq!(
        protocol.sets()[0].source.seeds().to_vec(),
        vec![vec![[0.25, -0.5, 0.75]]]
    );
    assert_eq!(protocol.sets()[1].name, "Spots 1");
    assert_eq!(protocol.sets()[1].mask(), Some("Surfaces 1"));
    assert!(protocol.sets()[1].source.seeds().is_empty());
    let text = protocol.to_json().expect("the protocol serializes");
    let restored = Protocol::from_json(&text).expect("the protocol round trips");
    assert_eq!(protocol, restored);
}

#[test]
fn captures_and_stages_a_surface_seeded_from_another_set() {
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
                "Membrane",
                [0.3, 1.0, 0.4],
                VoxelData::U16(vec![0; extent.voxel_count() as usize]),
            ),
        ],
    );
    let movie = VolumeSeries::new(vec![volume], 1.0);
    let mut scene = Scene::new(Dataset::synthetic("fixture", movie.pyramid().clone()));
    let cells = scene.add_object(ObjectKind::Surfaces, 1);
    let nuclei = scene.add_object(ObjectKind::Surfaces, 0);
    assert!(scene.set_seed_source(cells, Some(nuclei)));
    assert!(scene.add_seed(nuclei, [0.25, -0.5, 0.75]));

    let protocol =
        Protocol::capture(&scene, &Measure::new(), None).expect("the seeded analysis captures");
    assert_eq!(protocol.sets().len(), 2);
    assert_eq!(
        protocol.sets()[0].name,
        "Surfaces 2",
        "the seed source stages before the seeded set"
    );
    assert_eq!(protocol.sets()[1].name, "Surfaces 1");
    assert_eq!(
        protocol.sets()[1].source.seed_source(),
        Some("Surfaces 2"),
        "the seeded set names its source"
    );
    assert!(protocol.sets()[1].source.seeds().is_empty());
    assert_eq!(
        protocol.sets()[0].source.seeds().to_vec(),
        vec![vec![[0.25, -0.5, 0.75]]],
        "the source keeps its manual seeds"
    );
    let text = protocol.to_json().expect("the protocol serializes");
    let restored = Protocol::from_json(&text).expect("the protocol round trips");
    assert_eq!(protocol, restored);

    let mut staged_scene = Scene::new(Dataset::synthetic("fixture", movie.pyramid().clone()));
    let staged = protocol
        .stage(&mut staged_scene)
        .expect("the seeded protocol stages");
    assert_eq!(
        staged_scene.seed_source(staged.set(1)),
        Some(staged.set(0)),
        "the staged cells seed from the staged nuclei"
    );
    assert_eq!(
        staged_scene.seeded_dependents(staged.set(0)),
        [staged.set(1)]
    );

    let mut json = serde_json::to_value(&protocol).expect("the protocol carries into json");
    let sets = json["sets"]
        .as_array_mut()
        .expect("the protocol names sets");
    sets.reverse();
    let failure = Protocol::from_json(&serde_json::to_string(&json).expect("the json is text"))
        .expect_err("a seed source cannot run after its dependent");
    assert!(
        failure.to_string().contains("does not run before"),
        "{failure}"
    );
}

#[test]
fn stages_the_presentation_of_a_captured_protocol() {
    let extent = Extent::new(8, 8, 8);
    let volume = Volume::new(
        extent,
        Spacing::new(0.5, 0.5, 1.0),
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
    let series = VolumeSeries::new(vec![volume], 1.0);
    let mut scene = Scene::new(Dataset::synthetic("fixture", series.pyramid().clone()));
    let nuclei = scene.add_object(ObjectKind::Surfaces, 0);
    let beads = scene.add_object(ObjectKind::Spots, 1);
    assert!(scene.rename_object(nuclei, "Nuclei"));
    assert!(scene.rename_object(beads, "Beads"));
    scene.channels[0].enabled = false;
    scene.channels[1].opacity = 0.4;
    scene.exposure = 1.8;
    scene.step_scale = 0.6;
    scene.shading = Shading {
        ambient: 0.2,
        diffuse: 0.6,
        specular: 0.4,
        shininess: 40.0,
        shadows: 0.3,
    };
    scene.set_transfer(
        1,
        TransferFunction::new(vec![
            Stop::new(0.0, [0.0, 0.0, 0.0], 0.0),
            Stop::new(1.0, [0.2, 0.9, 0.4], 0.8),
        ]),
    );
    scene.set_region(Region::new(
        Vec3::new(-1.0, -1.0, -2.0),
        Vec3::new(1.0, 1.0, 2.0),
    ));
    scene.set_clip(Clip::new(Vec3::new(0.0, 0.0, -1.0), 1.2));
    scene.set_mask(Some(nuclei));
    assert!(scene.add_seed(nuclei, [0.5, -1.0, 1.5]));
    assert!(scene.add_relation(beads, nuclei, 0));
    let set = scene.objects.get_mut(nuclei).expect("the surface set");
    set.set_color([0.9, 0.5, 0.2]);
    set.set_opacity(0.35);
    set.set_visible(false);
    set.set_track_visible(false);
    let mut object = Coloring::new();
    object.set_metric(Some(ObjectColor::Measurement(Measurement::Diameter)));
    object.set_palette(Palette::Magma);
    object.set_reversed(true);
    object.set_manual(Some((0.5, 2.5)));
    scene
        .objects
        .get_mut(beads)
        .expect("the spot set")
        .set_object_coloring(object);
    let mut track = Coloring::new();
    track.set_metric(Some(TrackColor::Measurement(TrackMeasurement::Speed)));
    scene
        .objects
        .get_mut(beads)
        .expect("the spot set")
        .set_track_coloring(track);
    let camera = CameraState {
        pivot: [1.0, 2.0, 3.0],
        yaw: 0.4,
        pitch: -0.2,
        distance: 42.0,
        fov_y: 0.6,
    };
    let section = SectionState {
        axis: Axis::X,
        span: 7.5,
    };
    let timeline = Timeline::from_parts(
        24,
        6.0,
        vec![
            Keyframe {
                at: 0.0,
                time: 0,
                camera,
                section,
            },
            Keyframe {
                at: 3.0,
                time: 1,
                camera: CameraState { yaw: 1.2, ..camera },
                section,
            },
        ],
    )
    .expect("the timeline is valid");
    let view = ViewSpec::of(
        &ViewState {
            camera,
            section,
            split: 0.5,
            timeline: timeline.clone(),
            export: Export::new(1280, true),
        },
        0,
    );
    let protocol =
        Protocol::capture(&scene, &Measure::new(), Some(view.clone())).expect("the scene captures");
    let text = protocol.to_json().expect("the protocol serializes");
    let protocol = Protocol::from_json(&text).expect("the protocol round trips");
    assert_eq!(protocol.sets()[0].name, "Nuclei");
    assert_eq!(protocol.sets()[0].mask(), None);
    assert_eq!(protocol.sets()[1].mask(), Some("Nuclei"));
    assert_eq!(protocol.channels().len(), 2);
    assert!(!protocol.channels()[0].enabled);
    assert_eq!(protocol.channels()[1].opacity, 0.4);
    assert_eq!(
        protocol.channels()[1]
            .transfer
            .as_ref()
            .expect("the channel transfer captures")
            .stops
            .len(),
        2
    );
    assert_eq!(protocol.render().map(|render| render.exposure), Some(1.8));
    assert_eq!(
        protocol.sets()[0].track_visible(),
        Some(false),
        "the track appearance captures"
    );
    assert_eq!(protocol.view(), Some(&view));
    let played = protocol
        .view()
        .and_then(ViewSpec::timeline)
        .expect("the timeline captures");
    assert_eq!(played.fps(), 24);
    assert_eq!(played.duration(), 6.0);
    assert_eq!(played.keys().len(), 2);

    let mut target = Scene::new(Dataset::synthetic("fixture", series.pyramid().clone()));
    let staged = protocol
        .stage(&mut target)
        .expect("the protocol stages onto a fresh scene");
    assert_eq!(staged.len(), 2);
    let staged_nuclei = staged.set(0);
    let staged_beads = staged.set(1);
    let names = target
        .objects
        .iter()
        .map(|set| set.name().to_string())
        .collect::<Vec<_>>();
    assert_eq!(names, ["Nuclei", "Beads"]);
    assert!(!target.channels[0].enabled);
    assert_eq!(target.channels[1].opacity, 0.4);
    assert_eq!(target.channels[1].transfer(), scene.channels[1].transfer());
    assert_eq!(target.exposure, 1.8);
    assert_eq!(target.step_scale, 0.6);
    assert_eq!(target.shading, scene.shading);
    assert_eq!(target.mask, Some(staged_nuclei));
    assert_eq!(target.region.min(), scene.region.min());
    assert_eq!(target.region.max(), scene.region.max());
    assert_eq!(target.clip, scene.clip, "the clipping plane captures");
    let nuclei_set = target.objects.get(staged_nuclei).expect("the surface set");
    assert_eq!(nuclei_set.color(), [0.9, 0.5, 0.2]);
    assert_eq!(nuclei_set.opacity(), 0.35);
    assert!(!nuclei_set.visible());
    assert!(!nuclei_set.track_visible());
    assert_eq!(nuclei_set.seeds(0), [[0.5, -1.0, 1.5]]);
    let beads_set = target.objects.get(staged_beads).expect("the spot set");
    assert!(beads_set.relation(staged_nuclei, 0).is_some());
    let coloring = beads_set.object_coloring();
    assert_eq!(
        coloring.metric(),
        Some(ObjectColor::Measurement(Measurement::Diameter))
    );
    assert_eq!(coloring.palette(), Palette::Magma);
    assert!(coloring.reversed());
    assert_eq!(coloring.manual(), Some((0.5, 2.5)));
    assert_eq!(
        beads_set.track_coloring().metric(),
        Some(TrackColor::Measurement(TrackMeasurement::Speed))
    );
    assert_eq!(
        protocol.view().and_then(ViewSpec::export),
        Some(Export::new(1280, true)),
        "the export settings travel with the view"
    );
}

#[test]
fn captures_and_stages_a_relation_across_a_shifted_frame() {
    let extent = Extent::new(4, 4, 4);
    let volume = Volume::new(
        extent,
        Spacing::new(1.0, 1.0, 1.0),
        vec![Channel::new(
            "DAPI",
            [1.0, 1.0, 1.0],
            VoxelData::U16(vec![0; extent.voxel_count() as usize]),
        )],
    );
    let series = VolumeSeries::new(vec![volume.clone(); 3], 1.0);
    let mut scene = Scene::new(Dataset::synthetic("fixture", series.pyramid().clone()));
    let nuclei = scene.add_object(ObjectKind::Surfaces, 0);
    let beads = scene.add_object(ObjectKind::Spots, 0);
    assert!(scene.rename_object(nuclei, "Nuclei"));
    assert!(scene.rename_object(beads, "Beads"));
    assert!(scene.add_relation(beads, nuclei, -2));
    let mut coloring = scene
        .objects
        .get(beads)
        .expect("the spot set")
        .track_coloring()
        .clone();
    coloring.set_metric(Some(TrackColor::Relation {
        target: nuclei,
        field: RelationField::Inside,
        aggregate: Aggregate::Minimum,
        shift: -2,
    }));
    scene
        .objects
        .get_mut(beads)
        .expect("the spot set")
        .set_track_coloring(coloring);

    let protocol =
        Protocol::capture(&scene, &Measure::new(), None).expect("the shifted relation captures");
    assert_eq!(protocol.relations()[0].shift, -2);
    let text = protocol.to_json().expect("the protocol serializes");
    let restored = Protocol::from_json(&text).expect("the protocol round trips");
    assert_eq!(restored, protocol);

    let mut target = Scene::new(Dataset::synthetic("fixture", series.pyramid().clone()));
    let staged = restored
        .stage(&mut target)
        .expect("the protocol stages onto a fresh scene");
    let relation = target
        .objects
        .get(staged.set(1))
        .and_then(|set| set.relation(staged.set(0), -2))
        .expect("the staged relation joins the two sets");
    assert_eq!(relation.shift(), -2);
    assert_eq!(
        target
            .objects
            .get(staged.set(1))
            .expect("the staged spot set")
            .track_coloring()
            .metric(),
        Some(TrackColor::Relation {
            target: staged.set(0),
            field: RelationField::Inside,
            aggregate: Aggregate::Minimum,
            shift: -2,
        })
    );

    let single = VolumeSeries::new(vec![volume], 1.0);
    let mut narrow = Scene::new(Dataset::synthetic("fixture", single.pyramid().clone()));
    assert!(
        restored.stage(&mut narrow).is_err(),
        "a shift outside the frames of the dataset is refused"
    );
}

#[test]
fn round_trips_the_statistics_presentation() {
    let extent = Extent::new(4, 4, 2);
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
    let series = VolumeSeries::new(vec![volume.clone(); 2], 1.0);
    let mut scene = Scene::new(Dataset::synthetic("fixture", series.pyramid().clone()));
    let spots = scene.add_object(ObjectKind::Spots, 0);
    assert!(scene.rename_object(spots, "Puncta"));
    for time in 0..2 {
        let mut accumulator = SampleAccumulator::new();
        accumulator.push(0.5);
        scene.install(
            spots,
            time,
            ObjectFrame::Spots(SpotSet::new(
                0,
                vec![
                    Spot::new([0.5 * time as f32, 0.0, 0.0], 0.5, 0.4),
                    Spot::new([1.0 + time as f32, 0.0, 0.0], 0.75, 0.6),
                ],
                IntensityTable::new(2, vec![accumulator.statistics(); 4]),
            )),
        );
    }
    let mut measure = Measure::new();
    measure.select(spots, &scene);
    measure.set_scope(Scope::Current);
    measure.set_view(MeasureView::Scatter);
    measure.set_visibility(false);
    let (x, y) = {
        let table = measure.table().expect("the table exists");
        (
            table
                .column("contrast")
                .expect("the table measures contrast"),
            table
                .column("diameter_um")
                .expect("the table measures diameter"),
        )
    };
    measure.set_axis(grana_measure::PlotAxis::X, x);
    measure.set_axis(grana_measure::PlotAxis::Y, y);
    measure.set_lasso(Some(
        Lasso::new(x, y, vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]])
            .expect("the polygon covers the plot"),
    ));
    let last = measure.visible().len() - 1;
    let shown = measure.visible()[last];
    measure.set_column_visible(last, !shown);

    let protocol = Protocol::capture(&scene, &measure, None).expect("the statistics capture");
    let text = protocol.to_json().expect("the protocol serializes");
    let restored = Protocol::from_json(&text).expect("the protocol round trips");
    let spec = restored.sets()[0]
        .measure()
        .expect("the protocol carries the statistics");
    assert_eq!(spec.scope(), Scope::Current);
    assert_eq!(spec.view(), Some(MeasureView::Scatter));
    assert_eq!(spec.axes(), Some(["contrast", "diameter_um"]));
    assert!(!spec.visible());
    assert!(spec.lasso().is_some());

    let mut presented = Measure::new();
    restored.present_measure(&[spots], &mut presented, &scene);
    assert_eq!(presented.scope(), Scope::Current);
    assert_eq!(presented.view(), MeasureView::Scatter);
    assert!(!presented.visibility());
    assert_eq!(presented.axes(), [x, y]);
    assert!(presented.lasso().is_some());
    assert_eq!(presented.visible()[last], !shown);
}

#[test]
fn stages_a_derived_object_set_graph() {
    const DERIVED: &str = r#"
{
  "sets": [
    {
      "name": "Spots 1",
      "color": [0.3, 0.8, 1.0],
      "source": {"kind": "spots", "channel": "GFP", "diameter": 0.45, "contrast": 0.1}
    },
    {
      "name": "Big spots",
      "color": [1.0, 0.7, 0.2],
      "source": {"kind": "filtered", "source": "Spots 1", "filters": [{"column": "diameter_um", "minimum": 0.5, "maximum": 1.0}]}
    },
    {
      "name": "All spots",
      "color": [0.4, 0.9, 0.6],
      "source": {"kind": "copy", "source": "Spots 1"}
    },
    {
      "name": "Union",
      "color": [0.9, 0.5, 0.2],
      "source": {"kind": "merged", "parts": [
        {"name": "Spots 1", "source": {"kind": "spots", "channel": "GFP", "diameter": 0.45, "contrast": 0.1}},
        {"name": "All spots", "source": {"kind": "copy", "source": "Spots 1"}}
      ]}
    }
  ]
}
"#;
    let protocol = Protocol::from_json(DERIVED).expect("the derived protocol is valid");
    assert_eq!(protocol.kind_of("Big spots"), Some(ObjectKind::Spots));
    assert_eq!(protocol.channel_of("Union"), Some("GFP"));
    assert_eq!(
        protocol.sets()[1].source,
        SourceSpec::Filtered {
            source: "Spots 1".to_string(),
            group: "objects".to_string(),
            filters: vec![grana_protocol::FilterSpec {
                column: "diameter_um".to_string(),
                minimum: 0.5,
                maximum: 1.0,
            }],
            lasso: None,
        }
    );
    let text = protocol.to_json().expect("the protocol serializes");
    let restored = Protocol::from_json(&text).expect("the derived protocol round trips");
    assert_eq!(protocol, restored);

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
    let series = VolumeSeries::new(vec![volume], 1.0);
    let mut scene = Scene::new(Dataset::synthetic("fixture", series.pyramid().clone()));
    let staged = restored
        .stage(&mut scene)
        .expect("the derived protocol stages");
    assert_eq!(staged.len(), 4);
    let filtered = scene
        .objects
        .get(staged.set(1))
        .expect("the filtered set is staged");
    assert!(matches!(
        filtered.provenance(),
        grana_scene::Provenance::Filtered { .. }
    ));
    assert_eq!(filtered.name(), "Big spots");
    let copied = scene
        .objects
        .get(staged.set(2))
        .expect("the copied set is staged");
    assert!(matches!(
        copied.provenance(),
        grana_scene::Provenance::Copied { .. }
    ));
    for step in staged.steps() {
        if let grana_protocol::Step::Merge { set, part } = step {
            scene
                .merge_objects(*part, *set)
                .expect("the staged parts merge");
        }
    }
    let union = scene
        .objects
        .get(staged.set(3))
        .expect("the merged set is staged");
    assert!(union.provenance().is_merged());
    assert_eq!(union.provenance().parts().len(), 2);
    assert_eq!(
        staged
            .steps()
            .iter()
            .filter(|step| matches!(step, grana_protocol::Step::Merge { .. }))
            .count(),
        1
    );
    assert_eq!(
        staged
            .steps()
            .iter()
            .filter(|step| matches!(step, grana_protocol::Step::Filter { .. }))
            .count(),
        1
    );
    assert_eq!(
        staged
            .steps()
            .iter()
            .filter(|step| matches!(step, grana_protocol::Step::Copy { .. }))
            .count(),
        2
    );
}

#[test]
fn rejects_a_protocol_that_names_presentation_it_cannot_reach() {
    let invalid = |text: &str| Protocol::from_json(text).is_err();
    assert!(
        invalid(&PROTOCOL.replace(
            "\"color\": [0.3, 0.8, 1.0],",
            "\"color\": [0.3, 0.8, 1.0], \"opacity\": 1.5,"
        )),
        "an object set opacity is a unit fraction"
    );
    assert!(
        invalid(&PROTOCOL.replace(
            "\"color\": [0.3, 0.8, 1.0],",
            "\"color\": [0.3, 0.8, 1.0], \"color_by\": {\"key\": \"volume_um3\", \"palette\": \"ember\"},"
        )),
        "a color names a palette"
    );
    assert!(
        invalid(&PROTOCOL.replace(
            "\"color\": [0.3, 0.8, 1.0],",
            "\"color\": [0.3, 0.8, 1.0], \"color_by\": {\"target\": \"Missing\", \"field\": \"distance\", \"palette\": \"viridis\"},"
        )),
        "a color names a relation within the protocol"
    );
    assert!(
        invalid(&PROTOCOL.replace(
            "  \"region\":",
            "  \"view\": {\"camera\": {\"pivot\": [0.0, 0.0, 0.0], \"yaw\": 0.0, \"pitch\": 0.0, \"distance\": 1.0, \"fov_y\": 0.6}, \"section\": {\"axis\": \"z\", \"span\": 1.0}, \"split\": 0.5, \"timeline\": {\"fps\": 0, \"duration\": 4.0}},\n  \"region\":"
        )),
        "a timeline plays one to 240 frames per second"
    );
    assert!(
        invalid(&PROTOCOL.replace(
            "  \"region\":",
            "  \"render\": {\"exposure\": 0.0, \"step_scale\": 1.0, \"shading\": {\"ambient\": 0.3, \"diffuse\": 0.7, \"specular\": 0.3, \"shininess\": 28.0, \"shadows\": 0.7}},\n  \"region\":"
        )),
        "a rendering scales radiance by a positive exposure"
    );
    assert!(
        invalid(&PROTOCOL.replace(
            "  \"region\":",
            "  \"channels\": [{\"name\": \"DAPI\", \"enabled\": true, \"opacity\": 1.0, \"window\": {\"low\": 0.0, \"high\": 1.0}, \"transfer\": {\"stops\": []}}],\n  \"region\":"
        )),
        "a channel transfer holds at least one stop"
    );
    assert!(
        invalid(&PROTOCOL.replace(
            "  \"region\":",
            "  \"view\": {\"camera\": {\"pivot\": [0.0, 0.0, 0.0], \"yaw\": 0.0, \"pitch\": 0.0, \"distance\": -1.0, \"fov_y\": 0.6}, \"section\": {\"axis\": \"z\", \"span\": 1.0}, \"split\": 0.5},\n  \"region\":"
        )),
        "a view describes a plausible camera"
    );
}

#[test]
fn stages_a_relation_before_the_filtered_set_that_reads_it() {
    let extent = Extent::new(4, 4, 4);
    let volume = Volume::new(
        extent,
        Spacing::new(1.0, 1.0, 1.0),
        vec![Channel::new(
            "DAPI",
            [1.0, 1.0, 1.0],
            VoxelData::U16(vec![0; extent.voxel_count() as usize]),
        )],
    );
    let series = VolumeSeries::new(vec![volume], 1.0);
    let mut scene = Scene::new(Dataset::synthetic("fixture", series.pyramid().clone()));
    let puncta = scene.add_object(ObjectKind::Spots, 0);
    let nuclei = scene.add_object(ObjectKind::Surfaces, 0);
    assert!(scene.rename_object(puncta, "Puncta"));
    assert!(scene.rename_object(nuclei, "Nuclei"));
    assert!(scene.add_relation(puncta, nuclei, 0));
    let criteria = Criteria::new(
        Group::Object,
        vec![FilterRange::new("nuclei_distance_um", 0.0, 1.0)],
        None,
    );
    assert!(
        scene.filter_object(puncta, "filtered", criteria).is_some(),
        "the filtered set derives from the puncta"
    );
    let protocol =
        Protocol::capture(&scene, &Measure::new(), None).expect("the relation filter captures");
    let mut staged_scene = Scene::new(Dataset::synthetic("fixture", series.pyramid().clone()));
    let staged = protocol
        .stage(&mut staged_scene)
        .expect("the relation filter stages");
    let steps = staged.steps();
    let relation = steps
        .iter()
        .position(|step| matches!(step, Step::Relation { .. }))
        .expect("the protocol measures the relation");
    let filter = steps
        .iter()
        .position(|step| matches!(step, Step::Filter { .. }))
        .expect("the protocol filters the puncta");
    assert!(
        relation < filter,
        "the relation runs before the filter that reads its column"
    );
    assert_eq!(steps.last(), Some(&Step::Present));
}
