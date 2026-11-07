use std::fs;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use glam::Vec3;
use grana_analysis::{Sources, SurfaceBuilder, recut};
use grana_animation::{Keyframe, Timeline};
use grana_display::{CameraState, Palette, SectionState, TransferFunction};
use grana_geometry::GeometryStore;
use grana_geometry::{Bounds, Mesh, MeshVertex};
use grana_measure::Measurement;
use grana_measure::{
    Aggregate, ChildField, ChildMetric, ColocCost, ColocFrame, ColocPair, ColocSeries,
    ColocSeriesInfo, ColocSignificance, RelationField, Statistic,
};
use grana_object::{
    FilamentSet, FilamentStatistics, FilamentTopology, IntensityTable, RelationFrame,
    SampleAccumulator, SampleStatistics, Spot, SpotSet, SurfaceSet, SurfaceStatistics, Track,
    TrackNode, TrackSet,
};
use grana_object::{SpotParams, SurfaceFilter, SurfaceParams};
use grana_project::{Export, Project, Source, ViewState};
use grana_scene::{
    Criteria, Dataset, Drift, FilterRange, Item, MaskFilter, Measure, ObjectColor, ObjectFrame,
    ObjectKind, ObjectParams, Scene, Selection, TrackColor, Visibility,
};
use grana_volume::{Axis, Channel, Clip, Extent, Spacing, Volume, VolumeSeries, VoxelData};

const EXTENT: Extent = Extent::new(8, 8, 4);

#[test]
fn round_trips_a_project() {
    let mut scene = fixture();
    scene.set_mask_radius(1.5);
    stage(&mut scene);
    let mut measure = Measure::new();
    let spots = scene
        .objects
        .iter()
        .find(|set| set.kind() == ObjectKind::Spots)
        .expect("the fixture stages spots")
        .id();
    let surfaces = scene
        .objects
        .iter()
        .find(|set| set.kind() == ObjectKind::Surfaces)
        .expect("the fixture stages surfaces")
        .id();
    let mask = scene
        .add_mask(spots, 0)
        .expect("the fixture masks its channel");
    scene
        .objects
        .get_mut(spots)
        .expect("the spots exist")
        .set_visibility(Visibility::Object(Arc::new(vec![
            vec![true, false],
            vec![false, true],
        ])));
    scene
        .objects
        .get_mut(surfaces)
        .expect("the surfaces exist")
        .set_visibility(Visibility::Track(Arc::new(vec![true, false, true])));
    scene
        .objects
        .get_mut(spots)
        .expect("the spots exist")
        .set_labels(true);
    assert!(scene.set_label_metric(spots, Some(ObjectColor::Measurement(Measurement::Diameter))));
    assert!(scene.set_object_hidden(spots, 0, &[1], true));
    scene.set_time(1);
    assert!(scene.add_seed(surfaces, [0.25, -0.5, 0.75]));
    assert!(scene.add_seed(surfaces, [-0.25, 0.5, -0.75]));
    scene.set_time(0);
    scene.select(Some(Selection {
        set: surfaces,
        item: Item::Track(2),
    }));
    let hidden_point = scene.annotations.add_point([0.1, 0.2, 0.3]);
    scene
        .annotations
        .add_distance([0.1, 0.2, 0.3], [0.4, -0.2, 0.1]);
    let roi = scene.annotations.add_roi([0.0, 0.0, 0.0], 1.0);
    assert!(scene.annotations.set_visible(hidden_point, false));
    assert!(scene.annotations.set_radius(roi, 1.25));
    assert!(scene.annotations.select(Some(roi)));
    assert!(scene.annotations.set_roi(Some(roi)));
    measure.select(spots, &scene);
    measure.set_group(grana_measure::Group::Track, &scene);
    measure.add_filter();
    let columns = measure
        .table()
        .expect("the track table builds")
        .columns()
        .len();
    measure.toggle_sort(columns - 1);
    measure.set_column_visible(0, false);
    measure.set_view(grana_measure::MeasureView::Scatter);
    let axes = measure.axes();
    let (lasso_points, x, y) = {
        let table = measure.table().expect("the track table builds");
        let (x_low, x_high) = table.extent(axes[0]).expect("the x column holds values");
        let (y_low, y_high) = table.extent(axes[1]).expect("the y column holds values");
        (
            vec![
                [x_low, y_low],
                [x_high, y_low],
                [x_high, y_high],
                [x_low, y_high],
            ],
            axes[0],
            axes[1],
        )
    };
    assert_ne!(x, y, "a scatter plots two columns");
    measure.set_lasso(Some(
        grana_measure::Lasso::new(x, y, lasso_points).expect("the polygon covers the plot"),
    ));
    measure.set_scope(grana_measure::Scope::Current);
    let view = ViewState {
        camera: CameraState {
            pivot: [1.0, -2.0, 0.5],
            yaw: 0.4,
            pitch: -0.3,
            distance: 12.0,
            fov_y: 0.6,
        },
        section: SectionState {
            axis: Axis::Y,
            span: 30.0,
        },
        split: 0.42,
        timeline: timeline(),
        export: Export::new(2048, true),
    };
    let source = Source::Synthetic {
        name: "fixture".to_string(),
    };
    let project = Project::capture(&scene, &measure, source, view);
    let path = std::env::temp_dir().join(format!("grana-project-{}.grana", std::process::id()));
    let mut steps = Vec::new();
    let mut record = |done: usize, total: usize| steps.push((done, total));
    project
        .write(&path, &AtomicBool::new(false), &mut record)
        .expect("the project writes");
    assert!(
        steps.windows(2).all(|pair| pair[0].0 < pair[1].0),
        "the save reports every step once"
    );
    let total = steps.last().expect("the save reports its steps").1;
    assert_eq!(steps.last().map(|step| step.0), Some(total));
    let blocked = path.with_extension("cancelled.grana");
    fs::remove_file(&blocked).ok();
    assert!(matches!(
        project.write(&blocked, &AtomicBool::new(true), &mut |_, _| {}),
        Err(grana_project::Error::Cancelled)
    ));
    assert!(!blocked.exists(), "a cancelled save leaves no file");
    let mut opened = Vec::new();
    let mut record = |done: usize, total: usize| opened.push((done, total));
    let loaded = Project::read(
        &path,
        GeometryStore::temporary(1).expect("the store opens a spill file"),
        &AtomicBool::new(false),
        &mut record,
    )
    .expect("the project reads");
    assert_eq!(opened.first().map(|step| step.1), Some(total));
    assert_eq!(opened.last().map(|step| step.0), Some(total));
    fs::remove_file(&path).ok();
    assert_eq!(loaded, project);

    let mut restored = fixture();
    let masks = loaded.masks.clone();
    let infos = masks
        .iter()
        .map(|spec| spec.info(&restored.dataset.channels()[spec.channel()]))
        .collect::<Vec<_>>();
    restored
        .set_mask_layout(infos)
        .expect("the mask channels fit the dataset");
    restored.restore(loaded.scene);
    restored
        .bind_masks(masks)
        .expect("the mask channels bind their object sets");
    assert_eq!(restored.dataset.channels().len(), 3);
    assert_eq!(restored.masks().len(), 1);
    assert_eq!(
        restored.dataset.channels()[mask].name(),
        loaded.masks[0].name()
    );
    assert_eq!(restored.masks()[0].set(), spots);
    let mut restored_measure = Measure::new();
    restored_measure.restore(loaded.measure, &restored);
    assert_eq!(restored.state(), project.scene);
    assert!(
        restored
            .objects
            .get(spots)
            .expect("the spots exist")
            .labels(),
        "the restored set keeps its object labels"
    );
    assert_eq!(
        restored
            .objects
            .get(spots)
            .expect("the spots exist")
            .label_metric(),
        Some(ObjectColor::Measurement(Measurement::Diameter)),
        "the restored set keeps its label metric"
    );
    assert!(
        restored
            .objects
            .get(spots)
            .expect("the spots exist")
            .hidden_object(0, 1),
        "the restored set keeps its hidden object"
    );
    restored.set_time(1);
    assert_eq!(restored.seed_count(surfaces), 2);
    restored.set_time(0);
    assert_eq!(restored_measure.state(), project.measure);
    assert_eq!(restored.annotations.len(), 3);
    assert!(
        !restored
            .annotations
            .get(hidden_point)
            .expect("the point exists")
            .visible()
    );
    assert_eq!(
        restored.annotations.roi_geometry().map(|roi| roi.radius()),
        Some(1.25)
    );
    assert_eq!(loaded.view, project.view);
    let restored_result = restored
        .coloc
        .result()
        .expect("the restored scene keeps its colocalization result");
    let captured_result = &project.scene.coloc.results[0];
    assert_eq!(restored_result.series(), captured_result.series());
    assert!(restored_result.significance().is_some());
    assert_eq!(restored_result.costes(), captured_result.costes());
    assert_eq!(restored_result.level(), 0);
    assert_eq!(restored_result.permutation_level(), 1);
    assert_eq!(restored_result.permutation_stride(), 1);
}

