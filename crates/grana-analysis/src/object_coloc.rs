use std::collections::BTreeSet;
use std::ops::Range;

use grana_measure::{ColocPair, ObjectColocValue};
use grana_object::ObjectAnchor;
use grana_volume::{Intensity, Level, Region};

use crate::coloc::{ColocRead, scale, unit};
use crate::schedule;

pub type LabelRead<'a> = dyn Fn(&[Range<u32>; 3]) -> Result<Vec<u16>, String> + Sync + 'a;

pub const OBJECT_COLOC_FIELDS: usize = 6;
pub const OBJECT_COLOC_VOXELS: u64 = 128 << 20;

const TILE: [u32; 3] = [128, 128, 16];

pub fn object_coloc_level(
    pyramid: &grana_volume::Pyramid,
    frames: u32,
    region: Region,
    anchors: &dyn Fn(u32) -> Vec<ObjectAnchor>,
) -> usize {
    let levels = pyramid.level_count();
    let mut best = levels - 1;
    for level in 0..levels {
        let info = pyramid.level(level);
        if region.voxel_range(info).is_none() {
            continue;
        }
        let mut total = 0u64;
        let mut over = false;
        for time in 0..frames {
            for anchor in anchors(time) {
                total += anchor_voxels(info, region, anchor.bounds());
                if total > OBJECT_COLOC_VOXELS {
                    over = true;
                    break;
                }
            }
            if over {
                break;
            }
        }
        if !over {
            return level;
        }
        best = level;
    }
    best
}

pub struct ObjectColocScan {
    pair: ColocPair,
    level: usize,
    tiles: Vec<[Range<u32>; 3]>,
    scales: [[f32; 2]; 2],
    objects: usize,
}

impl ObjectColocScan {
    pub fn new(
        pair: ColocPair,
        level: usize,
        info: Level,
        region: Region,
        intensities: [Intensity; 2],
        objects: usize,
        anchors: &[ObjectAnchor],
    ) -> Self {
        assert!(
            objects > 0,
            "an object colocalization measures at least one object"
        );
        assert!(
            intensities.iter().all(|intensity| intensity.span() > 0.0),
            "an object colocalization selects channels with an intensity range"
        );
        let tiles = object_coloc_tiles(info, region, anchors);
        Self {
            pair,
            level,
            tiles,
            scales: intensities.map(scale),
            objects,
        }
    }

    pub fn level(&self) -> usize {
        self.level
    }

    pub fn slice(
        &self,
        workers: usize,
        cancel: &std::sync::atomic::AtomicBool,
        labels: &LabelRead<'_>,
        read: &ColocRead<'_>,
        time: u32,
        progress: &mut dyn FnMut(usize, usize),
    ) -> Result<Option<Vec<ObjectColocValue>>, String> {
        let mut totals = vec![0.0f64; self.objects * OBJECT_COLOC_FIELDS];
        let completed = schedule::ordered(
            schedule::Run {
                total: self.tiles.len(),
                workers: schedule::workers(workers),
                cancel,
                progress,
            },
            Tile::new,
            |tile, index| {
                tile.accumulate(
                    &self.tiles[index],
                    &TileContext {
                        labels,
                        read,
                        time,
                        pair: self.pair,
                        scales: self.scales,
                        objects: self.objects,
                    },
                )
            },
            |_, values| {
                merge(&mut totals, &values);
                Ok(())
            },
            |_| {},
        )?;
        if !completed {
            return Ok(None);
        }
        Ok(Some(object_values(&totals)))
    }
}

fn object_coloc_tiles(
    info: Level,
    region: Region,
    anchors: &[ObjectAnchor],
) -> Vec<[Range<u32>; 3]> {
    let Some(ranges) = region.voxel_range(info) else {
        return Vec::new();
    };
    let origin = [0, 1, 2].map(|axis| ranges[axis].start);
    let mut tiles = BTreeSet::new();
    for anchor in anchors {
        let bounds = anchor.bounds();
        let low = info.voxel_of(bounds.min());
        let high = info.voxel_of(bounds.max());
        let mut cell = [(0u32, 0u32); 3];
        let mut empty = false;
        for axis in 0..3 {
            let start = (low[axis].floor() as i64)
                .clamp(i64::from(ranges[axis].start), i64::from(ranges[axis].end));
            let end = ((high[axis].ceil() as i64) + 1)
                .clamp(i64::from(ranges[axis].start), i64::from(ranges[axis].end));
            if end <= start {
                empty = true;
                break;
            }
            let first = (start as u32 - origin[axis]) / TILE[axis];
            let last = (end as u32 - 1 - origin[axis]) / TILE[axis];
            cell[axis] = (first, last);
        }
        if empty {
            continue;
        }
        for z in cell[2].0..=cell[2].1 {
            for y in cell[1].0..=cell[1].1 {
                for x in cell[0].0..=cell[0].1 {
                    tiles.insert((x, y, z));
                }
            }
        }
    }
    tiles
        .into_iter()
        .map(|(x, y, z)| {
            [0, 1, 2].map(|axis| {
                let start = origin[axis] + [x, y, z][axis] * TILE[axis];
                start..(start + TILE[axis]).min(ranges[axis].end)
            })
        })
        .collect()
}

