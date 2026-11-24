use std::ops::Range;

use grana_volume::{BRICK_SIDE, Extent, PADDED_SIDE, VoxelData, VoxelFormat, VoxelSemantics};

const RATIO: u32 = 2;

pub(crate) const STEP: u32 = RATIO;

const SAMPLES: f64 = 8.0;

pub(crate) struct Source {
    dims: [usize; 3],
    origin: [u32; 3],
    extent: [u32; 3],
}

impl Source {
    pub(crate) fn new(extent: Extent, ranges: &[Range<u32>; 3]) -> Self {
        Self {
            dims: [
                (ranges[0].end - ranges[0].start) as usize,
                (ranges[1].end - ranges[1].start) as usize,
                (ranges[2].end - ranges[2].start) as usize,
            ],
            origin: [ranges[0].start, ranges[1].start, ranges[2].start],
            extent: [extent.x(), extent.y(), extent.z()],
        }
    }

    pub(crate) fn count(&self) -> usize {
        self.dims[0] * self.dims[1] * self.dims[2]
    }

    pub(crate) fn stash(&self, destination: &mut VoxelData, brick: &VoxelData, coord: [u32; 3]) {
        assert_eq!(
            destination.format(),
            brick.format(),
            "a reduced source holds the format of its level"
        );
        assert_eq!(
            brick.voxel_count(),
            (PADDED_SIDE as usize).pow(3),
            "a brick holds a padded cube"
        );
        let base = [
            coord[0] * BRICK_SIDE,
            coord[1] * BRICK_SIDE,
            coord[2] * BRICK_SIDE,
        ];
        macro_rules! stash_arms {
            ($($variant:ident),*) => {
                match (destination, brick) {
                    $(
                        (VoxelData::$variant(destination), VoxelData::$variant(brick)) => {
                            stash(
                                destination,
                                &self.dims,
                                &self.origin,
                                &self.extent,
                                brick,
                                &base,
                            );
                        }
                    )*
                    _ => panic!("a reduced source holds the format of its level"),
                }
            };
        }
        stash_arms!(U8, U16, U32, U64, I8, I16, I32, I64, F32, F64);
    }

    fn index(&self, x: u32, y: u32, z: u32) -> usize {
        let local = [
            x as usize - self.origin[0] as usize,
            y as usize - self.origin[1] as usize,
            z as usize - self.origin[2] as usize,
        ];
        assert!(
            local[0] < self.dims[0] && local[1] < self.dims[1] && local[2] < self.dims[2],
            "a reduced source holds every voxel it serves"
        );
        local[0] + self.dims[0] * (local[1] + self.dims[1] * local[2])
    }
}

fn stash<T: Copy>(
    destination: &mut [T],
    dims: &[usize; 3],
    origin: &[u32; 3],
    extent: &[u32; 3],
    brick: &[T],
    base: &[u32; 3],
) {
    let side = PADDED_SIDE as usize;
    for z in 0..PADDED_SIDE {
        let absolute_z = absolute(base[2], z, extent[2]);
        if absolute_z < origin[2] || absolute_z >= origin[2] + dims[2] as u32 {
            continue;
        }
        for y in 0..PADDED_SIDE {
            let absolute_y = absolute(base[1], y, extent[1]);
            if absolute_y < origin[1] || absolute_y >= origin[1] + dims[1] as u32 {
                continue;
            }
            for x in 0..PADDED_SIDE {
                let absolute_x = absolute(base[0], x, extent[0]);
                if absolute_x < origin[0] || absolute_x >= origin[0] + dims[0] as u32 {
                    continue;
                }
                let at = (absolute_x - origin[0]) as usize
                    + dims[0]
                        * ((absolute_y - origin[1]) as usize
                            + dims[1] * (absolute_z - origin[2]) as usize);
                let from = x as usize + side * (y as usize + side * z as usize);
                destination[at] = brick[from];
            }
        }
    }
}

fn absolute(base: u32, offset: u32, extent: u32) -> u32 {
    (base + offset).saturating_sub(1).min(extent - 1)
}

pub(crate) fn region(
    format: VoxelFormat,
    semantics: VoxelSemantics,
    source: &Source,
    data: &VoxelData,
    ranges: &[Range<u32>; 3],
) -> VoxelData {
    assert_eq!(
        data.format(),
        format,
        "a reduced source holds the format of its level"
    );
    assert_eq!(
        data.voxel_count(),
        source.count(),
        "a reduced source holds every voxel it serves"
    );
    let count = ranges
        .iter()
        .map(|range| (range.end - range.start) as usize)
        .product::<usize>();
    let mut values = Vec::with_capacity(count);
    macro_rules! reduce_arms {
        ($($variant:ident => $cast:expr),*) => {
            match data {
                $(
                    VoxelData::$variant(held) => {
                        reduced(
                            held,
                            source,
                            ranges,
                            semantics.averages(),
                            $cast,
                            &mut values,
                        );
                    }
                )*
            }
        };
    }
    reduce_arms!(
        U8 => |value: u8| f64::from(value),
        U16 => |value: u16| f64::from(value),
        U32 => |value: u32| f64::from(value),
        U64 => |value: u64| value as f64,
        I8 => |value: i8| f64::from(value),
        I16 => |value: i16| f64::from(value),
        I32 => |value: i32| f64::from(value),
        I64 => |value: i64| value as f64,
        F32 => |value: f32| f64::from(value),
        F64 => |value: f64| value
    );
    let mut next = values.into_iter();
    VoxelData::from_values(format, count, || {
        next.next().expect("a reduced region fills every voxel")
    })
}

fn reduced<T: Copy>(
    data: &[T],
    source: &Source,
    ranges: &[Range<u32>; 3],
    averages: bool,
    cast: impl Fn(T) -> f64,
    values: &mut Vec<f64>,
) {
    for z in ranges[2].clone() {
        for y in ranges[1].clone() {
            for x in ranges[0].clone() {
                let base = [RATIO * x, RATIO * y, RATIO * z];
                if averages {
                    let mut total = 0.0;
                    for dz in 0..RATIO {
                        let zz = (base[2] + dz).min(source.extent[2] - 1);
                        for dy in 0..RATIO {
                            let yy = (base[1] + dy).min(source.extent[1] - 1);
                            for dx in 0..RATIO {
                                let xx = (base[0] + dx).min(source.extent[0] - 1);
                                total += cast(data[source.index(xx, yy, zz)]);
                            }
                        }
                    }
                    values.push(total / SAMPLES);
                } else {
                    values.push(cast(data[source.index(base[0], base[1], base[2])]));
                }
            }
        }
    }
}