#[test]
fn round_trips_an_analysis_mask_filter() {
    let mut scene = fixture();
    stage(&mut scene);
    let spots = scene
        .objects
        .iter()
        .find(|set| set.kind() == ObjectKind::Spots)
        .expect("the fixture stages spots")
        .id();
    let surfaces = scene
        .objects
        .iter()
        .find(|set| set.kind() == ObjectKind::Surfaces)
        .expect("the fixture stages surfaces")
        .id();
    scene.set_mask(Some(spots));
    let filter = MaskFilter::new(surfaces, -1, RelationField::Distance, 0.25, 1.75);
    assert!(scene.set_mask_filter(Some(filter)));
    let view = ViewState {
        camera: CameraState {
            pivot: [0.0, 0.0, 0.0],
            yaw: 0.0,
            pitch: 0.0,
            distance: 10.0,
            fov_y: 0.6,
        },
        section: SectionState {
            axis: Axis::Z,
            span: 20.0,
        },
        split: 0.5,
        timeline: timeline(),
        export: Export::NATIVE,
    };
    let project = Project::capture(
        &scene,
        &Measure::new(),
        Source::Synthetic {
            name: "fixture".to_string(),
        },
        view,
    );
    assert_eq!(project.scene.mask_filter, Some(filter));
    let path = std::env::temp_dir().join(format!("grana-mask-{}.grana", std::process::id()));
    project
        .write(&path, &AtomicBool::new(false), &mut |_, _| {})
        .expect("the project writes");
    let loaded = Project::read(
        &path,
        GeometryStore::temporary(1).expect("the store opens a spill file"),
        &AtomicBool::new(false),
        &mut |_, _| {},
    )
    .expect("the project reads");
    fs::remove_file(&path).ok();
    assert_eq!(loaded, project);
    assert_eq!(loaded.scene.mask_filter, Some(filter));
    let mut restored = fixture();
    restored.restore(loaded.scene);
    assert_eq!(restored.mask_filter, Some(filter));
}

