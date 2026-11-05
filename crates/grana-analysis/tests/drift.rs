use grana_analysis::drift_shift;

const DIMS: [usize; 3] = [32, 32, 16];

fn texture(state: &mut u64) -> f32 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    ((*state >> 33) as f32) / ((u32::MAX >> 1) as f32) - 0.5
}

fn blobs() -> Vec<f32> {
    let mut values = vec![0.0f32; DIMS[0] * DIMS[1] * DIMS[2]];
    let mut state = 0x5eed_1234_u64;
    for (center, sigma, weight) in [
        ([10.0f32, 9.0, 6.0], 2.5f32, 1.0f32),
        ([20.0, 20.0, 10.0], 3.5, 0.7),
        ([15.0, 22.0, 12.0], 2.2, 1.4),
    ] {
        for z in 0..DIMS[2] {
            for y in 0..DIMS[1] {
                for x in 0..DIMS[0] {
                    let dx = x as f32 - center[0];
                    let dy = y as f32 - center[1];
                    let dz = z as f32 - center[2];
                    let squared = dx * dx + dy * dy + dz * dz;
                    let envelope = weight * (-squared / (2.0 * sigma * sigma)).exp();
                    values[x + DIMS[0] * (y + DIMS[1] * z)] +=
                        envelope * (0.6 + 0.8 * texture(&mut state));
                }
            }
        }
    }
    values
}

fn sample(source: &[f32], at: [f32; 3]) -> f32 {
    let low = at.map(|value| value.floor() as i64);
    let fraction = [0, 1, 2].map(|axis| at[axis] - low[axis] as f32);
    let mut sum = 0.0;
    for corner in 0..8 {
        let offset = [0, 1, 2].map(|axis| (corner >> axis) & 1);
        let weight = (0..3)
            .map(|axis| {
                if offset[axis] == 1 {
                    fraction[axis]
                } else {
                    1.0 - fraction[axis]
                }
            })
            .product::<f32>();
        let at = [0, 1, 2].map(|axis| low[axis] + offset[axis]);
        if at
            .iter()
            .enumerate()
            .any(|(axis, value)| *value < 0 || *value >= DIMS[axis] as i64)
        {
            continue;
        }
        sum +=
            weight * source[at[0] as usize + DIMS[0] * (at[1] as usize + DIMS[1] * at[2] as usize)];
    }
    sum
}

fn moved(source: &[f32], offset: [f32; 3]) -> Vec<f32> {
    let mut values = vec![0.0f32; source.len()];
    for z in 0..DIMS[2] {
        for y in 0..DIMS[1] {
            for x in 0..DIMS[0] {
                let at = [
                    x as f32 - offset[0],
                    y as f32 - offset[1],
                    z as f32 - offset[2],
                ];
                values[x + DIMS[0] * (y + DIMS[1] * z)] = sample(source, at);
            }
        }
    }
    values
}

#[test]
fn recovers_the_translation_of_a_moved_volume() {
    let reference = blobs();
    for offset in [
        [3.0, -2.0, 1.0],
        [-4.0, 5.0, -2.0],
        [0.0, 0.0, 0.0],
        [2.5, -1.25, 0.5],
        [0.0, 3.75, -1.5],
    ] {
        let moving = moved(&reference, offset);
        let found = drift_shift(&reference, &moving, DIMS);
        for axis in 0..3 {
            assert!(
                (found[axis] - offset[axis]).abs() < 0.2,
                "the estimator recovers {offset:?} but reported {found:?}"
            );
        }
    }
}

#[test]
fn reports_no_motion_for_a_uniform_volume() {
    let values = vec![4.0f32; DIMS[0] * DIMS[1] * DIMS[2]];
    assert_eq!(drift_shift(&values, &values, DIMS), [0.0; 3]);
}
