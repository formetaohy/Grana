use grana_image::Image;
use grana_optics::Kernel;

use crate::detect::Bead;
use crate::error::Error;
use crate::settings::Settings;

const SHIFT_LIMIT: f32 = 4.0;

pub(crate) fn patch(
    image: &Image,
    settings: &Settings,
    bead: &Bead,
) -> Result<Option<Kernel>, Error> {
    let level = image.pyramid().level(0);
    let extent = level.extent();
    let half = settings.half();
    let centre = level.voxel_of(bead.world).map(|value| value - 0.5);
    let start = [0, 1, 2].map(|axis| (centre[axis] - half[axis] as f32).round() as i64);
    let dims = [0, 1, 2].map(|axis| 2 * half[axis] + 1);
    let limits = [extent.x(), extent.y(), extent.z()];
    for axis in 0..3 {
        if start[axis] < 0 || start[axis] + i64::from(dims[axis]) > i64::from(limits[axis]) {
            return Ok(None);
        }
    }
    let ranges = [0, 1, 2].map(|axis| start[axis] as u32..start[axis] as u32 + dims[axis]);
    let data = image.read_region(
        settings.time(),
        0,
        settings.channel(),
        &ranges,
        &grana_warp::Warp::IDENTITY,
    )?;
    let mut values = vec![0.0f32; data.voxel_count()];
    data.write_values(&mut values);
    let background = border_median(&values, dims);
    for value in &mut values {
        *value = (*value - background).max(0.0);
    }
    let total: f32 = values.iter().sum();
    if !(total.is_finite() && total > 0.0) {
        return Ok(None);
    }
    let centroid = [0, 1, 2].map(|axis| {
        let mut moment = 0.0f64;
        for z in 0..dims[2] {
            for y in 0..dims[1] {
                for x in 0..dims[0] {
                    let index = (x + dims[0] * (y + dims[1] * z)) as usize;
                    moment += f64::from(values[index])
                        * match axis {
                            0 => f64::from(x),
                            1 => f64::from(y),
                            _ => f64::from(z),
                        };
                }
            }
        }
        (moment / f64::from(total)) as f32
    });
    let shift = [0, 1, 2].map(|axis| centroid[axis] - half[axis] as f32);
    if shift.iter().any(|value| value.abs() > SHIFT_LIMIT) {
        return Ok(None);
    }
    let shifted = shifted_values(&values, dims, shift);
    Ok(Some(Kernel::from_values(dims, shifted)))
}

fn border_median(values: &[f32], dims: [u32; 3]) -> f32 {
    let mut border = Vec::new();
    for z in 0..dims[2] {
        for y in 0..dims[1] {
            for x in 0..dims[0] {
                if x == 0
                    || y == 0
                    || z == 0
                    || x + 1 == dims[0]
                    || y + 1 == dims[1]
                    || z + 1 == dims[2]
                {
                    border.push(values[(x + dims[0] * (y + dims[1] * z)) as usize]);
                }
            }
        }
    }
    if border.is_empty() {
        return 0.0;
    }
    border.sort_unstable_by(f32::total_cmp);
    border[border.len() / 2]
}

fn shifted_values(values: &[f32], dims: [u32; 3], shift: [f32; 3]) -> Vec<f32> {
    let mut shifted = vec![0.0f32; values.len()];
    for z in 0..dims[2] {
        for y in 0..dims[1] {
            for x in 0..dims[0] {
                let at = [
                    x as f32 + shift[0],
                    y as f32 + shift[1],
                    z as f32 + shift[2],
                ];
                shifted[(x + dims[0] * (y + dims[1] * z)) as usize] = sample(values, dims, at);
            }
        }
    }
    shifted
}

fn sample(values: &[f32], dims: [u32; 3], at: [f32; 3]) -> f32 {
    let low = [0, 1, 2].map(|axis| at[axis].floor() as i64);
    let fraction = [0, 1, 2].map(|axis| at[axis] - at[axis].floor());
    let mut total = 0.0f32;
    for z in 0..2 {
        for y in 0..2 {
            for x in 0..2 {
                let corner = [low[0] + x, low[1] + y, low[2] + z];
                if [0, 1, 2]
                    .iter()
                    .any(|axis| corner[*axis] < 0 || corner[*axis] >= i64::from(dims[*axis]))
                {
                    continue;
                }
                let corner_weight = [
                    if x == 0 {
                        1.0 - fraction[0]
                    } else {
                        fraction[0]
                    },
                    if y == 0 {
                        1.0 - fraction[1]
                    } else {
                        fraction[1]
                    },
                    if z == 0 {
                        1.0 - fraction[2]
                    } else {
                        fraction[2]
                    },
                ];
                let index = (corner[0]
                    + i64::from(dims[0]) * (corner[1] + i64::from(dims[1]) * corner[2]))
                    as usize;
                total += values[index] * corner_weight.iter().product::<f32>();
            }
        }
    }
    total.max(0.0)
}