#[test]
fn round_trips_an_imported_label_image() {
    let mut scene = fixture();
    let id = scene.import_object("original/cells");
    let statistics = SurfaceStatistics::new(
        1,
        1.0,
        1.0,
        [0.0, 0.0, 0.0],
        Bounds::new([0.0, 0.0, 0.0], [1.0, 1.0, 0.0]),
    );
    scene.install(
        id,
        0,
        ObjectFrame::Surfaces(SurfaceSet::new(
            0,
            triangle(),
            vec![statistics],
            intensities(1, 1),
        )),
    );
    let view = ViewState {
        camera: CameraState {
            pivot: [0.0, 0.0, 0.0],
            yaw: 0.0,
            pitch: 0.0,
            distance: 10.0,
            fov_y: 0.6,
        },
        section: SectionState {
            axis: Axis::Z,
            span: 20.0,
        },
        split: 0.5,
        timeline: timeline(),
        export: Export::NATIVE,
    };
    let project = Project::capture(
        &scene,
        &Measure::new(),
        Source::Synthetic {
            name: "fixture".to_string(),
        },
        view,
    );
    assert_eq!(
        project.scene.sets[0].provenance.label(),
        Some("original/cells")
    );
    let path = std::env::temp_dir().join(format!("grana-label-{}.grana", std::process::id()));
    project
        .write(&path, &AtomicBool::new(false), &mut |_, _| {})
        .expect("the project writes");
    let loaded = Project::read(
        &path,
        GeometryStore::temporary(1).expect("the store opens a spill file"),
        &AtomicBool::new(false),
        &mut |_, _| {},
    )
    .expect("the project reads");
    fs::remove_file(&path).ok();
    assert_eq!(loaded, project);
    let mut restored = fixture();
    restored.restore(loaded.scene);
    let set = restored
        .objects
        .get(id)
        .expect("the imported object set returns");
    assert_eq!(set.provenance().label(), Some("original/cells"));
    assert_eq!(set.kind(), ObjectKind::Surfaces);
    assert_eq!(
        set.frame(0).map(ObjectFrame::object_count),
        Some(1),
        "the imported frame returns"
    );
}

#[test]
fn round_trips_the_manual_edits_of_a_project() {
    let mut scene = fixture();
    stage(&mut scene);
    let spots = scene
        .objects
        .iter()
        .find(|set| set.kind() == ObjectKind::Spots)
        .expect("the fixture stages spots")
        .id();
    assert!(
        scene
            .delete_objects(spots, 0, &[1])
            .expect("the spot leaves its frame")
    );
    let curation = scene
        .objects
        .get(spots)
        .expect("the spots exist")
        .curation()
        .clone();
    assert_eq!(curation.removal_count(), 1);
    let project = Project::capture(
        &scene,
        &Measure::new(),
        Source::Synthetic {
            name: "fixture".to_string(),
        },
        ViewState {
            camera: CameraState {
                pivot: [0.0; 3],
                yaw: 0.0,
                pitch: 0.0,
                distance: 1.0,
                fov_y: 0.6,
            },
            section: SectionState {
                axis: Axis::Z,
                span: 1.0,
            },
            split: 0.5,
            timeline: Timeline::default(),
            export: Export::NATIVE,
        },
    );
    let path = std::env::temp_dir().join(format!("grana-curation-{}.grana", std::process::id()));
    project
        .write(&path, &AtomicBool::new(false), &mut |_, _| {})
        .expect("the project writes");
    let loaded = Project::read(
        &path,
        GeometryStore::memory(),
        &AtomicBool::new(false),
        &mut |_, _| {},
    )
    .expect("the project reads");
    fs::remove_file(&path).ok();
    assert_eq!(loaded, project);
    let mut restored = fixture();
    restored.restore(loaded.scene);
    assert_eq!(
        restored
            .objects
            .get(spots)
            .expect("the spots exist")
            .curation(),
        &curation
    );
}

#[test]
fn round_trips_object_set_order() {
    let mut scene = fixture();
    stage(&mut scene);
    let ids = scene.objects.iter().map(|set| set.id()).collect::<Vec<_>>();
    let moved = *ids.last().expect("the fixture stages object sets");
    assert!(scene.reorder_object(moved, 0));
    let order = scene.objects.iter().map(|set| set.id()).collect::<Vec<_>>();
    assert_eq!(order[0], moved);
    let project = Project::capture(
        &scene,
        &Measure::new(),
        Source::Synthetic {
            name: "fixture".to_string(),
        },
        ViewState {
            camera: CameraState {
                pivot: [0.0; 3],
                yaw: 0.0,
                pitch: 0.0,
                distance: 1.0,
                fov_y: 0.6,
            },
            section: SectionState {
                axis: Axis::Z,
                span: 1.0,
            },
            split: 0.5,
            timeline: Timeline::default(),
            export: Export::NATIVE,
        },
    );
    let path = std::env::temp_dir().join(format!("grana-order-{}.grana", std::process::id()));
    project
        .write(&path, &AtomicBool::new(false), &mut |_, _| {})
        .expect("the project writes");
    let loaded = Project::read(
        &path,
        GeometryStore::memory(),
        &AtomicBool::new(false),
        &mut |_, _| {},
    )
    .expect("the project reads");
    fs::remove_file(&path).ok();
    let mut restored = fixture();
    restored.restore(loaded.scene);
    assert_eq!(
        restored
            .objects
            .iter()
            .map(|set| set.id())
            .collect::<Vec<_>>(),
        order
    );
}

