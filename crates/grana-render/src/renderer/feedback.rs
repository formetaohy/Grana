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
        let mut dropped = [None; PANE_COUNT];
        for (index, readback) in self.oit_readback.iter_mut().enumerate() {
            let mut last = None;
            while let Some((_, bytes)) = readback.take(device) {
                let (words, _) = bytes.as_chunks::<4>();
                if let Some(word) = words.first() {
                    last = Some(u32::from_ne_bytes(*word));
                }
            }
            dropped[index] = last;
        }
        for (slot, value) in dropped.iter().enumerate() {
            if let Some(value) = value {
                self.dropped[slot] = *value;
            }
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
            section_measure,
            section_sequence,
            volume_measure,
            ..
        } = self;
        let pyramid = pyramid.as_ref()?;
        let planner = planner.as_ref()?;
        let atlas = atlas.as_mut()?;
        let words = feedback_words(bytes);
        if words.len() < PANE_COUNT * (BAND + 1) * *view_words {
            return None;
        }
        *pin += 1;
        atlas.clear_holds();
        let mut requests = Requests {
            time: meta.time,
            ..Default::default()
        };
        let mut demanded = HashSet::new();
        let mut issued = Vec::new();
        let mut complete = true;
        for (index, pane) in meta.panes.iter().enumerate() {
            let Some(pane) = pane else {
                continue;
            };
            let base = index * (BAND + 1) * *view_words;
            match pane {
                PaneResidency::Volume { levels, .. } => {
                    let ladder = levels.ladder;
                    let mut demands = Vec::with_capacity(ladder.count);
                    for band in 0..ladder.count {
                        let slice =
                            &words[base + band * *view_words..base + (band + 1) * *view_words];
                        let held = decode_plan(
                            slice,
                            planner.panes[index].bands[band].capacity(),
                            meta.time,
                            ladder.level(band),
                            pyramid,
                        )?;
                        demands.push(held);
                    }
                    for (band, demand) in demands.iter().enumerate() {
                        if let Some(slot) = volume_measure[index].get_mut(ladder.level(band)) {
                            *slot = Some(demand.info.visible);
                        }
                        atlas.touch(
                            meta.time,
                            ladder.level(band),
                            demand.indices.iter().copied(),
                            *pin,
                        );
                    }
                    let empty = Demand::default();
                    issued.extend(prefetch_requests(
                        &Prefetch {
                            enabled: *prefetch,
                            rate: *prefetch_rate,
                            atlas,
                            time: meta.time,
                            ladder,
                            demands: &demands,
                            section: &empty,
                            section_level: 0,
                            section_fallback: 0,
                            pyramid,
                        },
                        prefetched,
                    ));
                    for (band, demand) in demands.iter().enumerate() {
                        let level = ladder.level(band);
                        demanded.extend(demand.indices.iter().map(|index| (level, *index)));
                        complete &= demands_complete(demand, meta.assured);
                    }
                    let mut merged = Demand::default();
                    for demand in demands {
                        merged.merge(demand);
                    }
                    merged.info.level = ladder.base;
                    info.panes[index] = merged.info;
                    requests.panes[index] = merged.info;
                    requests.volume.extend(merged.requests.iter().copied());
                    for request in &merged.requests {
                        prefetched.remove(request);
                    }
                }
                PaneResidency::Section { levels, .. } => {
                    let slice = &words[base + BAND * *view_words..base + (BAND + 1) * *view_words];
                    let demand = decode_plan(
                        slice,
                        planner.panes[index].section.capacity(),
                        meta.time,
                        levels.level,
                        pyramid,
                    )?;
                    atlas.touch(
                        meta.time,
                        levels.level,
                        demand.indices.iter().copied(),
                        *pin,
                    );
                    *section_sequence += 1;
                    section_measure[index] = Some((
                        *section_sequence,
                        SectionMeasure {
                            level: levels.level,
                            visible: demand.info.visible,
                        },
                    ));
                    demanded.extend(demand.indices.iter().map(|index| (levels.level, *index)));
                    complete &= demands_complete(&demand, meta.assured);
                    issued.extend(prefetch_requests(
                        &Prefetch {
                            enabled: *prefetch,
                            rate: *prefetch_rate,
                            atlas,
                            time: meta.time,
                            ladder: Ladder { base: 0, count: 0 },
                            demands: &[],
                            section: &demand,
                            section_level: levels.level,
                            section_fallback: levels.fallback,
                            pyramid,
                        },
                        prefetched,
                    ));
                    info.panes[index] = demand.info;
                    requests.panes[index] = demand.info;
                    requests.section.extend(demand.requests.iter().copied());
                    for request in &demand.requests {
                        prefetched.remove(request);
                    }
                }
            }
        }
        if !prefetch_issued.is_empty() {
            let issued_count = prefetch_issued
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
            if issued_count > 0 {
                let rate = hit as f32 / issued_count as f32;
                *prefetch_rate = 0.7 * *prefetch_rate + 0.3 * rate.clamp(0.25, 1.0);
            }
        }
        prefetch_issued.clear();
        let key = view_key(&meta);
        if *prefetch_key != Some(key) {
            *prefetch_key = Some(key);
            prefetched.clear();
        }
        prefetch_issued.extend_from_slice(&issued);
        let settled_now = complete && (!meta.assured || issued.is_empty());
        *settled = settled_now.then_some(meta);
        requests.prefetch = issued;
        Some(requests)
    }
}

fn demands_complete(demand: &Demand, assured: bool) -> bool {
    demand.requests.is_empty()
        && demand.info.resident == demand.info.demand
        && (!assured || demand.info.visible == demand.info.demand)
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
    for pane in meta.panes.iter().flatten() {
        match pane {
            PaneResidency::Volume {
                eye,
                focus,
                field_of_view,
                size,
                levels,
            } => {
                eye.map(f32::to_bits).hash(&mut hasher);
                focus.map(f32::to_bits).hash(&mut hasher);
                field_of_view.to_bits().hash(&mut hasher);
                size.hash(&mut hasher);
                levels.ladder.hash(&mut hasher);
                levels.capacity.hash(&mut hasher);
            }
            PaneResidency::Section {
                center,
                normal,
                right,
                span,
                thickness,
                size,
                levels,
                ..
            } => {
                center.map(f32::to_bits).hash(&mut hasher);
                normal.map(f32::to_bits).hash(&mut hasher);
                right.map(f32::to_bits).hash(&mut hasher);
                span.to_bits().hash(&mut hasher);
                thickness.to_bits().hash(&mut hasher);
                size.hash(&mut hasher);
                levels.level.hash(&mut hasher);
                levels.fallback.hash(&mut hasher);
                levels.capacity.hash(&mut hasher);
            }
        }
    }
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
    if budget > 0 && ladder.count > 0 {
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
