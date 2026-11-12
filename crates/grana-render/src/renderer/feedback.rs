use super::*;

impl Renderer {
    pub fn poll_requests(&mut self, device: &Device) -> Option<Requests> {
        while let Some((slot, bytes)) = self.take_feedback(device) {
            let Some(meta) = self.frames[slot].take() else {
                continue;
            };
            self.accept(meta, &bytes);
        }
        while self.oit_readback.take(device).is_some() {}
        self.emit()
    }

    pub(super) fn take_feedback(&mut self, device: &Device) -> Option<(usize, Vec<u8>)> {
        self.feedback
            .as_mut()
            .and_then(|feedback| feedback.take(device))
    }

    pub fn requests_pending(&self) -> bool {
        self.feedback.as_ref().is_some_and(Readback::pending)
    }

    fn accept(&mut self, meta: Residency, bytes: &[u8]) {
        if meta.epoch != self.epoch {
            return;
        }
        let Some(bands) = self.parse(&meta, bytes) else {
            return;
        };
        self.pin += 1;
        let pin = self.pin;
        if let Some(atlas) = self.atlas.as_mut() {
            atlas.clear_holds();
            for band in &bands {
                if let Some(slot) = self.volume_measure.get_mut(band.info.level) {
                    *slot = Some(band.info.visible);
                }
                atlas.touch(
                    meta.time,
                    band.info.level,
                    band.indices.iter().copied(),
                    pin,
                );
            }
        }
        self.observe(&meta, &bands);
        self.settled = bands
            .iter()
            .all(|band| demands_complete(band, meta.assured))
            .then_some(meta);
        self.demand = Some(ResidencyDemand {
            residency: meta,
            bands,
        });
    }

    fn parse(&self, meta: &Residency, bytes: &[u8]) -> Option<Vec<Demand>> {
        let pyramid = self.pyramid.as_ref()?;
        let planner = self.planner.as_ref()?;
        let words = feedback_words(bytes);
        if words.len() < (BAND + 1) * self.view_words {
            return None;
        }
        let ladder = meta.levels.ladder;
        let mut bands = Vec::with_capacity(ladder.count);
        for band in 0..ladder.count {
            let slice = &words[band * self.view_words..(band + 1) * self.view_words];
            bands.push(decode_plan(
                slice,
                planner.view.bands[band].capacity(),
                meta.time,
                ladder.level(band),
                pyramid,
            )?);
        }
        Some(bands)
    }

    fn observe(&mut self, meta: &Residency, bands: &[Demand]) {
        let key = view_key(meta);
        if self.prefetch_key != Some(key) {
            self.prefetch_key = Some(key);
            self.prefetched.clear();
        }
        let Some(pyramid) = self.pyramid.as_ref() else {
            return;
        };
        let demanded = bands
            .iter()
            .flat_map(|band| {
                band.indices
                    .iter()
                    .map(move |index| (band.info.level, *index))
            })
            .collect::<HashSet<_>>();
        let mut issued = 0;
        let mut hit = 0;
        for request in &self.prefetch_issued {
            if request.time != meta.time {
                continue;
            }
            issued += 1;
            if demanded.contains(&(request.level, index_of(request, pyramid))) {
                hit += 1;
            }
        }
        if issued > 0 {
            let rate = hit as f32 / issued as f32;
            self.prefetch_rate = 0.7 * self.prefetch_rate + 0.3 * rate.clamp(0.25, 1.0);
        }
        self.prefetch_issued.clear();
        for band in bands {
            for request in &band.requests {
                self.prefetched.remove(request);
            }
        }
    }

    fn emit(&mut self) -> Option<Requests> {
        let demand = self.demand.as_ref()?;
        if demand.residency.time != self.time {
            return None;
        }
        let pyramid = self.pyramid.as_ref()?;
        let atlas = self.atlas.as_ref()?;
        let mut requests = Requests {
            time: demand.residency.time,
            ..Default::default()
        };
        let mut view = ViewInfo {
            level: demand.residency.levels.ladder.base,
            ..Default::default()
        };
        let mut missing = Vec::new();
        for band in &demand.bands {
            let grid = BrickGrid::new(pyramid.level(band.info.level).extent());
            let mut absent = Vec::new();
            for (request, bucket) in band.requests.iter().zip(&band.buckets) {
                let index = grid.index(request.coord);
                if atlas.slot(request.time, request.level, index).is_some()
                    || atlas.is_hollow(request.time, request.level, index)
                {
                    continue;
                }
                absent.push((*bucket, *request));
            }
            view.visible += band.info.visible;
            view.demand += band.info.demand;
            view.resident += band.info.demand - absent.len();
            missing.extend(absent);
        }
        missing.sort_by_key(|(bucket, _)| *bucket);
        requests.volume = missing.into_iter().map(|(_, request)| request).collect();
        if requests.volume.is_empty() && self.prefetch {
            let issued = prefetch_requests(
                &Prefetch {
                    rate: self.prefetch_rate,
                    atlas,
                    time: demand.residency.time,
                    ladder: demand.residency.levels.ladder,
                    demands: &demand.bands,
                    pyramid,
                },
                &mut self.prefetched,
            );
            self.prefetch_issued.extend_from_slice(&issued);
            requests.prefetch = issued;
        }
        self.info.view = view;
        requests.view = view;
        Some(requests)
    }
}

impl ResidencyDemand {
    pub(super) fn covers(&self, residency: &Residency) -> bool {
        view_key(&self.residency) == view_key(residency)
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
        rate,
        atlas,
        time,
        ladder,
        demands,
        pyramid,
    } = *prefetch;
    let total = ((atlas.slots() as f32 / PREFETCH_SHARE as f32) * rate.clamp(0.25, 1.0))
        .min(PREFETCH_LIMIT as f32) as usize;
    let next = (time + 1) % pyramid.frame_count();
    let mut budget = total;
    let mut requests = Vec::new();
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