#[test]
fn round_trips_the_provenance_of_derived_sets() {
    let mut scene = fixture();
    stage(&mut scene);
    let spots = scene
        .objects
        .iter()
        .find(|set| set.kind() == ObjectKind::Spots)
        .expect("the fixture stages spots")
        .id();
    let criteria = Criteria::new(
        grana_measure::Group::Object,
        vec![FilterRange::new("diameter_um", 0.5, 2.0)],
        None,
    );
    let filtered = scene
        .filter_object(spots, "filtered", criteria)
        .expect("the spots filter into a derived set");
    let copy = scene.copy_object(spots).expect("the spots copy");
    scene
        .merge_objects(copy, spots)
        .expect("the copied spots merge back");

    let project = Project::capture(
        &scene,
        &Measure::new(),
        Source::Synthetic {
            name: "fixture".to_string(),
        },
        ViewState {
            camera: CameraState {
                pivot: [0.0; 3],
                yaw: 0.0,
                pitch: 0.0,
                distance: 1.0,
                fov_y: 0.6,
            },
            section: SectionState {
                axis: Axis::Z,
                span: 1.0,
            },
            split: 0.5,
            timeline: Timeline::default(),
            export: Export::NATIVE,
        },
    );
    let path = std::env::temp_dir().join(format!("grana-derived-{}.grana", std::process::id()));
    project
        .write(&path, &AtomicBool::new(false), &mut |_, _| {})
        .expect("the project writes");
    let loaded = Project::read(
        &path,
        GeometryStore::memory(),
        &AtomicBool::new(false),
        &mut |_, _| {},
    )
    .expect("the project reads");
    fs::remove_file(&path).ok();
    assert_eq!(loaded, project);
    let mut restored = fixture();
    restored.restore(loaded.scene);
    let filtered = restored
        .objects
        .get(filtered)
        .expect("the derived set restores");
    assert!(filtered.provenance().depends_on(spots));
    let merged = restored
        .objects
        .get(spots)
        .expect("the merged set restores");
    assert!(merged.provenance().is_merged());
    assert_eq!(merged.provenance().parts().len(), 2);
}

#[test]
fn rejects_damaged_projects() {
    let mut scene = fixture();
    stage(&mut scene);
    let project = Project::capture(
        &scene,
        &Measure::new(),
        Source::Synthetic {
            name: "fixture".to_string(),
        },
        ViewState {
            camera: CameraState {
                pivot: [0.0; 3],
                yaw: 0.0,
                pitch: 0.0,
                distance: 1.0,
                fov_y: 0.6,
            },
            section: SectionState {
                axis: Axis::Z,
                span: 1.0,
            },
            split: 0.5,
            timeline: Timeline::default(),
            export: Export::NATIVE,
        },
    );
    let path = std::env::temp_dir().join(format!("grana-damaged-{}.grana", std::process::id()));
    project
        .write(&path, &AtomicBool::new(false), &mut |_, _| {})
        .expect("the project writes");
    let bytes = fs::read(&path).expect("the project reads");
    fs::write(&path, b"not a grana project").expect("the damaged file writes");
    assert!(
        Project::read(
            &path,
            GeometryStore::memory(),
            &AtomicBool::new(false),
            &mut |_, _| {}
        )
        .is_err()
    );
    fs::write(&path, &bytes[..bytes.len() / 2]).expect("the truncated file writes");
    assert!(
        Project::read(
            &path,
            GeometryStore::memory(),
            &AtomicBool::new(false),
            &mut |_, _| {}
        )
        .is_err()
    );
    fs::remove_file(&path).ok();
}

fn timeline() -> Timeline {
    let mut timeline = Timeline::new(24, 6.0);
    timeline.insert(Keyframe {
        at: 0.0,
        time: 0,
        camera: CameraState {
            pivot: [0.5, 0.0, -1.0],
            yaw: 0.2,
            pitch: 0.1,
            distance: 9.0,
            fov_y: 0.7,
        },
        section: SectionState {
            axis: Axis::Z,
            span: 4.0,
        },
    });
    timeline.insert(Keyframe {
        at: 6.0,
        time: 1,
        camera: CameraState {
            pivot: [0.0, 1.0, 0.0],
            yaw: 3.6,
            pitch: -0.4,
            distance: 4.0,
            fov_y: 0.7,
        },
        section: SectionState {
            axis: Axis::X,
            span: 2.5,
        },
    });
    timeline
}

fn fixture() -> Scene {
    let spacing = Spacing::new(0.5, 0.5, 2.0);
    let frames = (0..2)
        .map(|frame| {
            let value = frame as u16 + 1;
            Volume::new(
                EXTENT,
                spacing,
                vec![
                    Channel::new(
                        "nuclei",
                        [0.3, 0.6, 1.0],
                        VoxelData::U16(vec![value; EXTENT.voxel_count() as usize]),
                    ),
                    Channel::new(
                        "signal",
                        [1.0, 0.4, 0.2],
                        VoxelData::F32(vec![0.5; EXTENT.voxel_count() as usize]),
                    ),
                ],
            )
        })
        .collect();
    let movie = VolumeSeries::new(frames, 1.5);
    Scene::new(Dataset::synthetic("fixture", movie.pyramid().clone()))
}

