use std::fs;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use glam::Vec3;
use grana_analysis::SpotParams;
use grana_animation::{Keyframe, Timeline};
use grana_display::{CameraState, Palette, SectionState, TransferFunction};
use grana_geometry::MeshStore;
use grana_geometry::{Bounds, Mesh, MeshVertex};
use grana_measure::Measurement;
use grana_measure::{
    Aggregate, ColocCost, ColocFrame, ColocPair, ColocSeries, ColocSeriesInfo, ColocSignificance,
    RelationField,
};
use grana_object::{
    FilamentSet, FilamentStatistics, FilamentTopology, IntensityTable, RelationFrame,
    SampleAccumulator, SampleStatistics, Spot, SpotSet, SurfaceSet, SurfaceStatistics, Track,
    TrackNode, TrackSet,
};
use grana_project::{Export, Project, Source, ViewState};
use grana_scene::{
    Dataset, Item, Measure, ObjectColor, ObjectFrame, ObjectKind, ObjectParams, Scene, Selection,
    TrackColor, Visibility,
};
use grana_volume::{Axis, Channel, Extent, Spacing, Volume, VolumeSeries, VoxelData};

const EXTENT: Extent = Extent::new(8, 8, 4);

#[test]
fn round_trips_a_project() {
    let mut scene = fixture();
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
    scene.set_time(1);
    assert!(scene.add_seed(surfaces, [0.25, -0.5, 0.75]));
    assert!(scene.add_seed(surfaces, [-0.25, 0.5, -0.75]));
    scene.set_time(0);
    scene.select(Some(Selection {
        set: surfaces,
        item: Item::Track(2),
    }));
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
        MeshStore::temporary(1).expect("the store opens a spill file"),
        &AtomicBool::new(false),
        &mut record,
    )
    .expect("the project reads");
    assert_eq!(opened.first().map(|step| step.1), Some(total));
    assert_eq!(opened.last().map(|step| step.0), Some(total));
    fs::remove_file(&path).ok();
    assert_eq!(loaded, project);

    let mut restored = fixture();
    restored.restore(loaded.scene);
    let mut restored_measure = Measure::new();
    restored_measure.restore(loaded.measure, &restored);
    assert_eq!(restored.state(), project.scene);
    restored.set_time(1);
    assert_eq!(restored.seed_count(surfaces), 2);
    restored.set_time(0);
    assert_eq!(restored_measure.state(), project.measure);
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
        MeshStore::memory(),
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
            MeshStore::memory(),
            &AtomicBool::new(false),
            &mut |_, _| {}
        )
        .is_err()
    );
    fs::write(&path, &bytes[..bytes.len() / 2]).expect("the truncated file writes");
    assert!(
        Project::read(
            &path,
            MeshStore::memory(),
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
            time,
            RelationFrame::new(vec![0.5 + time as f32, 1.5], vec![1.0, 0.0], vec![1, 1]),
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
    }));
    coloring.set_palette(Palette::Inferno);
    coloring.set_reversed(true);
    coloring.set_manual(Some((0.2, 0.8)));
    spots.set_object_coloring(coloring);
    let mut track_coloring = spots.track_coloring().clone();
    track_coloring.set_metric(Some(TrackColor::Relation {
        target: surfaces,
        field: RelationField::Inside,
        aggregate: Aggregate::Maximum,
    }));
    track_coloring.set_palette(Palette::Plasma);
    spots.set_track_coloring(track_coloring);
    let spot_set = spots.id();
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
    surfaces.set_object_coloring(coloring);
    surfaces.set_visible(false);
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
    scene.set_time(1);
    scene.select(Some(Selection {
        set: spot_set,
        item: Item::Object(1),
    }));
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
