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
            view_words,
            info,
            settled,
            pin,
            prefetch,
            prefetched,
            prefetch_key,
            prefetch_issued,
            prefetch_rate,
            wanted,
            wanted_level,
            section_measure,
            section_sequence,
            ..
        } = self;
        let pyramid = pyramid.as_ref()?;
        let planner = planner.as_ref()?;
        let atlas = atlas.as_mut()?;
        let words = feedback_words(bytes);
        if words.len() < (BAND + 1) * *view_words {
            return None;
        }
        let ladder = meta.levels.ladder;
        let mut demands = Vec::with_capacity(ladder.count);
        for band in 0..ladder.count {
            let slice = &words[band * *view_words..(band + 1) * *view_words];
            demands.push(decode_plan(
                slice,
                planner.bands[band].capacity(),
                meta.time,
                ladder.level(band),
                pyramid,
            )?);
        }
        let section = decode_plan(
            &words[BAND * *view_words..(BAND + 1) * *view_words],
            planner.section.capacity(),
            meta.time,
            meta.levels.section,
            pyramid,
        )?;
        *pin += 1;
        atlas.clear_holds();
        for (band, demand) in demands.iter().enumerate() {
            atlas.touch(
                meta.time,
                ladder.level(band),
                demand.indices.iter().copied(),
                *pin,
            );
            wanted[band] = demand.info.visible;
        }
        wanted[ladder.count..].fill(0);
        *wanted_level = Some(ladder.base);
        atlas.touch(
            meta.time,
            meta.levels.section,
            section.indices.iter().copied(),
            *pin,
        );
        info.section = section.info;
        *section_sequence += 1;
        *section_measure = Some((
            *section_sequence,
            SectionMeasure {
                level: meta.levels.section,
                visible: section.info.visible,
            },
        ));
        let mut demanded = HashSet::new();
        for (band, demand) in demands.iter().enumerate() {
            let level = ladder.level(band);
            demanded.extend(demand.indices.iter().map(|index| (level, *index)));
        }
        demanded.extend(
            section
                .indices
                .iter()
                .map(|index| (meta.levels.section, *index)),
        );
        if !prefetch_issued.is_empty() {
            let issued = prefetch_issued
                .iter()
                .filter(|request| request.time == meta.time)
                .count();
            let hit = prefetch_issued
                .iter()
                .filter(|request| {
                    request.time == meta.time
                        && demanded.contains(&(request.level, index_of(request, pyramid)))
                })
                .count();
            if issued > 0 {
                let rate = hit as f32 / issued as f32;
                *prefetch_rate = 0.7 * *prefetch_rate + 0.3 * rate.clamp(0.25, 1.0);
            }
        }
        prefetch_issued.clear();
        let key = view_key(&meta);
        if *prefetch_key != Some(key) {
            *prefetch_key = Some(key);
            prefetched.clear();
        }
        let issued = prefetch_requests(
            &Prefetch {
                enabled: *prefetch,
                rate: *prefetch_rate,
                atlas,
                time: meta.time,
                ladder,
                demands: &demands,
                section: &section,
                section_level: meta.levels.section,
                section_fallback: meta.levels.section_fallback,
                pyramid,
            },
            prefetched,
        );
        prefetch_issued.extend_from_slice(&issued);
        let complete = demands.iter().all(|demand| {
            demand.requests.is_empty()
                && demand.info.resident == demand.info.demand
                && (!meta.assured || demand.info.visible == demand.info.demand)
        }) && section.requests.is_empty()
            && section.info.resident == section.info.demand
            && (!meta.assured || section.info.visible == section.info.demand);
        let settled_now = complete && (!meta.assured || issued.is_empty());
        *settled = settled_now.then_some(meta);
        let mut merged = Demand::default();
        for demand in demands {
            merged.merge(demand);
        }
        for request in merged.requests.iter().chain(&section.requests) {
            prefetched.remove(request);
        }
        merged.info.level = ladder.base;
        info.volume = merged.info;
        Some(Requests {
            time: meta.time,
            volume: merged.requests,
            section: section.requests,
            prefetch: issued,
            volume_view: merged.info,
            section_view: section.info,
        })
    }
}

