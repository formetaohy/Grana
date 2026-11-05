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

fn moved(source: &[f32], offset: [i32; 3]) -> Vec<f32> {
    let mut values = vec![0.0f32; source.len()];
    for z in 0..DIMS[2] as i32 {
        for y in 0..DIMS[1] as i32 {
            for x in 0..DIMS[0] as i32 {
                let source_at = [x - offset[0], y - offset[1], z - offset[2]];
                if source_at.iter().any(|value| *value < 0) {
                    continue;
                }
                let [sx, sy, sz] = source_at.map(|value| value as usize);
                if sx >= DIMS[0] || sy >= DIMS[1] || sz >= DIMS[2] {
                    continue;
                }
                values[x as usize + DIMS[0] * (y as usize + DIMS[1] * z as usize)] =
                    source[sx + DIMS[0] * (sy + DIMS[1] * sz)];
            }
        }
    }
    values
}

#[test]
fn recovers_an_integer_translation() {
    let reference = blobs();
    for offset in [[3, -2, 1], [-4, 5, -2], [0, 0, 0], [6, 6, 3]] {
        let moving = moved(&reference, offset);
        assert_eq!(
            drift_shift(&reference, &moving, DIMS),
            offset,
            "the estimator recovers the translation {offset:?}"
        );
    }
}

#[test]
fn reports_no_motion_for_a_uniform_volume() {
    let values = vec![4.0f32; DIMS[0] * DIMS[1] * DIMS[2]];
    assert_eq!(drift_shift(&values, &values, DIMS), [0; 3]);
}
