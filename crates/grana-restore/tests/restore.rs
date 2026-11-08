use grana_optics::{KernelBank, PsfParams};
use grana_restore::{Backend, Restorer};
use grana_volume::Spacing;

fn convolve(dims: [u32; 3], source: &[f32], taps: &[f32], kernel: [u32; 3]) -> Vec<f32> {
    let [width, height, depth] = dims;
    let [taps_x, taps_y, taps_z] = kernel;
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
                            sum += source[cell] * taps[tap];
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
    let params = PsfParams::new(1.4, 0.52, 1.515, 1.515, 0.0);
    let bank = KernelBank::new(params, spacing, dims[2]);
    assert_eq!(
        bank.layers(),
        1,
        "a thin stack restores one point spread function"
    );
    let centre = (dims[0] / 2 + dims[0] * (dims[1] / 2 + dims[1] * (dims[2] / 2))) as usize;
    let mut point = vec![0.0f32; (dims[0] * dims[1] * dims[2]) as usize];
    point[centre] = 1.0;
    let blurred = convolve(dims, &point, bank.taps(0), bank.dims());
    let processor = Restorer::processor().restore(dims, &blurred, &bank, 0, 12);
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
        let restored = graphics.restore(dims, &blurred, &bank, 0, 12);
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

#[test]
fn a_layered_bank_sharpens_a_point_at_every_depth() {
    let dims = [21, 21, 17];
    let spacing = Spacing::new(0.1, 0.1, 0.3);
    let params = PsfParams::new(1.4, 0.52, 1.515, 1.515, 2.0);
    let bank = KernelBank::new(params, spacing, dims[2]);
    assert_eq!(bank.layers(), 2, "a 4.8 µm stack samples two depths");
    let voxels = (dims[0] * dims[1] * dims[2]) as usize;
    let mut blurred = vec![0.0f32; voxels];
    let points = [
        [dims[0] / 2, dims[1] / 2, 4],
        [dims[0] / 2, dims[1] / 2, 14],
    ];
    for point in points {
        let mut impulse = vec![0.0f32; voxels];
        let index = dims[0] * (dims[1] * point[2] + point[1]) + point[0];
        impulse[index as usize] = 1.0;
        let layer = bank.layer(0, point[2]);
        let blurred_point = convolve(dims, &impulse, bank.taps(layer), bank.dims());
        for (slot, value) in blurred.iter_mut().zip(blurred_point) {
            *slot += value;
        }
    }
    let restored = Restorer::processor().restore(dims, &blurred, &bank, 0, 16);
    for point in points {
        let index = dims[0] * (dims[1] * point[2] + point[1]) + point[0];
        let index = index as usize;
        assert!(
            restored[index] > blurred[index] * 1.3,
            "the point at {point:?} sharpens: {} against {}",
            restored[index],
            blurred[index]
        );
    }
    let before = total(&blurred);
    let after = total(&restored);
    assert!(
        (after - before).abs() < before * 0.2,
        "the restoration keeps {after} of {before} photons"
    );
    let graphics = Restorer::new();
    if graphics.backend() == Backend::Graphics {
        let layered = graphics.restore(dims, &blurred, &bank, 0, 16);
        let deviation = layered
            .iter()
            .zip(&restored)
            .map(|(left, right)| (left - right).abs())
            .fold(0.0f32, f32::max);
        let width = dims[0] as usize;
        let height = dims[1] as usize;
        let peak = restored[width / 2 + width * (height / 2 + height * 14)];
        assert!(
            deviation < peak * 0.01,
            "the graphics backend samples every layer by {deviation}"
        );
    }
}