fn stage(scene: &mut Scene) {
    scene.set_drift(Drift::new(1, 0, vec![[0.0, 0.0, 0.0], [2.5, -1.25, 0.0]]));
    let spots = scene.add_object(ObjectKind::Spots, 0);
    let surfaces = scene.add_object(ObjectKind::Surfaces, 1);
    let filaments = scene.add_object(ObjectKind::Filaments, 0);
    for time in 0..2 {
        scene.install(
            spots,
            time,
            ObjectFrame::Spots(SpotSet::new(
                0,
                vec![
                    Spot::new([time as f32, 0.0, 0.0], 1.5, 0.4),
                    Spot::new([2.0, time as f32, 1.0], 0.75, 0.25),
                ],
                intensities(2, 2),
            )),
        );
        scene.install(
            surfaces,
            time,
            ObjectFrame::Surfaces(SurfaceSet::new(
                0,
                triangle(),
                vec![SurfaceStatistics::new(
                    4,
                    3.0,
                    6.0,
                    [0.0, 0.0, 0.0],
                    Bounds::new([-1.0, -1.0, -1.0], [1.0, 1.0, 1.0]),
                )],
                intensities(2, 1),
            )),
        );
        scene.install(
            filaments,
            time,
            ObjectFrame::Filaments(FilamentSet::new(
                0,
                triangle(),
                vec![FilamentStatistics::new(
                    2.0,
                    0.6,
                    1.2,
                    FilamentTopology::new(1, 2, 0, 3),
                    [0.5, 0.5, 0.5],
                    Bounds::new([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]),
                )],
                intensities(2, 1),
            )),
        );
    }
    scene.add_relation(spots, surfaces, -1);
    for time in 0..2 {
        scene.install_relation(
            spots,
            surfaces,
            -1,
            time,
            RelationFrame::new(
                vec![0.5 + time as f32, 1.5],
                vec![1.0, 0.0],
                vec![2.5 + time as f32, 0.25],
                vec![1, 1],
            ),
        );
    }
    let node = |time: u32, x: f32| TrackNode::new(time, time as usize, [x, 0.0, 0.0]);
    scene.install_tracks(
        spots,
        TrackSet::new(
            1.5,
            vec![Track::new(None, vec![node(0, 0.0), node(1, 1.0)])],
        ),
    );
    let bins = 64;
    let mut frame = ColocFrame::new(bins);
    frame.push(0.25, 0.75);
    frame.push(0.5, 0.5);
    let mut joint = vec![0; bins * bins];
    joint[5 + bins * 7] = 2;
    let series = ColocSeries::from_parts(
        ColocPair::new(0, 1),
        ColocSeriesInfo::new(0, 1, 1, None),
        bins,
        vec![frame, ColocFrame::new(bins)],
        joint,
        Some(ColocSignificance::new(0.004975, 200, 1.0)),
        ColocCost::new(2, 1024, 1, 200, 8),
    );
    scene.coloc.install(series);
    let spots = scene.objects.get_mut(spots).expect("the spots exist");
    spots.set_color([0.1, 0.2, 0.3]);
    spots.set_opacity(0.35);
    spots.set_params(ObjectParams::Spots(SpotParams::new(3.0, 0.3)));
    let mut coloring = spots.object_coloring().clone();
    coloring.set_metric(Some(ObjectColor::Relation {
        target: surfaces,
        field: RelationField::Distance,
        shift: -1,
    }));
    coloring.set_palette(Palette::Inferno);
    coloring.set_reversed(true);
    coloring.set_manual(Some((0.2, 0.8)));
    let mut track_coloring = spots.track_coloring().clone();
    track_coloring.set_metric(Some(TrackColor::Relation {
        target: surfaces,
        field: RelationField::Inside,
        aggregate: Aggregate::Maximum,
        shift: -1,
    }));
    track_coloring.set_palette(Palette::Plasma);
    spots.set_track_coloring(track_coloring);
    let spot_set = spots.id();
    scene.objects.set_object_coloring(spot_set, coloring);
    scene.set_track_thickness(spot_set, 2.0);
    let nodes = vec![
        TrackNode::new(0, 0, [0.0, 0.0, 0.0]),
        TrackNode::new(1, 0, [1.0, 0.0, 0.0]),
    ];
    let children = vec![
        TrackNode::new(2, 0, [5.0, 0.0, 0.0]),
        TrackNode::new(3, 0, [6.0, 0.0, 0.0]),
    ];
    let siblings = vec![
        TrackNode::new(2, 1, [4.0, 0.0, 0.0]),
        TrackNode::new(3, 1, [4.5, 0.0, 0.0]),
    ];
    scene.install_tracks(
        surfaces,
        TrackSet::new(
            1.5,
            vec![
                Track::new(None, nodes),
                Track::new(Some(0), children),
                Track::new(Some(0), siblings),
            ],
        ),
    );
    let surfaces = scene.objects.get_mut(surfaces).expect("the surfaces exist");
    let mut coloring = surfaces.object_coloring().clone();
    coloring.set_metric(Some(ObjectColor::Measurement(Measurement::Volume)));
    coloring.set_palette(Palette::Magma);
    let surface_set = surfaces.id();
    surfaces.set_visible(false);
    scene.objects.set_object_coloring(surface_set, coloring);
    scene.channels[1].opacity = 0.35;
    scene.channels[0].enabled = false;
    scene.set_window(0, 100.0, 200.0);
    scene.set_transfer(1, TransferFunction::grayscale());
    scene.select_channel(1);
    scene.shading.diffuse = 0.5;
    scene.step_scale = 0.5;
    scene.region.set_enabled(true);
    scene
        .region
        .set_bounds(Vec3::new(-1.0, -1.0, -1.0), Vec3::new(1.5, 1.5, 1.5));
    scene.set_clip(Clip::new(Vec3::new(1.0, 0.5, 0.25), 0.5));
    scene.set_time(1);
    scene.select(Some(Selection {
        set: spot_set,
        item: Item::Object(1),
    }));
}

