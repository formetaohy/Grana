use grana_optics::{Kernel, PsfParams};
use grana_restore::{Backend, Restorer};
use grana_volume::Spacing;

fn convolve(dims: [u32; 3], source: &[f32], kernel: &Kernel) -> Vec<f32> {
    let [width, height, depth] = dims;
    let [taps_x, taps_y, taps_z] = kernel.dims();
    let centre = [taps_x / 2, taps_y / 2, taps_z / 2];
    let mut out = vec![0.0f32; source.len()];
    for z in 0..depth {
        for y in 0..height {
            for x in 0..width {
                let mut sum = 0.0;
                for tap_z in 0..taps_z {
                    let source_z = clamp(z as i64 + tap_z as i64 - centre[2] as i64, depth);
                    for tap_y in 0..taps_y {
                        let source_y = clamp(y as i64 + tap_y as i64 - centre[1] as i64, height);
                        for tap_x in 0..taps_x {
                            let source_x = clamp(x as i64 + tap_x as i64 - centre[0] as i64, width);
                            let tap = (tap_x + taps_x * (tap_y + taps_y * tap_z)) as usize;
                            let cell =
                                source_x + width as usize * (source_y + height as usize * source_z);
                            sum += source[cell] * kernel.values()[tap];
                        }
                    }
                }
                out[(x + width * (y + height * z)) as usize] = sum;
            }
        }
    }
    out
}

fn clamp(value: i64, limit: u32) -> usize {
    value.clamp(0, limit as i64 - 1) as usize
}

fn total(values: &[f32]) -> f32 {
    values.iter().sum()
}

#[test]
fn richardson_lucy_sharpens_a_blurred_point_on_every_backend() {
    let dims = [25, 25, 11];
    let spacing = Spacing::new(0.1, 0.1, 0.3);
    let kernel = Kernel::new(PsfParams::new(1.4, 0.52, 1.515, 1.515, 0.0), spacing);
    let centre = (dims[0] / 2 + dims[0] * (dims[1] / 2 + dims[1] * (dims[2] / 2))) as usize;
    let mut point = vec![0.0f32; (dims[0] * dims[1] * dims[2]) as usize];
    point[centre] = 1.0;
    let blurred = convolve(dims, &point, &kernel);
    let processor = Restorer::processor().restore(dims, &blurred, &kernel, 12);
    assert!(
        processor[centre] > blurred[centre] * 1.5,
        "the restored point is sharper than the blurred one"
    );
    assert!(
        processor.iter().all(|value| *value >= 0.0),
        "richardson-lucy keeps every voxel non negative"
    );
    let before = total(&blurred);
    let after = total(&processor);
    assert!(
        (after - before).abs() < before * 0.1,
        "the restoration keeps {after} of {before} photons"
    );
    let graphics = Restorer::new();
    if graphics.backend() == Backend::Graphics {
        let restored = graphics.restore(dims, &blurred, &kernel, 12);
        let deviation = restored
            .iter()
            .zip(&processor)
            .map(|(left, right)| (left - right).abs())
            .fold(0.0f32, f32::max);
        assert!(
            deviation < processor[centre] * 0.01,
            "the graphics backend agrees with the processor by {deviation}"
        );
    }
}
