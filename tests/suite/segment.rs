use grana_segment::{Flow, Prompt};

const SIDE: u32 = 64;
const CENTERS: [[f32; 2]; 2] = [[16.5, 16.5], [46.5, 46.5]];
const RADIUS: f32 = 8.0;

fn basin_flow() -> Flow {
    let width = SIDE as usize;
    let mut dy = vec![0f32; width * width];
    let mut dx = vec![0f32; width * width];
    let mut cellprob = vec![0f32; width * width];
    for y in 0..width {
        for x in 0..width {
            let point = [x as f32 + 0.5, y as f32 + 0.5];
            let (direction, distance) = CENTERS
                .iter()
                .map(|center| {
                    let offset = [center[0] - point[0], center[1] - point[1]];
                    let distance = offset[0].hypot(offset[1]);
                    (offset, distance)
                })
                .fold(([0.0f32; 2], f32::INFINITY), |best, candidate| {
                    if candidate.1 < best.1 {
                        candidate
                    } else {
                        best
                    }
                });
            let index = x + width * y;
            dx[index] = direction[0] / distance.max(1.0e-6);
            dy[index] = direction[1] / distance.max(1.0e-6);
            cellprob[index] = if distance <= RADIUS { 5.0 } else { -5.0 };
        }
    }
    Flow::from_parts(SIDE, SIDE, dy, dx, cellprob)
}

#[test]
fn assembles_slices_by_overlap() {
    let width = 4u32;
    let height = 2u32;
    let mut first = vec![0u32; 8];
    let mut second = vec![0u32; 8];
    for index in [0, 1, 4] {
        first[index] = 1;
        second[index] = 1;
    }
    second[2] = 2;
    let volume = grana_segment::assemble(&mut [first, second], width, height, 0.5)
        .expect("the slices assemble");
    assert_eq!(volume.count(), 2, "the overlap joins one object only");
    assert_eq!(
        volume.labels()[0],
        volume.labels()[8],
        "the overlapping object keeps one label across slices"
    );
    assert_ne!(
        volume.labels()[0],
        volume.labels()[10],
        "the unmatched object keeps its own label"
    );
}

#[test]
fn normalizes_a_plane_to_its_one_and_ninety_ninth_percentiles() {
    let mut values = (0..1000).map(|index| index as f32).collect::<Vec<_>>();
    values[0] = -5.0;
    let normalized = grana_segment::normalize(&values);
    let expected = (-5.0 - 9.99) / (989.01 - 9.99);
    assert!(
        (normalized[0] - expected).abs() <= 1.0e-5,
        "the plane keeps its values below the first percentile: {} against {expected}",
        normalized[0]
    );
    assert!(
        normalized[999] > 1.0,
        "the plane keeps its values above the ninety ninth percentile: {}",
        normalized[999]
    );
    let flat = grana_segment::normalize(&[3.0; 16]);
    assert!(
        flat.iter().all(|value| *value == 0.0),
        "a plane without contrast normalizes to zeros"
    );
}

#[test]
fn assigns_flow_basins_to_their_seeds() {
    let flow = basin_flow();
    let labels = flow.seeded(Prompt::new(&CENTERS, 0.0, 6.0));
    let width = SIDE as usize;
    let at = |x: usize, y: usize| labels[x + width * y];
    assert_eq!(at(16, 16), 1, "the first seed marks another basin");
    assert_eq!(at(46, 46), 2, "the second seed marks another basin");
    assert_eq!(at(1, 1), 0, "the background belongs to no seed");
    let first = labels.iter().filter(|label| **label == 1).count();
    let second = labels.iter().filter(|label| **label == 2).count();
    assert!(
        first > 100 && second > 100,
        "the seeded basins are too small: {first} and {second}"
    );
    assert_eq!(first, second, "the symmetric basins keep their areas");
    let automatic = flow.masks(0.0);
    assert!(
        automatic.contains(&2),
        "the automatic pipeline finds both basins"
    );
}