#[test]
fn round_trips_the_surface_topology() {
    let volume = lobes();
    let params = lobe_params(0.7);
    let set = detect(&volume, params);
    assert_eq!(set.object_count(), 1, "the shallow split keeps the lobes");
    assert!(set.cuttable(), "a detection carries its topology");
    let mut scene = surface_scene(&volume);
    let surfaces = scene.add_object(ObjectKind::Surfaces, 0);
    scene.install(surfaces, 0, ObjectFrame::Surfaces(set));
    scene
        .objects
        .get_mut(surfaces)
        .expect("the surfaces exist")
        .set_params(ObjectParams::Surfaces(params));
    let project = Project::capture(
        &scene,
        &Measure::new(),
        Source::Synthetic {
            name: "lobes".to_string(),
        },
        ViewState {
            camera: CameraState {
                pivot: [0.0; 3],
                yaw: 0.0,
                pitch: 0.0,
                distance: 10.0,
                fov_y: 0.6,
            },
            section: SectionState {
                axis: Axis::Z,
                span: 10.0,
            },
            split: 0.5,
            timeline: Timeline::default(),
            export: Export::NATIVE,
        },
    );
    let path = std::env::temp_dir().join(format!("grana-topology-{}.grana", std::process::id()));
    let mut written = Vec::new();
    project
        .write(&path, &AtomicBool::new(false), &mut |done, total| {
            written.push((done, total))
        })
        .expect("the project writes");
    let mut opened = Vec::new();
    let loaded = Project::read(
        &path,
        GeometryStore::memory(),
        &AtomicBool::new(false),
        &mut |done, total| opened.push((done, total)),
    )
    .expect("the project reads");
    fs::remove_file(&path).ok();
    assert_eq!(
        opened.last().map(|step| step.0),
        written.last().map(|step| step.1),
        "the opened project replays every step of the save"
    );
    assert_eq!(loaded, project);
    let restored = loaded
        .scene
        .sets
        .iter()
        .find(|set| set.kind == ObjectKind::Surfaces)
        .and_then(|set| set.frames[0].as_deref())
        .expect("the project carries its surface frame");
    let ObjectFrame::Surfaces(restored) = restored else {
        panic!("the surface frame restores as surfaces");
    };
    assert!(
        restored.cuttable(),
        "the restored surface keeps its watershed topology"
    );
    let deeper = lobe_params(0.3);
    let cut = recut(restored, deeper.cut(), 2, None)
        .expect("the restored topology is cut")
        .expect("the restored surface carries its topology");
    assert_eq!(
        cut.object_count(),
        2,
        "the restored topology splits the lobes without reading the volume"
    );
    assert_eq!(
        cut,
        detect(&volume, deeper),
        "the restored recut matches a fresh detection"
    );
}

#[test]
fn round_trips_the_objects_a_user_deleted() {
    let volume = lobes();
    let params = lobe_params(0.3);
    let set = detect(&volume, params);
    assert_eq!(set.object_count(), 2, "the deep split separates the lobes");
    let pruned = set
        .without(0)
        .expect("the packed mesh prunes its object")
        .expect("one lobe remains");
    assert_eq!(pruned.object_count(), 1);
    assert!(
        pruned.cuttable(),
        "a hand deleted object keeps the watershed topology"
    );
    assert_eq!(pruned.removed(), &set.leaves()[0][..]);
    let mut scene = surface_scene(&volume);
    let surfaces = scene.add_object(ObjectKind::Surfaces, 0);
    scene.install(surfaces, 0, ObjectFrame::Surfaces(pruned.clone()));
    scene
        .objects
        .get_mut(surfaces)
        .expect("the surfaces exist")
        .set_params(ObjectParams::Surfaces(params));
    let project = Project::capture(
        &scene,
        &Measure::new(),
        Source::Synthetic {
            name: "lobes".to_string(),
        },
        ViewState {
            camera: CameraState {
                pivot: [0.0; 3],
                yaw: 0.0,
                pitch: 0.0,
                distance: 10.0,
                fov_y: 0.6,
            },
            section: SectionState {
                axis: Axis::Z,
                span: 10.0,
            },
            split: 0.5,
            timeline: Timeline::default(),
            export: Export::NATIVE,
        },
    );
    let path = std::env::temp_dir().join(format!("grana-deleted-{}.grana", std::process::id()));
    project
        .write(&path, &AtomicBool::new(false), &mut |_done, _total| {})
        .expect("the project writes");
    let loaded = Project::read(
        &path,
        GeometryStore::memory(),
        &AtomicBool::new(false),
        &mut |_done, _total| {},
    )
    .expect("the project reads");
    fs::remove_file(&path).ok();
    let restored = loaded
        .scene
        .sets
        .iter()
        .find(|set| set.kind == ObjectKind::Surfaces)
        .and_then(|set| set.frames[0].as_deref())
        .expect("the project carries its surface frame");
    let ObjectFrame::Surfaces(restored) = restored else {
        panic!("the surface frame restores as surfaces");
    };
    assert_eq!(restored, &pruned, "the deleted lobes travel with the set");
    let joined = lobe_params(0.7);
    assert_eq!(
        detect(&volume, joined).object_count(),
        1,
        "the shallow split joins the lobes"
    );
    let cut = recut(restored, joined.cut(), 2, None)
        .expect("the restored topology is cut")
        .expect("the restored surface carries its topology");
    assert_eq!(
        cut.object_count(),
        1,
        "the deleted lobe never merges back into the kept lobe"
    );
}

