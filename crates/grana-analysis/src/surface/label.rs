use std::ops::Range;

use grana_volume::VoxelData;

use super::{EMPTY, Face, SurfaceBuilder, find, length, parabola, strides_of};

impl SurfaceBuilder {
    pub(super) fn flood(&mut self) {
        let dims = self.dims;
        let strides = strides_of(dims);
        let count = dims[0] * dims[1] * dims[2];
        for start in 0..count {
            if !self.mask[start] || self.labels[start] != EMPTY {
                continue;
            }
            let node = self.create_node(0);
            self.labels[start] = node;
            self.stack.clear();
            self.stack.push(start as u32);
            while let Some(cell) = self.stack.pop() {
                let cell = cell as usize;
                for axis in 0..3 {
                    let coordinate = (cell / strides[axis]) % dims[axis];
                    for step in [-1i64, 1] {
                        let reached = coordinate as i64 + step;
                        if reached < 0 || reached >= dims[axis] as i64 {
                            continue;
                        }
                        let neighbour = (cell as i64 + step * strides[axis] as i64) as usize;
                        if self.mask[neighbour] && self.labels[neighbour] == EMPTY {
                            self.labels[neighbour] = node;
                            self.stack.push(neighbour as u32);
                        }
                    }
                }
            }
        }
    }

