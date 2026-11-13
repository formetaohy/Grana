use std::ops::Range;
use std::sync::atomic::AtomicBool;

use grana_object::{CellParams, Mask};
use grana_volume::{Level, Pyramid, Region};

use crate::schedule;

mod barrier;
mod flood;
mod plan;
mod ridge;

pub(crate) const LEVELS: usize = 1 << 16;
pub(crate) const TOP: u16 = (LEVELS - 1) as u16;
pub(crate) const CHECK: usize = 1 << 16;
pub(crate) const SNAP: i64 = 2;

pub trait CellField: Sync {
    fn values(&self, ranges: &[Range<u32>; 3]) -> Result<Vec<f32>, String>;
}

pub struct CellSeed {
    centre: [f32; 3],
    radius: f32,
}

impl CellSeed {
    pub fn new(centre: [f32; 3], radius: f32) -> Self {
        assert!(
            centre.iter().all(|value| value.is_finite()),
            "a cell seed names a finite centre"
        );
        assert!(
            radius.is_finite() && radius > 0.0,
            "a cell seed names a positive nucleus radius"
        );
        Self { centre, radius }
    }

    pub fn centre(&self) -> [f32; 3] {
        self.centre
    }

    pub fn radius(&self) -> f32 {
        self.radius
    }
}

pub struct GrowField<'a> {
    pub level: Level,
    pub ranges: [Range<u32>; 3],
    pub params: CellParams,
    pub band: u32,
    pub mask: Option<&'a Mask>,
    pub seeds: &'a [CellSeed],
}

pub struct Band {
    start: u32,
    core: u32,
    planes: u32,
    labels: Vec<u32>,
}

impl Band {
    pub fn start(&self) -> u32 {
        self.start
    }

    pub fn core(&self) -> u32 {
        self.core
    }

    pub fn planes(&self) -> u32 {
        self.planes
    }

    pub fn labels(&self) -> &[u32] {
        &self.labels
    }
}

pub struct Growth<'a> {
    level: Level,
    ranges: [Range<u32>; 3],
    params: CellParams,
    band: u32,
    pad: u32,
    halo: u32,
    mask: Option<&'a Mask>,
    seeds: &'a [CellSeed],
    source: &'a dyn CellField,
    cancel: &'a AtomicBool,
    centres: Vec<[f32; 3]>,
    snapped: Vec<Option<[f32; 3]>>,
    barrier: u16,
    present: Vec<bool>,
}

pub fn cell_band(level: Level, ranges: &[Range<u32>; 3], params: CellParams, reach: f32) -> u32 {
    plan::preferred(ranges, level, params, reach)
}

pub fn cell_level(
    pyramid: &Pyramid,
    region: Region,
    params: CellParams,
    radius: f32,
) -> Option<usize> {
    plan::level(pyramid, region, params, radius)
}

impl<'a> Growth<'a> {
    pub fn new(
        field: GrowField<'a>,
        source: &'a dyn CellField,
        cancel: &'a AtomicBool,
    ) -> Result<Option<Self>, String> {
        let GrowField {
            level,
            ranges,
            params,
            band,
            mask,
            seeds,
        } = field;
        if seeds.is_empty() || ranges.iter().any(Range::is_empty) {
            return Ok(None);
        }
        assert!(band > 0, "a growth band holds at least one plane");
        let reach = params.reach() * seeds.iter().map(CellSeed::radius).fold(0.0f32, f32::max);
        let workers = schedule::workers(0);
        let seeds_context = barrier::Seeds {
            level,
            ranges: &ranges,
            params,
            seeds,
            source,
            workers,
            cancel,
        };
        let centres = seeds
            .iter()
            .map(|seed| level.voxel_of(seed.centre()))
            .collect::<Vec<_>>();
        let Some(barrier) = barrier::barrier(&seeds_context)? else {
            return Ok(None);
        };
        let Some(snapped) = barrier::snap(&seeds_context, barrier)? else {
            return Ok(None);
        };
        if snapped.iter().all(Option::is_none) {
            return Ok(None);
        }
        Ok(Some(Self {
            level,
            ranges,
            params,
            band,
            pad: plan::pad(level, reach),
            halo: plan::halo(level, params.grain()),
            mask,
            seeds,
            source,
            cancel,
            centres,
            snapped,
            barrier,
            present: vec![false; seeds.len()],
        }))
    }

