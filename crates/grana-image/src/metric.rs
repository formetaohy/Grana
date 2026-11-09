use std::ops::Range;

use glam::Vec3;
use grana_volume::{Basis, Extent, Level, Pyramid, VoxelData};

use crate::Error;
use crate::align;
use crate::image::Image;

pub struct Metric {
    pyramid: Pyramid,
}

impl Metric {
    pub fn of(pyramid: &Pyramid) -> Option<Self> {
        if pyramid
            .levels()
            .iter()
            .all(|level| level.basis().orthogonal())
        {
            return None;
        }
        let fine = pyramid.level(0);
        let basis = orthogonal(fine.basis());
        let center = Vec3::from(fine.center());
        let (low, high) = fine.bounds();
        let world = Vec3::from(high) - Vec3::from(low);
        let extent = Extent::new(
            axis_extent(world, basis, 0),
            axis_extent(world, basis, 1),
            axis_extent(world, basis, 2),
        );
        let levels = (0..pyramid.level_count())
            .map(|index| {
                let step = (1u32 << index) as f32;
                let level_basis = Basis::new(std::array::from_fn(|axis| {
                    (Vec3::from(basis.column(axis)) * step).to_array()
                }));
                let reach = Vec3::new(
                    extent.x().div_ceil(1 << index).max(1) as f32,
                    extent.y().div_ceil(1 << index).max(1) as f32,
                    extent.z().div_ceil(1 << index).max(1) as f32,
                );
                let origin = center - level_basis.matrix() * (0.5 * reach);
                Level::placed(reach_extent(reach), level_basis, origin.to_array())
            })
            .collect::<Vec<_>>();
        Some(Self {
            pyramid: Pyramid::placed(
                levels,
                pyramid.stored(),
                pyramid.channels().to_vec(),
                pyramid.frame_count(),
                pyramid.frame_interval(),
                pyramid.placement(),
            ),
        })
    }

    pub fn pyramid(&self) -> &Pyramid {
        &self.pyramid
    }

    pub fn read_region(
        &self,
        image: &Image,
        time: u32,
        level: usize,
        channel: usize,
        ranges: &[Range<u32>; 3],
        shift: [f32; 3],
    ) -> Result<VoxelData, Error> {
        let Some(format) = image
            .pyramid()
            .channels()
            .get(channel)
            .map(|info| info.format())
        else {
            return Err(Error::Resolved {
                detail: "the requested channel does not exist".to_string(),
            });
        };
        let source = image.pyramid().level(level);
        let target = self.pyramid.level(level);
        let shift = align::level_shift(shift, level);
        let limits = [
            source.extent().x(),
            source.extent().y(),
            source.extent().z(),
        ];
        let Some(gather) = gather(source, target, ranges, shift, limits) else {
            return Ok(align::zeros(format, ranges));
        };
        let data = image.read_region(time, level, channel, &gather, [0.0; 3])?;
        let view = View {
            source,
            target,
            shift,
            limits,
            gather,
        };
        Ok(match data {
            VoxelData::U8(values) => VoxelData::U8(resample(&values, ranges, &view)),
            VoxelData::U16(values) => VoxelData::U16(resample(&values, ranges, &view)),
            VoxelData::F32(values) => VoxelData::F32(resample(&values, ranges, &view)),
        })
    }
}

struct View {
    source: Level,
    target: Level,
    shift: [f32; 3],
    limits: [u32; 3],
    gather: [Range<u32>; 3],
}

fn resample<T: Value>(data: &[T], ranges: &[Range<u32>; 3], view: &View) -> Vec<T> {
    let View {
        source,
        target,
        shift,
        limits,
        gather,
    } = view;
    let gathered = Extent::new(
        gather[0].end - gather[0].start,
        gather[1].end - gather[1].start,
        gather[2].end - gather[2].start,
    );
    assert_eq!(
        data.len() as u64,
        gathered.voxel_count(),
        "the gathered region holds every source voxel"
    );
    let width = (ranges[0].end - ranges[0].start) as usize;
    let height = (ranges[1].end - ranges[1].start) as usize;
    let depth = (ranges[2].end - ranges[2].start) as usize;
    let matrix = target.basis().matrix();
    let inverse = source.basis().inverse();
    let center = Vec3::from(target.origin())
        + matrix
            * Vec3::new(
                ranges[0].start as f32 + 0.5,
                ranges[1].start as f32 + 0.5,
                ranges[2].start as f32 + 0.5,
            )
        - Vec3::from(source.origin());
    let start = inverse * center - Vec3::splat(0.5) + Vec3::from(*shift);
    let steps = [0, 1, 2].map(|axis| inverse * matrix.col(axis));
    let reach = Vec3::new(
        limits[0] as f32 - 1.0,
        limits[1] as f32 - 1.0,
        limits[2] as f32 - 1.0,
    );
    let base = [
        gather[0].start as i64,
        gather[1].start as i64,
        gather[2].start as i64,
    ];
    let mut values = Vec::with_capacity(width * height * depth);
    for z in 0..depth {
        for y in 0..height {
            let row = start + steps[2] * z as f32 + steps[1] * y as f32;
            for x in 0..width {
                values.push(T::encode(sample(
                    data,
                    &gathered,
                    &base,
                    row + steps[0] * x as f32,
                    reach,
                )));
            }
        }
    }
    values
}

