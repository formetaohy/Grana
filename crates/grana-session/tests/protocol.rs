use glam::Vec3;
use grana_animation::{Keyframe, Timeline};
use grana_display::{
    CameraState, Coloring, Palette, SectionState, Shading, Stop, TransferFunction,
};
use grana_measure::{
    ColocCost, ColocFrame, ColocPair, ColocSeries, ColocSeriesInfo, ColocSignificance,
    ColocThresholds, Measurement, TrackMeasurement,
};
use grana_project::ViewState;
use grana_scene::{Dataset, Measure, ObjectColor, ObjectKind, Scene};
use grana_session::{DetectionSpec, Protocol, ViewSpec};
use grana_volume::{Axis, Channel, Extent, Region, Spacing, Volume, VolumeSeries, VoxelData};

const PROTOCOL: &str = r#"
{
  "region": {"low": [0.1, 0.2, 0.0], "high": [0.9, 0.8, 1.0]},
  "sets": [
    {
      "name": "Nuclei",
      "channel": "DAPI",
      "color": [1.0, 0.7, 0.2],
      "detection": {"kind": "surfaces", "threshold": 0.25, "grain": 0.1, "split": 0.6, "min_volume": 2.0, "opening": 0.2, "closing": 0.3, "min_sphericity": 0.4, "min_mean_intensity": 0.05},
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
    let protocol =
        Protocol::capture(&scene, &measure, None).expect("the scene analysis is capturable");
    assert_eq!(protocol.sets().len(), 1);
    let spec = &protocol.sets()[0];
    assert_eq!(spec.name, "Spots 1");
    assert_eq!(spec.channel, "DAPI");
    assert_eq!(spec.kind(), ObjectKind::Spots);
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
        protocol.sets()[0].seeds().to_vec(),
        vec![vec![[0.25, -0.5, 0.75]]]
    );
    assert_eq!(protocol.sets()[1].name, "Spots 1");
    assert_eq!(protocol.sets()[1].mask(), Some("Surfaces 1"));
    assert!(protocol.sets()[1].seeds().is_empty());
    let text = protocol.to_json().expect("the protocol serializes");
    let restored = Protocol::from_json(&text).expect("the protocol round trips");
    assert_eq!(protocol, restored);
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
    scene.set_mask(Some(nuclei));
    assert!(scene.add_seed(nuclei, [0.5, -1.0, 1.5]));
    assert!(scene.add_relation(beads, nuclei));
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
    track.set_metric(Some(TrackMeasurement::Speed));
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
    let nuclei_set = target.objects.get(staged_nuclei).expect("the surface set");
    assert_eq!(nuclei_set.color(), [0.9, 0.5, 0.2]);
    assert_eq!(nuclei_set.opacity(), 0.35);
    assert!(!nuclei_set.visible());
    assert!(!nuclei_set.track_visible());
    assert_eq!(nuclei_set.seeds(0), [[0.5, -1.0, 1.5]]);
    let beads_set = target.objects.get(staged_beads).expect("the spot set");
    assert!(beads_set.relation(staged_nuclei).is_some());
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
        Some(TrackMeasurement::Speed)
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
