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
        while self.oit_readback.take(device).is_some() {}
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
            volume_measure,
            ..
        } = self;
        let pyramid = pyramid.as_ref()?;
        let planner = planner.as_ref()?;
        let atlas = atlas.as_mut()?;
        let words = feedback_words(bytes);
        if words.len() < (BAND + 1) * *view_words {
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
        let ladder = meta.levels.ladder;
        let mut demands = Vec::with_capacity(ladder.count);
        for band in 0..ladder.count {
            let slice = &words[band * *view_words..(band + 1) * *view_words];
            let held = decode_plan(
                slice,
                planner.view.bands[band].capacity(),
                meta.time,
                ladder.level(band),
                pyramid,
            )?;
            demands.push(held);
        }
        for (band, demand) in demands.iter().enumerate() {
            if let Some(slot) = volume_measure.get_mut(ladder.level(band)) {
                *slot = Some(demand.info.visible);
            }
            atlas.touch(
                meta.time,
                ladder.level(band),
                demand.indices.iter().copied(),
                *pin,
            );
        }
        issued.extend(prefetch_requests(
            &Prefetch {
                enabled: *prefetch,
                rate: *prefetch_rate,
                atlas,
                time: meta.time,
                ladder,
                demands: &demands,
                pyramid,
            },
            prefetched,
        ));
        for (band, demand) in demands.iter().enumerate() {
            demanded.extend(
                demand
                    .indices
                    .iter()
                    .map(|index| (ladder.level(band), *index)),
            );
            complete &= demands_complete(demand, meta.assured);
        }
        let mut merged = Demand::default();
        for demand in demands {
            merged.merge(demand);
        }
        merged.info.level = ladder.base;
        info.view = merged.info;
        requests.view = merged.info;
        requests.volume.extend(merged.requests.iter().copied());
        for request in &merged.requests {
            prefetched.remove(request);
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
    meta.data.hash(&mut hasher);
    meta.channels.hash(&mut hasher);
    meta.prefetch.hash(&mut hasher);
    meta.assured.hash(&mut hasher);
    meta.time.hash(&mut hasher);
    meta.eye.map(f32::to_bits).hash(&mut hasher);
    meta.focus.map(f32::to_bits).hash(&mut hasher);
    meta.field_of_view.to_bits().hash(&mut hasher);
    meta.size.hash(&mut hasher);
    meta.levels.ladder.hash(&mut hasher);
    meta.levels.capacity.hash(&mut hasher);
    hasher.finish()
}

pub(super) struct Prefetch<'a> {
    pub(super) enabled: bool,
    pub(super) rate: f32,
    pub(super) atlas: &'a Atlas,
    pub(super) time: u32,
    pub(super) ladder: Ladder,
    pub(super) demands: &'a [Demand],
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
        pyramid,
    } = *prefetch;
    let mut requests = Vec::new();
    let total = ((atlas.slots() as f32 / PREFETCH_SHARE as f32) * rate.clamp(0.25, 1.0))
        .min(PREFETCH_LIMIT as f32) as usize;
    if !enabled {
        return requests;
    }
    let next = (time + 1) % pyramid.frame_count();
    let mut budget = total;
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

pub(super) fn feedback_words(bytes: &[u8]) -> Vec<u32> {
    let (words, _) = bytes.as_chunks::<4>();
    words.iter().map(|word| u32::from_ne_bytes(*word)).collect()
}