fn sample<T: Value>(
    data: &[T],
    gathered: &Extent,
    base: &[i64; 3],
    point: Vec3,
    reach: Vec3,
) -> f32 {
    let out = (0..3).any(|axis| point[axis] < 0.0 || point[axis] > reach[axis]);
    if out {
        return 0.0;
    }
    let index = [
        point.x.floor() as i64,
        point.y.floor() as i64,
        point.z.floor() as i64,
    ];
    let weight = [
        point.x - index[0] as f32,
        point.y - index[1] as f32,
        point.z - index[2] as f32,
    ];
    let last = [
        base[0] + i64::from(gathered.x()) - 1,
        base[1] + i64::from(gathered.y()) - 1,
        base[2] + i64::from(gathered.z()) - 1,
    ];
    let mut value = 0.0;
    for corner in 0..8usize {
        let shift = [corner & 1, (corner >> 1) & 1, (corner >> 2) & 1];
        let mut factor = 1.0;
        let mut tap = [0u32; 3];
        for axis in 0..3 {
            let at = (index[axis] + shift[axis] as i64).clamp(base[axis], last[axis]) - base[axis];
            tap[axis] = at as u32;
            factor *= if shift[axis] == 0 {
                1.0 - weight[axis]
            } else {
                weight[axis]
            };
        }
        if factor == 0.0 {
            continue;
        }
        value += data[gathered.index(tap[0], tap[1], tap[2]) as usize].decode() * factor;
    }
    value
}

fn gather(
    source: Level,
    target: Level,
    ranges: &[Range<u32>; 3],
    shift: [f32; 3],
    limits: [u32; 3],
) -> Option<[Range<u32>; 3]> {
    let matrix = target.basis().matrix();
    let inverse = source.basis().inverse();
    let origin = Vec3::from(target.origin()) - Vec3::from(source.origin());
    let mut low = Vec3::splat(f32::INFINITY);
    let mut high = Vec3::splat(f32::NEG_INFINITY);
    let mut corner_of = [[0u32; 3]; 8];
    for (corner, slot) in corner_of.iter_mut().enumerate() {
        for axis in 0..3 {
            let range = &ranges[axis];
            slot[axis] = if corner & (1 << axis) == 0 {
                range.start
            } else {
                range.end
            };
        }
    }
    for corner in corner_of {
        let index = Vec3::new(corner[0] as f32, corner[1] as f32, corner[2] as f32);
        let point = inverse * (origin + matrix * index) - Vec3::splat(0.5) + Vec3::from(shift);
        low = low.min(point);
        high = high.max(point);
    }
    let mut gathered = [0..0, 0..0, 0..0];
    for axis in 0..3 {
        let limit = i64::from(limits[axis]);
        if high[axis] < 0.0 || low[axis] > (limit - 1) as f32 {
            return None;
        }
        let first = (low[axis].floor() as i64).clamp(0, limit - 1);
        let last = ((high[axis].floor() as i64) + 2).clamp(first + 1, limit);
        gathered[axis] = first as u32..last as u32;
    }
    Some(gathered)
}

fn axis_extent(world: Vec3, basis: Basis, axis: usize) -> u32 {
    let column = Vec3::from(basis.column(axis));
    let reach = column.normalize().abs().dot(world) / column.length();
    reach.ceil().max(1.0) as u32
}

fn orthogonal(basis: Basis) -> Basis {
    let columns = [0, 1, 2].map(|axis| Vec3::from(basis.column(axis)));
    let lengths = columns.map(Vec3::length);
    let first = columns[0] / lengths[0];
    let second = (columns[1] - first * columns[1].dot(first)).normalize();
    let third = first.cross(second);
    let scale = (basis.determinant().abs() / (lengths[0] * lengths[1] * lengths[2])).cbrt();
    Basis::new([
        (first * (lengths[0] * scale)).to_array(),
        (second * (lengths[1] * scale)).to_array(),
        (third * (lengths[2] * scale)).to_array(),
    ])
}

fn reach_extent(reach: Vec3) -> Extent {
    Extent::new(
        reach.x.max(1.0) as u32,
        reach.y.max(1.0) as u32,
        reach.z.max(1.0) as u32,
    )
}

trait Value: Copy {
    fn decode(self) -> f32;
    fn encode(value: f32) -> Self;
}

impl Value for u8 {
    fn decode(self) -> f32 {
        f32::from(self)
    }

    fn encode(value: f32) -> Self {
        (value + 0.5).clamp(0.0, f32::from(u8::MAX)) as Self
    }
}

impl Value for u16 {
    fn decode(self) -> f32 {
        f32::from(self)
    }

    fn encode(value: f32) -> Self {
        (value + 0.5).clamp(0.0, f32::from(u16::MAX)) as Self
    }
}

impl Value for f32 {
    fn decode(self) -> f32 {
        self
    }

    fn encode(value: f32) -> Self {
        value
    }
}
