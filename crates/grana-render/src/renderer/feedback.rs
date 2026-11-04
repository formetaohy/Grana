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
        let prefetched = prefetch_requests(*prefetch, atlas, meta.time, &volume, &section, pyramid);
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

pub(super) fn prefetch_requests(
    enabled: bool,
    atlas: &Atlas,
    time: u32,
    volume: &Demand,
    section: &Demand,
    pyramid: &Pyramid,
) -> Vec<BrickRequest> {
    if !enabled {
        return Vec::new();
    }
    let next = time + 1;
    if next >= pyramid.frame_count() {
        return Vec::new();
    }
    let mut budget = (atlas.slots() / PREFETCH_SHARE).min(PREFETCH_LIMIT);
    let mut requests = Vec::new();
    for (demand, level) in [(volume, volume.info.level), (section, section.info.level)] {
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
            requests.push(BrickRequest {
                time: next,
                level,
                coord: grid.coord(*index as usize),
            });
            budget -= 1;
        }
    }
    requests
}

pub(super) fn feedback_words(bytes: &[u8]) -> Vec<u32> {
    let (words, _) = bytes.as_chunks::<4>();
    words.iter().map(|word| u32::from_ne_bytes(*word)).collect()
}
