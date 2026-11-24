use std::ops::Range;

use grana_volume::{
    BRICK_SIDE, Extent, NoData, PADDED_SIDE, VoxelData, VoxelFormat, VoxelSemantics, VoxelValue,
};

const RATIO: u32 = 2;

pub(crate) const STEP: u32 = RATIO;

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
    no_data: NoData,
) -> VoxelData {
    assert_eq!(
        data.format(),
        format,
        "a reduced source holds the format of its level"
    );
    assert_eq!(
        no_data.format(),
        format,
        "a reduced source holds the no-data value of its format"
    );
    assert_eq!(
        data.voxel_count(),
        source.count(),
        "a reduced source holds every voxel it serves"
    );
    let averaging = semantics.averages();
    let declared = no_data.declared();
    macro_rules! reduce_arms {
        ($($variant:ident),*) => {
            match data {
                $(
                    VoxelData::$variant(held) => {
                        let fill = match no_data {
                            NoData::$variant(fill) => fill,
                            _ => unreachable!("a reduced source holds the no-data value of its format"),
                        };
                        VoxelData::$variant(reduced(
                            held,
                            source,
                            ranges,
                            averaging,
                            fill,
                            declared,
                        ))
                    }
                )*
            }
        };
    }
    reduce_arms!(U8, U16, U32, U64, I8, I16, I32, I64, F32, F64)
}

fn reduced<T: VoxelValue>(
    data: &[T],
    source: &Source,
    ranges: &[Range<u32>; 3],
    averages: bool,
    fill: T,
    no_data: Option<NoData>,
) -> Vec<T> {
    let count = ranges
        .iter()
        .map(|range| (range.end - range.start) as usize)
        .product::<usize>();
    let mut values = Vec::with_capacity(count);
    for z in ranges[2].clone() {
        for y in ranges[1].clone() {
            for x in ranges[0].clone() {
                let base = [RATIO * x, RATIO * y, RATIO * z];
                if !averages {
                    values.push(data[source.index(base[0], base[1], base[2])]);
                    continue;
                }
                let mut total = 0.0;
                let mut present = 0u32;
                for dz in 0..RATIO {
                    let zz = (base[2] + dz).min(source.extent[2] - 1);
                    for dy in 0..RATIO {
                        let yy = (base[1] + dy).min(source.extent[1] - 1);
                        for dx in 0..RATIO {
                            let xx = (base[0] + dx).min(source.extent[0] - 1);
                            let held = data[source.index(xx, yy, zz)];
                            if no_data.is_some_and(|no_data| held.is_no_data(no_data)) {
                                continue;
                            }
                            total += held.to_f64();
                            present += 1;
                        }
                    }
                }
                values.push(if present == 0 {
                    fill
                } else {
                    T::from_f64(total / f64::from(present))
                });
            }
        }
    }
    values
}