fn anchor_voxels(info: Level, region: Region, bounds: grana_geometry::Bounds) -> u64 {
    let Some(ranges) = region.voxel_range(info) else {
        return 0;
    };
    let low = info.voxel_of(bounds.min());
    let high = info.voxel_of(bounds.max());
    (0..3)
        .map(|axis| {
            let start = (low[axis].floor() as i64)
                .clamp(i64::from(ranges[axis].start), i64::from(ranges[axis].end));
            let end = ((high[axis].ceil() as i64) + 1)
                .clamp(i64::from(ranges[axis].start), i64::from(ranges[axis].end));
            (end - start).max(0) as u64
        })
        .product()
}

struct TileContext<'a> {
    labels: &'a LabelRead<'a>,
    read: &'a ColocRead<'a>,
    time: u32,
    pair: ColocPair,
    scales: [[f32; 2]; 2],
    objects: usize,
}

struct Tile {
    sums: Vec<f64>,
    touched: Vec<u16>,
}

impl Tile {
    fn new() -> Self {
        Self {
            sums: Vec::new(),
            touched: Vec::new(),
        }
    }

    fn accumulate(
        &mut self,
        ranges: &[Range<u32>; 3],
        context: &TileContext<'_>,
    ) -> Result<Vec<(u16, [f64; OBJECT_COLOC_FIELDS])>, String> {
        let TileContext {
            labels,
            read,
            time,
            pair,
            scales,
            objects,
        } = *context;
        if self.sums.is_empty() {
            self.sums = vec![0.0; objects * OBJECT_COLOC_FIELDS];
        }
        assert_eq!(
            self.sums.len(),
            objects * OBJECT_COLOC_FIELDS,
            "an object colocalization scans one object count"
        );
        let dims = [0, 1, 2].map(|axis| ranges[axis].end - ranges[axis].start);
        let voxels = dims.iter().map(|dim| *dim as usize).product::<usize>();
        let labels = labels(ranges)?;
        assert_eq!(
            labels.len(),
            voxels,
            "a label tile covers the voxels of its ranges"
        );
        if labels.iter().all(|label| *label == 0) {
            return Ok(Vec::new());
        }
        let a = read(time, pair.a(), ranges)?;
        let b = read(time, pair.b(), ranges)?;
        assert!(
            a.voxel_count() == voxels && b.voxel_count() == voxels,
            "an object colocalization tile carries both channels voxel for voxel"
        );
        for (index, label) in labels.iter().enumerate() {
            if *label == 0 {
                continue;
            }
            let object = (*label - 1) as usize;
            assert!(
                object < objects,
                "an object colocalization label names an object of its set"
            );
            let base = object * OBJECT_COLOC_FIELDS;
            if self.sums[base] == 0.0 {
                self.touched.push(*label);
            }
            let first = unit(scales[0], a.value(index));
            let second = unit(scales[1], b.value(index));
            let [first, second] = [first, second].map(f64::from);
            self.sums[base] += 1.0;
            self.sums[base + 1] += first;
            self.sums[base + 2] += second;
            self.sums[base + 3] += first * first;
            self.sums[base + 4] += second * second;
            self.sums[base + 5] += first * second;
        }
        Ok(self.take())
    }

    fn take(&mut self) -> Vec<(u16, [f64; OBJECT_COLOC_FIELDS])> {
        self.touched.sort_unstable();
        let mut drained = Vec::with_capacity(self.touched.len());
        for label in self.touched.drain(..) {
            let base = (label as usize - 1) * OBJECT_COLOC_FIELDS;
            let mut values = [0.0; OBJECT_COLOC_FIELDS];
            values.copy_from_slice(&self.sums[base..base + OBJECT_COLOC_FIELDS]);
            self.sums[base..base + OBJECT_COLOC_FIELDS].fill(0.0);
            drained.push((label, values));
        }
        drained
    }
}

fn merge(totals: &mut [f64], values: &[(u16, [f64; OBJECT_COLOC_FIELDS])]) {
    for (label, sums) in values {
        let base = (*label as usize - 1) * OBJECT_COLOC_FIELDS;
        for (target, value) in totals[base..base + OBJECT_COLOC_FIELDS]
            .iter_mut()
            .zip(sums)
        {
            *target += value;
        }
    }
}

fn object_values(totals: &[f64]) -> Vec<ObjectColocValue> {
    totals
        .as_chunks::<OBJECT_COLOC_FIELDS>()
        .0
        .iter()
        .map(|fields| {
            ObjectColocValue::new(
                fields[0] as u64,
                [fields[1], fields[2], fields[3], fields[4], fields[5]],
            )
        })
        .collect()
}
