use grana_optics::{Kernel, KernelBank};
use grana_restore::{Backend, Job, Restorer};

use crate::support;

const DIMS: [u32; 3] = [17, 13, 11];
const KERNEL: [u32; 3] = [9, 5, 7];

#[test]
fn graphics_restore_matches_the_processor() {
    let Some(_gpu) = support::gpu() else {
        return;
    };
    let graphics = Restorer::shared();
    if graphics.backend() != Backend::Graphics {
        return;
    }
    let bank = KernelBank::from_kernels(&[kernel(1.0), kernel(0.7)], DIMS[2]);
    let source = source();
    let job = Job {
        dims: DIMS,
        source: &source,
        first_plane: 4,
        iterations: 4,
    };
    let expected = Restorer::processor().restore_batch(&bank, std::slice::from_ref(&job));
    let actual = graphics.restore_batch(&bank, std::slice::from_ref(&job));
    let mut maximum = 0.0f32;
    let mut magnitude = 0.0f32;
    for (want, got) in expected[0].iter().zip(&actual[0]) {
        maximum = maximum.max((want - got).abs());
        magnitude = magnitude.max(want.abs());
    }
    assert!(
        maximum / magnitude.max(1.0e-9) < 5.0e-4,
        "the graphics restore diverges from the processor"
    );
}

fn kernel(seed: f32) -> Kernel {
    let mut values = vec![0.0f32; (KERNEL[0] * KERNEL[1] * KERNEL[2]) as usize];
    let centre = [KERNEL[0] / 2, KERNEL[1] / 2, KERNEL[2] / 2];
    for z in 0..KERNEL[2] {
        for y in 0..KERNEL[1] {
            for x in 0..KERNEL[0] {
                let dx = x as f32 - centre[0] as f32;
                let dy = y as f32 - centre[1] as f32;
                let dz = z as f32 - centre[2] as f32;
                let weight = (seed * (-(dx * dx + dy * dy + dz * dz) / 4.0).exp()).max(1.0e-5);
                values[(x + KERNEL[0] * (y + KERNEL[1] * z)) as usize] = weight;
            }
        }
    }
    Kernel::from_values(KERNEL, values)
}

fn source() -> Vec<f32> {
    (0..DIMS[0] * DIMS[1] * DIMS[2])
        .map(|index| {
            let value = ((index as f32 * 0.017).sin() * 0.5 + 0.5).abs();
            (value + ((index * 7) % 11) as f32 * 0.01).clamp(0.0, 1.0)
        })
        .collect()
}