    pub fn bands(&self) -> usize {
        plan::count(&self.ranges, self.band)
    }

    pub fn band(&mut self, index: usize) -> Result<Option<Band>, String> {
        let [range_x, range_y, range_z] = &self.ranges;
        let width = (range_x.end - range_x.start) as usize;
        let height = (range_y.end - range_y.start) as usize;
        let plane = width * height;
        let core = plan::core(&self.ranges, self.band, index);
        let domain = plan::expanded(range_z, core.clone(), self.pad);
        let elevations = plan::expanded(range_z, domain.clone(), self.halo);
        let values = self
            .source
            .values(&[range_x.clone(), range_y.clone(), elevations.clone()])?;
        let elevation_dims = [width, height, (elevations.end - elevations.start) as usize];
        assert_eq!(
            values.len(),
            elevation_dims[0] * elevation_dims[1] * elevation_dims[2],
            "the membrane field covers the read box"
        );
        let workers = schedule::workers(0);
        let Some(field) = ridge::elevations(
            &values,
            elevation_dims,
            self.level,
            self.params.grain(),
            workers,
            self.cancel,
        ) else {
            return Ok(None);
        };
        let dims = [width, height, (domain.end - domain.start) as usize];
        let offset = (domain.start - elevations.start) as usize;
        let field = &field[offset * plane..(offset + dims[2]) * plane];
        let origin = [range_x.start, range_y.start, domain.start];
        let mut labels = vec![0u32; plane * dims[2]];
        let mut sown = Vec::new();
        let mut slots = vec![0u32; self.seeds.len() + 1];
        for (slot, centre) in self.snapped.iter().enumerate() {
            let Some(centre) = centre else {
                continue;
            };
            let voxel = [0, 1, 2].map(|axis| (centre[axis] - origin[axis] as f32) as i64);
            if (0..3).any(|axis| voxel[axis] < 0 || voxel[axis] >= dims[axis] as i64) {
                continue;
            }
            slots[slot + 1] = sown.len() as u32 + 1;
            sown.push(flood::Sown::new(
                slot,
                voxel.map(|value| value as usize),
                self.centres[slot],
                self.params.reach() * self.seeds[slot].radius(),
            ));
        }
        let grown = flood::Flood {
            labels: &mut labels,
            elevations: field,
            dims,
            origin,
            steps: self.level.voxel(),
            barrier: self.barrier,
            mask: self.mask,
            sown: &sown,
            slots: &slots,
        }
        .run(self.cancel);
        if !grown {
            return Ok(None);
        }
        let core_planes = (core.end - core.start) as usize;
        let planes = core_planes + usize::from(core.end < range_z.end);
        let start = (core.start - domain.start) as usize;
        assert!(
            start + planes <= dims[2],
            "a growth band holds its core and the plane above it"
        );
        let held = &labels[start * plane..(start + planes) * plane];
        for label in &held[..core_planes * plane] {
            if *label != 0 {
                self.present[*label as usize - 1] = true;
            }
        }
        Ok(Some(Band {
            start: core.start,
            core: core_planes as u32,
            planes: planes as u32,
            labels: held.to_vec(),
        }))
    }

    pub fn nuclei(&self) -> Vec<usize> {
        self.present
            .iter()
            .enumerate()
            .filter_map(|(index, present)| present.then_some(index))
            .collect()
    }
}

pub(crate) fn dims(ranges: &[Range<u32>; 3]) -> [usize; 3] {
    [
        (ranges[0].end - ranges[0].start) as usize,
        (ranges[1].end - ranges[1].start) as usize,
        (ranges[2].end - ranges[2].start) as usize,
    ]
}
