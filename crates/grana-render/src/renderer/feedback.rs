use super::*;

impl Renderer {
    pub fn poll_requests(&mut self, device: &Device) -> Option<Requests> {
        let mut latest = None;
        while let Some((slot, bytes)) = self.take_feedback(device) {
            let Some(meta) = self.frames[slot].take() else {
                continue;
            };
            if let Some(requests) = self.decode(meta, &bytes) {
                latest = Some(requests);
            }
        }
        let mut dropped = None;
        while let Some((_, bytes)) = self.oit_readback.take(device) {
            let (words, _) = bytes.as_chunks::<4>();
            if let Some(word) = words.first() {
                dropped = Some(u32::from_ne_bytes(*word));
            }
        }
        if let Some(dropped) = dropped {
            self.dropped = dropped;
        }
        latest
    }

    pub(super) fn take_feedback(&mut self, device: &Device) -> Option<(usize, Vec<u8>)> {
        self.feedback
            .as_mut()
            .and_then(|feedback| feedback.take(device))
    }

    pub fn requests_pending(&self) -> bool {
        self.feedback.as_ref().is_some_and(Readback::pending)
    }

    pub(super) fn decode(&mut self, meta: Residency, bytes: &[u8]) -> Option<Requests> {
        if meta.epoch != self.epoch {
            return None;
        }
        let Self {
            pyramid,
            planner,
            atlas,
            counts,
            view_words,
            info,
            settled,
            pin,
            prefetch,
            prefetched,
            prefetch_key,
            ..
        } = self;
        let pyramid = pyramid.as_ref()?;
        let planner = planner.as_ref()?;
        let atlas = atlas.as_mut()?;
        let words = feedback_words(bytes);
        if words.len() < 2 * *view_words + CENSUS_LEVELS {
            return None;
        }
        let (volume_words, rest) = words.split_at(*view_words);
        let (section_words, census_words) = rest.split_at(*view_words);
        let volume = decode_plan(
            volume_words,
            planner.volume.capacity(),
            meta.time,
            meta.levels.volume,
            pyramid,
        )?;
        let section = decode_plan(
            section_words,
            planner.section.capacity(),
            meta.time,
            meta.levels.section,
            pyramid,
        )?;
        *pin += 1;
        atlas.touch(
            meta.time,
            meta.levels.volume,
            volume.indices.iter().copied(),
            *pin,
        );
        atlas.touch(
            meta.time,
            meta.levels.section,
            section.indices.iter().copied(),
            *pin,
        );
        counts.copy_from_slice(census_words);
        info.volume = volume.info;
        info.section = section.info;
        let settled_now = volume.requests.is_empty()
            && section.requests.is_empty()
            && volume.info.resident == volume.info.demand
            && section.info.resident == section.info.demand;
        *settled = settled_now.then_some(meta);
        let key = view_key(&meta);
        if *prefetch_key != Some(key) {
            *prefetch_key = Some(key);
            prefetched.clear();
        }
        let prefetched = prefetch_requests(
            &Prefetch {
                enabled: *prefetch,
                atlas,
                time: meta.time,
                levels: meta.levels,
                volume: &volume,
                section: &section,
                pyramid,
            },
            prefetched,
        );
        Some(Requests {
            time: meta.time,
            volume: volume.requests,
            section: section.requests,
            prefetch: prefetched,
            volume_view: volume.info,
            section_view: section.info,
        })
    }
}

pub(super) fn view_key(meta: &Residency) -> u64 {
    let mut hasher = DefaultHasher::new();
    meta.epoch.hash(&mut hasher);
    meta.style.hash(&mut hasher);
    meta.prefetch.hash(&mut hasher);
    meta.time.hash(&mut hasher);
    meta.eye.map(f32::to_bits).hash(&mut hasher);
    meta.focus.map(f32::to_bits).hash(&mut hasher);
    meta.field_of_view.to_bits().hash(&mut hasher);
    meta.axis.hash(&mut hasher);
    meta.span.to_bits().hash(&mut hasher);
    meta.volume.hash(&mut hasher);
    meta.section.hash(&mut hasher);
    meta.levels.volume.hash(&mut hasher);
    meta.levels.section.hash(&mut hasher);
    meta.levels.volume_fallback.hash(&mut hasher);
    meta.levels.section_fallback.hash(&mut hasher);
    hasher.finish()
}