#[test]
fn round_trips_the_seed_source_of_a_surface() {
    let volume = lobes();
    let mut scene = surface_scene(&volume);
    let nuclei = scene.add_object(ObjectKind::Surfaces, 0);
    let cells = scene.add_object(ObjectKind::Surfaces, 0);
    assert!(scene.set_seed_source(cells, Some(nuclei)));
    let project = Project::capture(
        &scene,
        &Measure::new(),
        Source::Synthetic {
            name: "lobes".to_string(),
        },
        ViewState {
            camera: CameraState {
                pivot: [0.0; 3],
                yaw: 0.0,
                pitch: 0.0,
                distance: 10.0,
                fov_y: 0.6,
            },
            section: SectionState {
                axis: Axis::Z,
                span: 10.0,
            },
            split: 0.5,
            timeline: Timeline::default(),
            export: Export::NATIVE,
        },
    );
    let path = std::env::temp_dir().join(format!("grana-seeded-{}.grana", std::process::id()));
    project
        .write(&path, &AtomicBool::new(false), &mut |_done, _total| {})
        .expect("the project writes");
    let loaded = Project::read(
        &path,
        GeometryStore::memory(),
        &AtomicBool::new(false),
        &mut |_done, _total| {},
    )
    .expect("the project reads");
    fs::remove_file(&path).ok();
    assert_eq!(loaded, project, "the seeded surface travels with its set");
    let mut restored = surface_scene(&volume);
    restored.restore(loaded.scene);
    assert_eq!(restored.seed_source(cells), Some(nuclei));
    assert_eq!(
        restored.seed_frames(cells).len(),
        restored.frame_count() as usize
    );
}

#[test]
fn round_trips_a_child_color() {
    let mut scene = fixture();
    stage(&mut scene);
    let spots = scene
        .objects
        .iter()
        .find(|set| set.kind() == ObjectKind::Spots)
        .map(|set| set.id())
        .expect("the spots exist");
    let surfaces = scene
        .objects
        .iter()
        .find(|set| set.kind() == ObjectKind::Surfaces && set.channel() == 1)
        .map(|set| set.id())
        .expect("the surfaces exist");
    assert!(scene.add_relation(spots, surfaces, 0));
    for time in 0..2 {
        scene.install_relation(
            spots,
            surfaces,
            0,
            time,
            RelationFrame::new(vec![0.0, 0.0], vec![1.0, 1.0], vec![0.0, 0.0], vec![1, 1]),
        );
    }
    let count = ChildField::Count;
    let diameter = ChildField::Stat {
        metric: ChildMetric::Diameter,
        statistic: Statistic::Mean,
    };
    let mut coloring = scene
        .objects
        .get(surfaces)
        .expect("the surfaces exist")
        .object_coloring()
        .clone();
    coloring.set_metric(Some(ObjectColor::Child {
        target: spots,
        field: count,
    }));
    assert!(scene.objects.set_object_coloring(surfaces, coloring));
    assert!(scene.set_label_metric(
        surfaces,
        Some(ObjectColor::Child {
            target: spots,
            field: diameter,
        })
    ));
    let project = Project::capture(
        &scene,
        &Measure::new(),
        Source::Synthetic {
            name: "fixture".to_string(),
        },
        ViewState {
            camera: CameraState {
                pivot: [0.0; 3],
                yaw: 0.0,
                pitch: 0.0,
                distance: 10.0,
                fov_y: 0.6,
            },
            section: SectionState {
                axis: Axis::Z,
                span: 10.0,
            },
            split: 0.5,
            timeline: Timeline::default(),
            export: Export::NATIVE,
        },
    );
    let path = std::env::temp_dir().join(format!("grana-child-{}.grana", std::process::id()));
    project
        .write(&path, &AtomicBool::new(false), &mut |_done, _total| {})
        .expect("the project writes");
    let loaded = Project::read(
        &path,
        GeometryStore::memory(),
        &AtomicBool::new(false),
        &mut |_done, _total| {},
    )
    .expect("the project reads");
    fs::remove_file(&path).ok();
    assert_eq!(loaded, project, "the child colors travel with their set");
    let mut restored = fixture();
    restored.restore(loaded.scene);
    let surfaces = restored.objects.get(surfaces).expect("the surfaces exist");
    assert_eq!(
        surfaces.object_coloring().metric(),
        Some(ObjectColor::Child {
            target: spots,
            field: count,
        })
    );
    assert_eq!(
        surfaces.label_metric(),
        Some(ObjectColor::Child {
            target: spots,
            field: diameter,
        })
    );
    assert_eq!(surfaces.object_extent(), Some((2.0, 2.0)));
}

