use std::collections::HashMap;
use std::ops::Range;

use super::{EMPTY, Measure, Tile, find, parabola, strides_of};

impl Tile<'_> {
    pub(super) fn watershed(&mut self) -> bool {
        self.fill_distance();
        let dims = self.dims;
        let strides = strides_of(dims);
        let count = dims[0] * dims[1] * dims[2];
        let cap = self.pass.cap as usize;
        let mut counts = vec![0u32; cap + 1];
        let mut saturated = false;
        for index in 0..count {
            if !self.mask[index] {
                continue;
            }
            let bucket = self.chamfer[index] as usize;
            saturated |= bucket == cap;
            counts[bucket] += 1;
        }
        let mut cursor = vec![0u32; cap + 1];
        let mut total = 0u32;
        for bucket in (0..=cap).rev() {
            cursor[bucket] = total;
            total += counts[bucket];
        }
        self.order.clear();
        self.order.resize(total as usize, 0);
        for index in 0..count {
            if !self.mask[index] {
                continue;
            }
            let bucket = self.chamfer[index] as usize;
            self.order[cursor[bucket] as usize] = index as u32;
            cursor[bucket] += 1;
        }
        for position in 0..self.order.len() {
            let index = self.order[position] as usize;
            let level = self.chamfer[index];
            let mut best = EMPTY;
            for axis in 0..3 {
                let coordinate = (index / strides[axis]) % dims[axis];
                for step in [-1i64, 1] {
                    let reached = coordinate as i64 + step;
                    if reached < 0 || reached >= dims[axis] as i64 {
                        continue;
                    }
                    let neighbour = (index as i64 + step * strides[axis] as i64) as usize;
                    if !self.mask[neighbour] {
                        continue;
                    }
                    let compete = best == EMPTY
                        || self.chamfer[neighbour] > self.chamfer[best as usize]
                        || (self.chamfer[neighbour] == self.chamfer[best as usize]
                            && neighbour < best as usize);
                    if compete {
                        best = neighbour as u32;
                    }
                }
            }
            if best != EMPTY
                && (self.chamfer[best as usize] > level
                    || (self.chamfer[best as usize] == level && (best as usize) < index))
            {
                assert!(
                    self.labels[best as usize] != EMPTY,
                    "the successor of a watershed voxel is labelled"
                );
                self.labels[index] = self.labels[best as usize];
            } else {
                self.labels[index] = self.create_node(0);
            }
        }
        saturated
    }

    fn fill_distance(&mut self) {
        let plan = self.pass.plan;
        let dims = self.dims;
        let strides = strides_of(dims);
        let count = dims[0] * dims[1] * dims[2];
        let cap = self.pass.cap;
        self.chamfer.clear();
        self.chamfer.resize(count, 0);
        for index in 0..count {
            self.chamfer[index] = if self.mask[index] { cap } else { 0 };
        }
        let anisotropy = plan.anisotropy;
        for axis in 0..3 {
            let weight = f64::from(anisotropy[axis] * anisotropy[axis]);
            let (first, second) = ((axis + 1) % 3, (axis + 2) % 3);
            let length = dims[axis];
            let mut line = vec![0.0f64; length];
            for position in 0..dims[second] {
                for row in 0..dims[first] {
                    let base = strides[first] * row + strides[second] * position;
                    for (step, slot) in line.iter_mut().enumerate() {
                        *slot = f64::from(self.chamfer[base + strides[axis] * step]);
                    }
                    parabola(&mut line, weight);
                    for (step, value) in line.iter().enumerate() {
                        self.chamfer[base + strides[axis] * step] =
                            value.min(f64::from(cap)) as u32;
                    }
                }
            }
        }
    }

    pub(super) fn record_necks(&mut self) {
        let dims = self.dims;
        let strides = strides_of(dims);
        let mut pairs: HashMap<(u32, u32), u32> = HashMap::new();
        for z in 0..dims[2] {
            for y in 0..dims[1] {
                for x in 0..dims[0] {
                    let index = x + dims[0] * (y + dims[1] * z);
                    if !self.mask[index] {
                        continue;
                    }
                    for axis in 0..3 {
                        if [x, y, z][axis] + 1 >= dims[axis] {
                            continue;
                        }
                        let neighbour = index + strides[axis];
                        if !self.mask[neighbour] {
                            continue;
                        }
                        let left = find(&mut self.nodes, self.labels[index]);
                        let right = find(&mut self.nodes, self.labels[neighbour]);
                        if left == right {
                            continue;
                        }
                        let key = (left.min(right), left.max(right));
                        let neck = self.chamfer[index].min(self.chamfer[neighbour]);
                        let slot = pairs.entry(key).or_insert(0);
                        *slot = (*slot).max(neck);
                    }
                }
            }
        }
        let mut keys = pairs.keys().copied().collect::<Vec<_>>();
        keys.sort_unstable();
        for key in keys {
            self.necks.push((key.0, key.1, pairs[&key]));
        }
    }

    pub(super) fn accumulate(
        &mut self,
        core: &[Range<u32>; 3],
        array: &[Range<u32>; 3],
        measure: &Measure<'_>,
    ) -> Result<(), String> {
        let plan = self.pass.plan;
        let dims = self.dims;
        let core_dims = super::dims_of(core);
        let core_count = core_dims[0] * core_dims[1] * core_dims[2];
        let origin = [
            (core[0].start - array[0].start) as usize,
            (core[1].start - array[1].start) as usize,
            (core[2].start - array[2].start) as usize,
        ];
        let channels = plan.channels;
        let value_dims = self.value_dims;
        let value_start = self.value_start;
        let mut measured = Vec::with_capacity(channels);
        for channel in 0..channels {
            if channel == plan.detection {
                measured.push(None);
                continue;
            }
            let data = measure(channel, core)?;
            if data.voxel_count() != core_count {
                return Err("the measured tile does not cover the requested region".to_string());
            }
            measured.push(Some(data));
        }
        for z in 0..core_dims[2] {
            for y in 0..core_dims[1] {
                for x in 0..core_dims[0] {
                    let index =
                        (origin[0] + x) + dims[0] * ((origin[1] + y) + dims[1] * (origin[2] + z));
                    if !self.mask[index] {
                        continue;
                    }
                    let node = self.labels[index] as usize;
                    assert!(
                        node < self.nodes.len(),
                        "the accumulated voxel belongs to a tile node"
                    );
                    let value_slot = (core[0].start as i64 + x as i64 - value_start[0]) as usize
                        + value_dims[0]
                            * ((core[1].start as i64 + y as i64 - value_start[1]) as usize
                                + value_dims[1]
                                    * ((core[2].start as i64 + z as i64 - value_start[2])
                                        as usize));
                    let local = x + core_dims[0] * (y + core_dims[1] * z);
                    for (channel, data) in measured.iter().enumerate() {
                        let value = match data {
                            Some(data) => data.value(local),
                            None => self.raw[value_slot],
                        };
                        self.accumulators[node * channels + channel].push(value);
                    }
                    let voxel = [
                        core[0].start + x as u32,
                        core[1].start + y as u32,
                        core[2].start + z as u32,
                    ];
                    let reach = self.chamfer.get(index).copied().unwrap_or(0);
                    let stats = &mut self.nodes[node];
                    stats.voxels += 1;
                    stats.reach = stats.reach.max(reach);
                    for (axis, coordinate) in voxel.iter().enumerate() {
                        stats.sum[axis] += f64::from(*coordinate);
                        stats.low[axis] = stats.low[axis].min(*coordinate);
                        stats.high[axis] = stats.high[axis].max(*coordinate);
                    }
                }
            }
        }
        Ok(())
    }
}
