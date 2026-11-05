use grana_measure::Measurement;
use grana_object::{IntensityTable, SampleAccumulator, Spot, SpotSet};
use grana_scene::{Dataset, ObjectColor, ObjectFrame, ObjectKind, Scene};
use grana_volume::{Channel, Extent, Spacing, Volume, VolumeSeries, VoxelData};

#[test]
fn labels_measure_objects_and_skip_hidden_ones() {
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
    let id = scene.add_object(ObjectKind::Spots, 0);
    let mut accumulator = SampleAccumulator::new();
    accumulator.push(0.5);
    let statistics = accumulator.statistics();
    scene.install(
        id,
        0,
        ObjectFrame::Spots(SpotSet::new(
            0,
            vec![
                Spot::new([0.0, 0.0, 0.0], 1.5, 0.5),
                Spot::new([1.0, 0.0, 0.0], 2.5, 0.5),
            ],
            IntensityTable::new(1, vec![statistics; 2]),
        )),
    );
    scene
        .objects
        .get_mut(id)
        .expect("the spots exist")
        .set_labels(true);
    let numbers = grana_figure::labels(&scene, 0)
        .into_iter()
        .map(|label| label.text)
        .collect::<Vec<_>>();
    assert_eq!(numbers, ["1", "2"]);

    assert!(scene.set_label_metric(id, Some(ObjectColor::Measurement(Measurement::Diameter))));
    let diameters = grana_figure::labels(&scene, 0)
        .into_iter()
        .map(|label| label.text)
        .collect::<Vec<_>>();
    assert_eq!(diameters, ["3.000 µm", "5.000 µm"]);

    assert!(scene.set_object_hidden(id, 0, &[0], true));
    let labels = grana_figure::labels(&scene, 0);
    assert_eq!(labels.len(), 1, "a hidden object carries no label");
    assert_eq!(labels[0].text, "5.000 µm");
}