fn lobes() -> Volume {
    let extent = Extent::new(200, 80, 80);
    let centres = [[80.0f32, 40.0, 40.0], [118.0, 40.0, 40.0]];
    let mut values = vec![0u16; extent.voxel_count() as usize];
    for z in 0..extent.z() {
        for y in 0..extent.y() {
            for x in 0..extent.x() {
                let inside = centres.iter().any(|centre| {
                    ((x as f32 - centre[0]).powi(2)
                        + (y as f32 - centre[1]).powi(2)
                        + (z as f32 - centre[2]).powi(2))
                    .sqrt()
                        < 22.0
                });
                if inside {
                    values[extent.index(x, y, z) as usize] = u16::MAX;
                }
            }
        }
    }
    Volume::new(
        extent,
        Spacing::new(0.1, 0.1, 0.1),
        vec![Channel::new(
            "lobes",
            [0.3, 0.8, 1.0],
            VoxelData::U16(values),
        )],
    )
}

fn lobe_params(split: f32) -> SurfaceParams {
    SurfaceParams::new(0.5, 0.0, split, 0.0, SurfaceFilter::new(0.0, 0.8, 0.0, 0.0))
}

fn surface_scene(volume: &Volume) -> Scene {
    let series = VolumeSeries::new(vec![volume.clone()], 1.0);
    Scene::new(Dataset::synthetic("lobes", series.pyramid().clone()))
}

fn detect(volume: &Volume, params: SurfaceParams) -> SurfaceSet {
    let level = grana_volume::Level::new(volume.extent(), volume.spacing());
    let intensity = grana_volume::Intensity::new(0.0, f32::from(u16::MAX));
    let channels = volume.channel_count();
    let builder = SurfaceBuilder::new(
        0,
        level,
        params,
        channels,
        0,
        grana_volume::Region::none(),
        intensity,
    );
    let cancel = AtomicBool::new(false);
    builder
        .scan(
            2,
            &cancel,
            None,
            &mut |_done, _total| {},
            Sources::new(
                &|ranges| {
                    Ok(volume.channels()[0].data().region(
                        volume.extent(),
                        ranges[0].clone(),
                        ranges[1].clone(),
                        ranges[2].clone(),
                    ))
                },
                &|channel, core| {
                    Ok(volume.channels()[channel].data().region(
                        volume.extent(),
                        core[0].clone(),
                        core[1].clone(),
                        core[2].clone(),
                    ))
                },
                &GeometryStore::memory(),
            ),
        )
        .expect("a surface frame is detected")
        .expect("surface detection completes")
}

fn intensities(channels: usize, objects: usize) -> IntensityTable {
    let values = (0..channels * objects)
        .map(|index| statistics(&[index as f32, index as f32 + 1.0, index as f32 + 2.0]))
        .collect();
    IntensityTable::new(channels, values)
}

fn statistics(values: &[f32]) -> SampleStatistics {
    let mut accumulator = SampleAccumulator::new();
    for value in values {
        accumulator.push(*value);
    }
    accumulator.statistics()
}

fn triangle() -> Mesh {
    let vertices = [
        MeshVertex::new([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 0),
        MeshVertex::new([1.0, 0.0, 0.0], [0.0, 0.0, 1.0], 0),
        MeshVertex::new([0.0, 1.0, 0.0], [0.0, 0.0, 1.0], 0),
    ];
    Mesh::new(vertices.to_vec(), vec![0, 1, 2])
}

#[test]
fn round_trips_a_classifier() {
    let mut scene = fixture();
    stage(&mut scene);
    let spots = scene
        .objects
        .iter()
        .find(|set| set.kind() == ObjectKind::Spots)
        .expect("the fixture stages spots")
        .id();
    let table = scene
        .table(spots, grana_measure::Group::Object, Aggregate::Mean)
        .expect("the spots build a table");
    let contrast = table
        .column("contrast")
        .expect("the table measures contrast");
    let positives = (0..table.rows())
        .filter(|row| table.value(*row, contrast) > 0.3)
        .collect::<Vec<_>>();
    scene
        .train_classifier(
            spots,
            &[Measurement::Diameter, Measurement::Contrast],
            &positives,
        )
        .expect("the classifier trains on the gate");
    let mut coloring = grana_display::Coloring::new();
    coloring.set_metric(Some(ObjectColor::Score));
    assert!(scene.set_object_coloring(spots, coloring));
    let view = ViewState {
        camera: CameraState {
            pivot: [0.0, 0.0, 0.0],
            yaw: 0.0,
            pitch: 0.0,
            distance: 10.0,
            fov_y: 0.7,
        },
        section: SectionState {
            axis: Axis::Z,
            span: 8.0,
        },
        split: 0.5,
        timeline: Timeline::new(24, 1.0),
        export: Export::NATIVE,
    };
    let project = Project::capture(
        &scene,
        &Measure::new(),
        Source::Synthetic {
            name: "fixture".to_string(),
        },
        view,
    );
    let path = std::env::temp_dir().join(format!("grana-classifier-{}.grana", std::process::id()));
    project
        .write(&path, &AtomicBool::new(false), &mut |_, _| {})
        .expect("the classified project writes");
    let loaded = Project::read(
        &path,
        GeometryStore::temporary(1).expect("the store opens a spill file"),
        &AtomicBool::new(false),
        &mut |_, _| {},
    )
    .expect("the classified project reads");
    fs::remove_file(&path).ok();
    assert_eq!(loaded, project);
    let set = loaded
        .scene
        .sets
        .iter()
        .find(|set| set.id == spots.raw())
        .expect("the classified set restores");
    let classifier = set.classifier.as_ref().expect("the classifier restores");
    assert_eq!(classifier.features.len(), 2);
    assert!(classifier.accuracy > 0.99);
    assert_eq!(set.object_color.metric, Some(ObjectColor::Score));
}