    pub(super) fn watershed(&mut self) {
        self.fill_distance();
        let dims = self.dims;
        let strides = strides_of(dims);
        let count = dims[0] * dims[1] * dims[2];
        let cap = self.distance_cap as usize;
        let mut counts = vec![0u32; cap + 1];
        for index in 0..count {
            if self.mask[index] {
                counts[self.chamfer[index] as usize] += 1;
            }
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
                self.labels[index] = self.create_node(u32::from(level));
            }
        }
    }

    fn fill_distance(&mut self) {
        let dims = self.dims;
        let strides = strides_of(dims);
        let count = dims[0] * dims[1] * dims[2];
        let cap = self.distance_cap;
        self.chamfer.clear();
        self.chamfer.resize(count, 0);
        for index in 0..count {
            self.chamfer[index] = if self.mask[index] { cap } else { 0 };
        }
        let anisotropy = self.anisotropy;
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
                            value.min(f64::from(cap)) as u16;
                    }
                }
            }
        }
    }

    pub(super) fn record_necks(&mut self) {
        let dims = self.dims;
        let strides = strides_of(dims);
        let mut pairs: std::collections::HashMap<(u32, u32), u16> =
            std::collections::HashMap::new();
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
        mut measure: impl FnMut(usize, &[Range<u32>; 3]) -> Result<VoxelData, String>,
    ) -> Result<(), String> {
        let dims = self.dims;
        let core_dims = super::dims_of(core);
        let core_count = core_dims[0] * core_dims[1] * core_dims[2];
        let origin = [
            (core[0].start - array[0].start) as usize,
            (core[1].start - array[1].start) as usize,
            (core[2].start - array[2].start) as usize,
        ];
        let mut measured = Vec::with_capacity(self.channels);
        for channel in 0..self.channels {
            if channel == self.detection {
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
                    let node = find(&mut self.nodes, self.labels[index]) as usize;
                    let local = x + core_dims[0] * (y + core_dims[1] * z);
                    for (channel, data) in measured.iter().enumerate() {
                        let value = match data {
                            Some(data) => data.value(local),
                            None => self.raw[index],
                        };
                        self.accumulators[node * self.channels + channel].push(value);
                    }
                    let voxel = [
                        core[0].start + x as u32,
                        core[1].start + y as u32,
                        core[2].start + z as u32,
                    ];
                    let stats = &mut self.nodes[node];
                    stats.voxels += 1;
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

    pub(super) fn join(&mut self) {
        let [x, y, z] = self.tile_position(self.next);
        let strides = strides_of(self.dims);

        if x > 0
            && let Some(face) = self.x_face.take()
        {
            let plane = self.join_plane(0);
            self.join_face(&face, 0, plane, strides);
        }
        if y > 0
            && let Some(face) = self.y_faces[x as usize].take()
        {
            let plane = self.join_plane(1);
            self.join_face(&face, 1, plane, strides);
        }
        if z > 0 {
            let slot = y as usize * self.tiles[0] as usize + x as usize;
            if let Some(face) = self.z_faces[slot].take() {
                let plane = self.join_plane(2);
                self.join_face(&face, 2, plane, strides);
            }
        }
    }

    fn join_plane(&self, axis: usize) -> usize {
        let core = self.core_range(self.next);
        let array = self.array_range();
        (core[axis].start - array[axis].start) as usize
    }

    fn join_face(&mut self, face: &Face, axis: usize, plane: usize, strides: [usize; 3]) {
        let array = self.array_range();
        let first = (axis + 1) % 3;
        let second = (axis + 2) % 3;
        let start = [
            face.origin[0].max(array[first].start),
            face.origin[1].max(array[second].start),
        ];
        let end = [
            (face.origin[0] + face.dims[0]).min(array[first].end),
            (face.origin[1] + face.dims[1]).min(array[second].end),
        ];
        if start[0] >= end[0] || start[1] >= end[1] {
            return;
        }
        for second_position in start[1]..end[1] {
            for first_position in start[0]..end[0] {
                let cached = face.labels[(first_position - face.origin[0]) as usize
                    + face.dims[0] as usize * (second_position - face.origin[1]) as usize];
                if cached == EMPTY {
                    continue;
                }
                let index = strides[axis] * plane
                    + strides[first] * (first_position - array[first].start) as usize
                    + strides[second] * (second_position - array[second].start) as usize;
                let local = self.labels[index];
                if local == EMPTY {
                    continue;
                }

                self.union(local, cached);
            }
        }
    }

    pub(super) fn store_faces(&mut self, core: &[Range<u32>; 3], array: &[Range<u32>; 3]) {
        let [x, y, _] = self.tile_position(self.next);
        if core[0].end < array[0].end {
            self.x_face = Some(self.slice(0, (core[0].end - array[0].start) as usize));
        }
        if core[1].end < array[1].end {
            self.y_faces[x as usize] = Some(self.slice(1, (core[1].end - array[1].start) as usize));
        }
        if core[2].end < array[2].end {
            let slot = y as usize * self.tiles[0] as usize + x as usize;
            self.z_faces[slot] = Some(self.slice(2, (core[2].end - array[2].start) as usize));
        }
    }

    fn slice(&self, axis: usize, plane: usize) -> Face {
        let dims = self.dims;
        let strides = strides_of(dims);
        let array = self.array_range();
        let first = (axis + 1) % 3;
        let second = (axis + 2) % 3;
        let mut labels = Vec::with_capacity(dims[first] * dims[second]);
        for second_position in 0..dims[second] {
            for first_position in 0..dims[first] {
                labels.push(
                    self.labels[strides[axis] * plane
                        + strides[first] * first_position
                        + strides[second] * second_position],
                );
            }
        }
        Face {
            origin: [array[first].start, array[second].start],
            dims: [dims[first] as u32, dims[second] as u32],
            labels,
        }
    }

    pub(super) fn merge_basins(&mut self) {
        let mut seams: std::collections::HashMap<(u32, u32), u16> =
            std::collections::HashMap::new();
        for (left, right, neck) in std::mem::take(&mut self.necks) {
            let left = find(&mut self.nodes, left);
            let right = find(&mut self.nodes, right);
            if left == right {
                continue;
            }
            let key = (left.min(right), left.max(right));
            let slot = seams.entry(key).or_insert(0);
            *slot = (*slot).max(neck);
        }
        let mut seams = seams.into_iter().collect::<Vec<_>>();
        seams.sort_unstable_by(|(left_key, left_neck), (right_key, right_neck)| {
            right_neck.cmp(left_neck).then(left_key.cmp(right_key))
        });
        let gap = self.merge_gap;
        for ((left, right), neck) in seams {
            let left = find(&mut self.nodes, left);
            let right = find(&mut self.nodes, right);
            if left == right {
                continue;
            }
            let reach = length(self.nodes[left as usize].reach)
                .min(length(self.nodes[right as usize].reach));
            let drop = i64::from(reach) - i64::from(length(u32::from(neck)));
            if drop < i64::from(gap) {
                self.union(left, right);
            }
        }
    }
}
