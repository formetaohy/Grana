use grana_optics::KernelBank;
use rayon::prelude::*;

const FLOOR: f32 = 1.0e-6;

#[derive(Clone, Copy)]
struct Grid {
    dims: [u32; 3],
    taps: [u32; 3],
    centre: [u32; 3],
}

impl Grid {
    fn of(dims: [u32; 3], taps: [u32; 3]) -> Self {
        Self {
            dims,
            taps,
            centre: [taps[0] / 2, taps[1] / 2, taps[2] / 2],
        }
    }

    fn source(&self, position: [u32; 3], tap: [u32; 3]) -> usize {
        let mut cell = [0u32; 3];
        for axis in 0..3 {
            cell[axis] = self.source_axis(axis, position[axis], tap[axis]);
        }
        (cell[0] + self.dims[0] * (cell[1] + self.dims[1] * cell[2])) as usize
    }

    fn source_axis(&self, axis: usize, position: u32, tap: u32) -> u32 {
        let value = position as i64 + tap as i64 - self.centre[axis] as i64;
        value.clamp(0, self.dims[axis] as i64 - 1) as u32
    }

    fn weight(&self, tap: [u32; 3], flipped: bool) -> usize {
        let mut weight = 0usize;
        for axis in (0..3).rev() {
            let index = if flipped {
                self.taps[axis] - 1 - tap[axis]
            } else {
                tap[axis]
            };
            weight = weight * self.taps[axis] as usize + index as usize;
        }
        weight
    }
}

pub fn restore(
    dims: [u32; 3],
    source: &[f32],
    bank: &KernelBank,
    first_plane: u32,
    iterations: u32,
) -> Vec<f32> {
    let voxels = (dims[0] * dims[1] * dims[2]) as usize;
    assert_eq!(
        source.len(),
        voxels,
        "the restored volume holds every requested voxel"
    );
    let measured = source
        .iter()
        .map(|value| {
            if value.is_finite() && *value > 0.0 {
                *value
            } else {
                0.0
            }
        })
        .collect::<Vec<f32>>();
    let mut estimate = measured.clone();
    let mut blurred = vec![0.0f32; voxels];
    let mut updated = vec![0.0f32; voxels];
    let grid = Grid::of(dims, bank.dims());
    for _ in 0..iterations {
        blur(&grid, bank, first_plane, &estimate, &mut blurred);
        correct(
            &grid,
            bank,
            first_plane,
            &measured,
            &blurred,
            &estimate,
            &mut updated,
        );
        std::mem::swap(&mut estimate, &mut updated);
    }
    estimate
}

fn blur(grid: &Grid, bank: &KernelBank, first_plane: u32, estimate: &[f32], out: &mut [f32]) {
    let [width, height, _] = grid.dims;
    out.par_chunks_mut((width * height) as usize)
        .enumerate()
        .for_each(|(z, slice)| {
            for y in 0..height {
                for x in 0..width {
                    let position = [x, y, z as u32];
                    let mut sum = 0.0f32;
                    for tap_z in 0..grid.taps[2] {
                        let source_z = grid.source_axis(2, position[2], tap_z);
                        let taps = bank.taps(bank.layer(first_plane, source_z));
                        for tap_y in 0..grid.taps[1] {
                            for tap_x in 0..grid.taps[0] {
                                let tap = [tap_x, tap_y, tap_z];
                                sum += estimate[grid.source(position, tap)]
                                    * taps[grid.weight(tap, false)];
                            }
                        }
                    }
                    slice[(x + width * y) as usize] = sum;
                }
            }
        });
}

fn correct(
    grid: &Grid,
    bank: &KernelBank,
    first_plane: u32,
    measured: &[f32],
    blurred: &[f32],
    estimate: &[f32],
    out: &mut [f32],
) {
    let [width, height, _] = grid.dims;
    out.par_chunks_mut((width * height) as usize)
        .enumerate()
        .for_each(|(z, slice)| {
            let taps = bank.taps(bank.layer(first_plane, z as u32));
            for y in 0..height {
                for x in 0..width {
                    let position = [x, y, z as u32];
                    let mut sum = 0.0f32;
                    for tap_z in 0..grid.taps[2] {
                        for tap_y in 0..grid.taps[1] {
                            for tap_x in 0..grid.taps[0] {
                                let tap = [tap_x, tap_y, tap_z];
                                let source = grid.source(position, tap);
                                sum += measured[source] / blurred[source].max(FLOOR)
                                    * taps[grid.weight(tap, true)];
                            }
                        }
                    }
                    let index = (x + width * y) as usize;
                    let cell = (z as u32 * width * height + index as u32) as usize;
                    slice[index] = estimate[cell] * sum;
                }
            }
        });
}