pub(super) struct Prefetch<'a> {
    pub(super) enabled: bool,
    pub(super) atlas: &'a Atlas,
    pub(super) time: u32,
    pub(super) levels: Levels,
    pub(super) volume: &'a Demand,
    pub(super) section: &'a Demand,
    pub(super) pyramid: &'a Pyramid,
}

pub(super) fn prefetch_requests(
    prefetch: &Prefetch<'_>,
    issued: &mut HashSet<BrickRequest>,
) -> Vec<BrickRequest> {
    let Prefetch {
        enabled,
        atlas,
        time,
        levels,
        volume,
        section,
        pyramid,
    } = *prefetch;
    let mut requests = Vec::new();
    let mut budget = (atlas.slots() / PREFETCH_SHARE).min(PREFETCH_LIMIT);
    for (demand, level, fallback) in [
        (volume, levels.volume, levels.volume_fallback),
        (section, levels.section, levels.section_fallback),
    ] {
        cover_level(
            pyramid,
            atlas,
            time,
            level,
            fallback,
            &demand.indices,
            &mut requests,
            &mut budget,
            issued,
        );
        if budget == 0 {
            break;
        }
    }
    if !enabled {
        return requests;
    }
    let next = time + 1;
    if next >= pyramid.frame_count() {
        return requests;
    }
    for (demand, level) in [(volume, levels.volume), (section, levels.section)] {
        if budget == 0 {
            break;
        }
        let grid = BrickGrid::new(pyramid.level(level).extent());
        for index in &demand.indices {
            if budget == 0 {
                break;
            }
            if atlas.slot(next, level, *index as usize).is_some()
                || atlas.is_hollow(next, level, *index as usize)
            {
                continue;
            }
            let request = BrickRequest {
                time: next,
                level,
                coord: grid.coord(*index as usize),
            };
            if !issued.insert(request) {
                continue;
            }
            requests.push(request);
            budget -= 1;
        }
    }
    requests
}

#[allow(clippy::too_many_arguments)]
fn cover_level(
    pyramid: &Pyramid,
    atlas: &Atlas,
    time: u32,
    source: usize,
    target: usize,
    indices: &[u32],
    requests: &mut Vec<BrickRequest>,
    budget: &mut usize,
    issued: &mut HashSet<BrickRequest>,
) {
    if source == target || *budget == 0 {
        return;
    }
    let source_grid = BrickGrid::new(pyramid.level(source).extent());
    let target_grid = BrickGrid::new(pyramid.level(target).extent());
    let source_voxel = pyramid.level(source).voxel();
    let target_voxel = pyramid.level(target).voxel();
    let counts = target_grid.counts();
    let mut seen = HashSet::new();
    for index in indices {
        if *budget == 0 {
            return;
        }
        let coord = source_grid.coord(*index as usize);
        let origin = source_grid.origin(coord);
        let extent = source_grid.extent_of(coord);
        let dimensions = [extent.x(), extent.y(), extent.z()];
        let mut first = [0u32; 3];
        let mut last = [0u32; 3];
        for axis in 0..3 {
            let ratio = f64::from(source_voxel[axis]) / f64::from(target_voxel[axis]);
            let start = f64::from(origin[axis]) * ratio;
            let end = f64::from(origin[axis] + dimensions[axis]) * ratio;
            first[axis] = (start / f64::from(BRICK_SIDE)).floor() as u32;
            last[axis] = (((end - 1.0) / f64::from(BRICK_SIDE)).floor() as u32)
                .min(counts[axis].saturating_sub(1));
        }
        for z in first[2]..=last[2] {
            for y in first[1]..=last[1] {
                for x in first[0]..=last[0] {
                    let coord = BrickCoord::new(x, y, z);
                    let index = target_grid.index(coord);
                    let request = BrickRequest {
                        time,
                        level: target,
                        coord,
                    };
                    if !seen.insert(index)
                        || atlas.slot(time, target, index).is_some()
                        || atlas.is_hollow(time, target, index)
                        || !issued.insert(request)
                    {
                        continue;
                    }
                    requests.push(request);
                    *budget -= 1;
                    if *budget == 0 {
                        return;
                    }
                }
            }
        }
    }
}

pub(super) fn feedback_words(bytes: &[u8]) -> Vec<u32> {
    let (words, _) = bytes.as_chunks::<4>();
    words.iter().map(|word| u32::from_ne_bytes(*word)).collect()
}