fn index_of(request: &BrickRequest, pyramid: &Pyramid) -> u32 {
    let grid = BrickGrid::new(pyramid.level(request.level).extent());
    grid.index(request.coord) as u32
}

pub(super) fn view_key(meta: &Residency) -> u64 {
    let mut hasher = DefaultHasher::new();
    meta.epoch.hash(&mut hasher);
    meta.style.hash(&mut hasher);
    meta.prefetch.hash(&mut hasher);
    meta.assured.hash(&mut hasher);
    meta.time.hash(&mut hasher);
    meta.eye.map(f32::to_bits).hash(&mut hasher);
    meta.focus.map(f32::to_bits).hash(&mut hasher);
    meta.field_of_view.to_bits().hash(&mut hasher);
    meta.section_center.map(f32::to_bits).hash(&mut hasher);
    meta.section_normal.map(f32::to_bits).hash(&mut hasher);
    meta.section_right.map(f32::to_bits).hash(&mut hasher);
    meta.span.to_bits().hash(&mut hasher);
    meta.volume.hash(&mut hasher);
    meta.section.hash(&mut hasher);
    meta.levels.ladder.hash(&mut hasher);
    meta.levels.section.hash(&mut hasher);
    meta.levels.section_fallback.hash(&mut hasher);
    hasher.finish()
}

pub(super) struct Prefetch<'a> {
    pub(super) enabled: bool,
    pub(super) rate: f32,
    pub(super) atlas: &'a Atlas,
    pub(super) time: u32,
    pub(super) ladder: Ladder,
    pub(super) demands: &'a [Demand],
    pub(super) section: &'a Demand,
    pub(super) section_level: usize,
    pub(super) section_fallback: usize,
    pub(super) pyramid: &'a Pyramid,
}

pub(super) fn prefetch_requests(
    prefetch: &Prefetch<'_>,
    issued: &mut HashSet<BrickRequest>,
) -> Vec<BrickRequest> {
    let Prefetch {
        enabled,
        rate,
        atlas,
        time,
        ladder,
        demands,
        section,
        section_level,
        section_fallback,
        pyramid,
    } = *prefetch;
    let mut requests = Vec::new();
    let total = ((atlas.slots() as f32 / PREFETCH_SHARE as f32) * rate.clamp(0.25, 1.0))
        .min(PREFETCH_LIMIT as f32) as usize;
    let mut covering = total;
    cover_level(
        pyramid,
        atlas,
        time,
        section_level,
        section_fallback,
        &section.indices,
        &mut requests,
        &mut covering,
        issued,
    );
    if !enabled {
        return requests;
    }
    let next = time + 1;
    if next >= pyramid.frame_count() {
        return requests;
    }
    let mut budget = total.saturating_sub(requests.len());
    for (band, demand) in demands.iter().enumerate() {
        if budget == 0 {
            break;
        }
        carry(
            pyramid,
            atlas,
            next,
            ladder.level(band),
            &demand.indices,
            &mut requests,
            &mut budget,
            issued,
        );
    }
    if budget > 0 {
        carry(
            pyramid,
            atlas,
            next,
            section_level,
            &section.indices,
            &mut requests,
            &mut budget,
            issued,
        );
    }
    requests
}

#[allow(clippy::too_many_arguments)]
fn carry(
    pyramid: &Pyramid,
    atlas: &Atlas,
    time: u32,
    level: usize,
    indices: &[u32],
    requests: &mut Vec<BrickRequest>,
    budget: &mut usize,
    issued: &mut HashSet<BrickRequest>,
) {
    if *budget == 0 || indices.is_empty() {
        return;
    }
    let grid = BrickGrid::new(pyramid.level(level).extent());
    for index in indices {
        if *budget == 0 {
            return;
        }
        if atlas.slot(time, level, *index as usize).is_some()
            || atlas.is_hollow(time, level, *index as usize)
        {
            continue;
        }
        let request = BrickRequest {
            time,
            level,
            coord: grid.coord(*index as usize),
        };
        if !issued.insert(request) {
            continue;
        }
        requests.push(request);
        *budget -= 1;
    }
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
