use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

use super::super::residency::{Mode, Resolution, SliceKey, ViewState, quantise};
use super::super::signature::digest;
use super::super::view::View;
use super::super::*;
use super::{Input, levels};
use crate::planner::{Demand, Measured, decode_plan};
use crate::schema::Versions;

const FEEDBACK_SLOTS: usize = 4;

const PREFETCH_LIMIT: usize = 1024;

const RESTORE_BUCKET: u32 = 0;

#[derive(Clone, Copy, PartialEq, Eq)]
struct Passes {
    layers: u64,
}

struct Batch {
    time: u32,
    requests: Vec<BrickRequest>,
}

pub(crate) struct ResidencyDemand {
    pub(crate) key: ViewKey,
    pub(crate) bands: Vec<Demand>,
}

impl ResidencyDemand {
    pub(crate) fn covers(&self, key: &ViewKey) -> bool {
        self.key.same_view(key)
    }
}

pub(crate) struct Residence {
    feedback: Option<Readback>,
    frames: Vec<Option<(ViewKey, u64)>>,
    words: usize,
    bytes: u64,
    epoch: u64,
    style: Option<u64>,
    content: u32,
    generation: u64,
    prefetched: HashSet<BrickRequest>,
    prefetch_key: Option<u64>,
    prefetch_stable: bool,
    batches: Vec<Batch>,
    refreshes: HashSet<BrickRequest>,
    prefetch_rate: f32,
    prefetch_sent: u64,
    prefetch_hit: u64,
    settled: Option<ViewKey>,
    planned: Option<ViewKey>,
    demand: Option<ResidencyDemand>,
    missing: Missing,
    restaged: bool,
    passes: Option<Passes>,
    cap: Option<usize>,
    level: Option<usize>,
    measure: Vec<Option<Measured>>,
    rejection: Option<(u64, usize)>,
    cache: Option<(u64, VolumeFit)>,
    view: ViewInfo,
}

#[derive(Default)]
struct Missing {
    time: Option<u32>,
    epoch: u32,
    bands: Vec<usize>,
    demanded: HashMap<usize, Vec<Word>>,
    wanted: HashMap<(usize, u32), Wanted>,
    order: Vec<BrickRequest>,
    ordered: bool,
    absent: [usize; BAND],
}

#[derive(Clone, Copy, Default)]
struct Word {
    epoch: u32,
    bits: u64,
}

#[derive(Clone, Copy)]
struct Wanted {
    band: usize,
    bucket: u32,
    request: BrickRequest,
}

impl Missing {
    fn begin(&mut self, time: Option<u32>) {
        self.time = time;
        self.epoch = self.epoch.wrapping_add(1);
        self.bands.clear();
        self.demanded.clear();
        self.wanted.clear();
        self.order.clear();
        self.ordered = true;
        self.absent = [0; BAND];
    }

    fn clear(&mut self) {
        self.begin(None);
    }

    fn apply(&mut self, demand: &ResidencyDemand, atlas: &Atlas) {
        let key = &demand.key;
        let time = key.time;
        let ladder = key.levels.ladder;
        let continued = self.time == Some(time)
            && self.bands.len() == demand.bands.len()
            && self
                .bands
                .iter()
                .zip(&demand.bands)
                .all(|(level, band)| *level == band.info.level);
        if !continued {
            self.begin(Some(time));
            self.bands = demand.bands.iter().map(|band| band.info.level).collect();
        }
        self.epoch = self.epoch.wrapping_add(1);
        for band in &demand.bands {
            for index in &band.indices {
                self.demands(band.info.level, *index);
            }
        }
        for (band, held) in demand.bands.iter().enumerate() {
            let level = held.info.level;
            for (position, (index, bucket)) in held.missing.iter().enumerate() {
                if atlas.slot(time, level, *index as usize).is_some()
                    || atlas.is_hollow(time, level, *index as usize)
                {
                    self.supply(level, *index);
                    continue;
                }
                let shown = level
                    .checked_sub(ladder.base)
                    .filter(|band| *band < ladder.count)
                    .unwrap_or(band);
                self.want(shown, *bucket, level, *index, held.requests[position]);
            }
        }
        self.prune();
    }

    fn demands(&mut self, level: usize, index: u32) {
        let word = index as usize / u64::BITS as usize;
        let words = self.demanded.entry(level).or_default();
        let fitted = word + 1;
        if words.len() < fitted {
            words.resize(fitted, Word::default());
        }
        let stamp = &mut words[word];
        if stamp.epoch != self.epoch {
            *stamp = Word {
                epoch: self.epoch,
                bits: 0,
            };
        }
        stamp.bits |= 1 << (index % u64::BITS);
    }

    fn demanded(&self, level: usize, index: u32) -> bool {
        let word = index as usize / u64::BITS as usize;
        self.demanded.get(&level).is_some_and(|words| {
            words.get(word).is_some_and(|stamp| {
                stamp.epoch == self.epoch && stamp.bits & (1 << (index % u64::BITS)) != 0
            })
        })
    }

    fn want(&mut self, band: usize, bucket: u32, level: usize, index: u32, request: BrickRequest) {
        match self.wanted.entry((level, index)) {
            std::collections::hash_map::Entry::Occupied(mut held) => {
                if held.get().bucket == bucket {
                    return;
                }
                held.get_mut().bucket = bucket;
            }
            std::collections::hash_map::Entry::Vacant(held) => {
                held.insert(Wanted {
                    band,
                    bucket,
                    request,
                });
                self.absent[band] += 1;
            }
        }
        self.ordered = false;
    }

    fn supply(&mut self, level: usize, index: u32) {
        if let Some(held) = self.wanted.remove(&(level, index)) {
            self.absent[held.band] -= 1;
            self.ordered = false;
        }
    }

    fn prune(&mut self) {
        let Self {
            epoch,
            demanded,
            wanted,
            absent,
            ordered,
            ..
        } = self;
        let before = wanted.len();
        wanted.retain(|(level, index), _| {
            let word = *index as usize / u64::BITS as usize;
            demanded.get(level).is_some_and(|words| {
                words.get(word).is_some_and(|stamp| {
                    stamp.epoch == *epoch && stamp.bits & (1 << (*index % u64::BITS)) != 0
                })
            })
        });
        if wanted.len() == before {
            return;
        }
        absent.fill(0);
        for held in wanted.values() {
            absent[held.band] += 1;
        }
        *ordered = false;
    }

    fn requests(&mut self) -> &[BrickRequest] {
        if !self.ordered {
            let mut pairs = self
                .wanted
                .values()
                .map(|held| (held.bucket, held.request))
                .collect::<Vec<_>>();
            pairs.sort_unstable_by_key(|(bucket, request)| {
                (
                    *bucket,
                    request.level,
                    request.coord.z(),
                    request.coord.y(),
                    request.coord.x(),
                )
            });
            self.order.clear();
            self.order
                .extend(pairs.into_iter().map(|(_, request)| request));
            self.ordered = true;
        }
        &self.order
    }
}

pub(crate) struct Adapt<'a> {
    pub(crate) pyramid: &'a Pyramid,
    pub(crate) camera: &'a Camera,
    pub(crate) region: Region,
    pub(crate) view: View,
    pub(crate) arena: &'a BrickArena,
    pub(crate) slots: usize,
    pub(crate) size: [u32; 2],
    pub(crate) time: u32,
    pub(crate) versions: Versions,
    pub(crate) awaits: bool,
    pub(crate) assured: bool,
    pub(crate) prefetch: bool,
    pub(crate) overflow: bool,
}

pub(crate) struct Outlook {
    pub(crate) ladder: Ladder,
    pub(crate) levels: Levels,
    pub(crate) key: ViewKey,
    pub(crate) pose: u64,
    pub(crate) replan: bool,
    pub(crate) settled: bool,
    pub(crate) truncated: bool,
    pub(crate) ready: bool,
}

impl Residence {
    pub(crate) fn new() -> Self {
        Self {
            feedback: None,
            frames: Vec::new(),
            words: 0,
            bytes: 0,
            epoch: 0,
            style: None,
            content: 0,
            generation: 0,
            prefetched: HashSet::new(),
            prefetch_key: None,
            prefetch_stable: false,
            batches: Vec::new(),
            refreshes: HashSet::new(),
            prefetch_rate: 1.0,
            prefetch_sent: 0,
            prefetch_hit: 0,
            settled: None,
            planned: None,
            demand: None,
            missing: Missing::default(),
            restaged: false,
            passes: None,
            cap: None,
            level: None,
            measure: Vec::new(),
            rejection: None,
            cache: None,
            view: ViewInfo::default(),
        }
    }

    pub(crate) fn setup(&mut self, device: &Device, slots: usize, levels: usize) {
        self.words = planner::plan_words(slots);
        self.bytes = (self.words * size_of::<u32>()) as u64;
        self.feedback = Some(Readback::new(
            device,
            "grana.feedback",
            (BAND * self.words) as u64 * size_of::<u32>() as u64,
            FEEDBACK_SLOTS,
        ));
        self.frames = vec![None; FEEDBACK_SLOTS];
        self.measure = vec![None; levels];
        self.epoch += 1;
        self.forget();
    }

    pub(crate) fn forget(&mut self) {
        self.frames.fill(None);
        self.style = None;
        self.content = 0;
        self.generation = 0;
        self.prefetched.clear();
        self.prefetch_key = None;
        self.prefetch_stable = false;
        self.batches.clear();
        self.refreshes.clear();
        self.prefetch_rate = 1.0;
        self.prefetch_sent = 0;
        self.prefetch_hit = 0;
        self.settled = None;
        self.planned = None;
        self.demand = None;
        self.missing.clear();
        self.restaged = false;
        self.passes = None;
        self.cap = None;
        self.level = None;
        self.measure.fill(None);
        self.rejection = None;
        self.cache = None;
        self.view = ViewInfo::default();
    }

    pub(crate) fn invalidate(&mut self) {
        self.content = self.content.wrapping_add(1);
        self.style = None;
        self.frames.fill(None);
        self.settled = None;
        self.planned = None;
        self.demand = None;
        self.missing.clear();
        self.restaged = false;
        self.prefetched.clear();
        self.prefetch_key = None;
        self.prefetch_stable = false;
        self.batches.clear();
        self.refreshes.clear();
        self.measure.fill(None);
        self.rejection = None;
        self.cache = None;
        self.bump();
    }

    pub(crate) fn bump(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }

    pub(crate) fn forget_settled(&mut self) {
        self.settled = None;
    }

    pub(crate) fn begin_export(&mut self) {
        self.passes = None;
        self.settled = None;
        self.demand = None;
        self.missing.clear();
        self.restaged = false;
        self.cap = None;
        self.level = None;
        self.rejection = None;
        self.planned = None;
        self.prefetched.clear();
        self.prefetch_key = None;
        self.prefetch_stable = false;
        self.batches.clear();
        self.refreshes.clear();
        self.prefetch_rate = 1.0;
        self.frames.fill(None);
        self.measure.fill(None);
        self.cache = None;
        self.view = ViewInfo::default();
    }

    pub(crate) fn set_cap(&mut self, cap: usize) {
        self.cap = Some(cap);
    }

    pub(crate) fn content(&self) -> u32 {
        self.content
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) fn epoch(&self) -> u64 {
        self.epoch
    }

    pub(crate) fn view(&self) -> ViewInfo {
        self.view
    }

    pub(crate) fn streams(&mut self) -> (&mut HashSet<BrickRequest>, &mut HashSet<BrickRequest>) {
        (&mut self.prefetched, &mut self.refreshes)
    }

    pub(crate) fn set_generation(&mut self, generation: u64) {
        self.generation = generation;
    }

    pub(crate) fn clear_prefetch(&mut self) {
        self.prefetched.clear();
        self.batches.clear();
    }

    pub(crate) fn refreshes(&mut self) -> &mut HashSet<BrickRequest> {
        &mut self.refreshes
    }

    pub(crate) fn prefetch_counts(&self) -> (u64, u64) {
        (self.prefetch_sent, self.prefetch_hit)
    }

    pub(crate) fn discard(&mut self, requests: &[BrickRequest]) {
        for request in requests {
            self.prefetched.remove(request);
        }
    }

    pub(crate) fn style_outdated(&self, style: u64) -> bool {
        self.style != Some(style)
    }

    pub(crate) fn set_style(&mut self, style: u64) {
        self.style = Some(style);
    }

    pub(crate) fn stamp(&self, data: u64) -> u64 {
        digest(&[data, self.generation, u64::from(self.content)])
    }

    pub(crate) fn passes_outdated(&self, versions: Versions) -> bool {
        self.passes
            != Some(Passes {
                layers: versions.topology(),
            })
    }

    pub(crate) fn load_passes(&mut self, versions: Versions) {
        self.passes = Some(Passes {
            layers: versions.topology(),
        });
    }

    pub(crate) fn adapt(&mut self, adapt: Adapt<'_>) -> Outlook {
        let Adapt {
            pyramid,
            camera,
            region,
            view,
            arena,
            slots,
            size,
            time,
            versions,
            awaits,
            assured,
            prefetch,
            overflow,
        } = adapt;
        let quantum = Resolution::of(
            pyramid,
            self.level.unwrap_or(pyramid.level_count() - 1),
            camera,
            size[1],
        );
        let pose = pose(camera, size, region, time, quantum);
        let current = if assured { None } else { self.level };
        let measured = !assured
            && current.is_some_and(|level| {
                self.measure
                    .get(level)
                    .copied()
                    .flatten()
                    .is_some_and(|held| held.pose == pose)
            });
        let rejected = if assured {
            None
        } else {
            self.rejection
                .filter(|(captured, _)| *captured == view.motion)
                .map(|(_, level)| level)
        };
        let planned = levels(
            Input {
                pyramid,
                camera,
                region,
                view,
                arena,
                slots,
                cap: self.cap,
                current,
                measured,
                pose,
                rejected,
            },
            &self.measure,
            &mut self.cache,
        );
        if !assured {
            self.rejection = planned
                .overflowed
                .or_else(|| current.filter(|_| overflow))
                .map(|level| (view.motion, level));
        }
        self.level = Some(planned.ladder.base);
        let key = ViewKey::of(
            self.epoch,
            self.stamp(versions.topology()),
            Mode { prefetch, assured },
            time,
            ViewState {
                camera,
                size,
                region,
                levels: planned.levels,
                quantum,
            },
        );
        let replan = self.planned.as_ref() != Some(&key);
        let settled = self.settled == Some(key);
        let truncated =
            self.view.level == planned.ladder.base && self.view.visible > self.view.demand;
        let ready_at_time = settled && (!assured || !truncated);
        let ready = if awaits || assured {
            ready_at_time
        } else {
            true
        };
        Outlook {
            ladder: planned.ladder,
            levels: planned.levels,
            key,
            pose,
            replan,
            settled,
            truncated,
            ready,
        }
    }

    pub(crate) fn reserve(&mut self, replan: bool) -> Option<usize> {
        if replan {
            self.feedback.as_mut().and_then(Readback::reserve)
        } else {
            None
        }
    }

    pub(crate) fn stage(
        &mut self,
        encoder: &mut CommandEncoder,
        slot: usize,
        planner: &Planner,
        key: ViewKey,
        pose: u64,
    ) {
        if let Some(feedback) = self.feedback.as_ref() {
            for (band, view) in planner.view.bands.iter().enumerate() {
                feedback.stage(
                    encoder,
                    slot,
                    band as u64 * self.bytes,
                    view.buffer(),
                    self.bytes,
                );
            }
        }
        self.frames[slot] = Some((key, pose));
        self.planned = Some(key);
        if self.demand.as_ref().is_none_or(|held| !held.covers(&key)) {
            self.demand = None;
        }
    }

    pub(crate) fn register(&mut self) {
        if let Some(feedback) = self.feedback.as_mut() {
            feedback.register();
        }
    }

    pub(crate) fn pending(&self) -> bool {
        self.feedback.as_ref().is_some_and(Readback::pending)
    }

    pub(crate) fn poll(
        &mut self,
        device: &Device,
        pyramid: &Pyramid,
        planner: &Planner,
        atlas: &mut Atlas,
        time: u32,
        prefetch: bool,
    ) -> Result<Option<Requests>, String> {
        while let Some((key, pose, bands)) = self.decode(device, pyramid, planner)? {
            self.accept(pyramid, key, pose, bands, atlas);
        }
        Ok(self.emit(pyramid, atlas, time, prefetch))
    }

    fn decode(
        &mut self,
        device: &Device,
        pyramid: &Pyramid,
        planner: &Planner,
    ) -> Result<Option<(ViewKey, u64, Vec<Demand>)>, String> {
        let Self {
            feedback,
            frames,
            words,
            ..
        } = self;
        let words = *words;
        let Some(feedback) = feedback.as_mut() else {
            return Ok(None);
        };
        let decoded = feedback.take_with(device, |slot, bytes| {
            let (key, pose) = frames[slot].take()?;
            let bands = parse_bands(bytes, words, key.levels.ladder, key.time, pyramid, planner)?;
            Some((key, pose, bands))
        })?;
        Ok(decoded.flatten())
    }

    fn accept(
        &mut self,
        pyramid: &Pyramid,
        key: ViewKey,
        pose: u64,
        bands: Vec<Demand>,
        atlas: &mut Atlas,
    ) {
        if key.epoch != self.epoch {
            return;
        }
        atlas.advance_holds();
        for band in &bands {
            if let Some(slot) = self.measure.get_mut(band.info.level) {
                *slot = Some(Measured {
                    pose,
                    visible: band.info.visible,
                });
            }
            atlas.touch(key.time, band.info.level, band.indices.iter().copied());
        }
        self.settled = bands
            .iter()
            .all(|band| demands_complete(band, key.assured))
            .then_some(key);
        let demand = ResidencyDemand { key, bands };
        self.missing.apply(&demand, atlas);
        self.demand = Some(demand);
        self.measure_prefetch(pyramid, key.time);
        let preview = key.prefetch_digest();
        if self.prefetch_key == Some(preview) {
            self.prefetch_stable = true;
        } else {
            self.prefetch_key = Some(preview);
            self.prefetch_stable = false;
            self.prefetched.clear();
            self.batches.clear();
        }
        self.restaged = false;
    }

    fn measure_prefetch(&mut self, pyramid: &Pyramid, time: u32) {
        let mut requests = Vec::new();
        self.batches.retain_mut(|batch| {
            if batch.time == time {
                requests.append(&mut batch.requests);
                false
            } else {
                batch.time > time
            }
        });
        let issued = requests.len();
        if issued == 0 {
            return;
        }
        let mut hits = 0u64;
        for request in &requests {
            let grid = BrickGrid::new(pyramid.level(request.level).extent());
            if self
                .missing
                .demanded(request.level, grid.index(request.coord) as u32)
            {
                hits += 1;
            }
        }
        self.prefetch_sent += issued as u64;
        self.prefetch_hit += hits;
        let rate = hits as f32 / issued as f32;
        self.prefetch_rate = (0.7 * self.prefetch_rate + 0.3 * rate).clamp(0.0, 1.0);
    }

    fn prefetch_budget(&self, atlas: &Atlas, demand: &ResidencyDemand) -> usize {
        let wanted = demand
            .bands
            .iter()
            .map(|band| band.indices.len())
            .sum::<usize>();
        let free = atlas.vacancies().saturating_sub(self.prefetched.len());
        let budget = (free.min(wanted) as f32 * self.prefetch_rate) as usize;
        budget.min(PREFETCH_LIMIT)
    }

    fn observe(&mut self, pyramid: &Pyramid, time: u32) {
        let next = carried_frame(time, pyramid.frame_count());
        self.prefetched
            .retain(|request| request.time == time || request.time == next);
        self.batches.retain(|batch| batch.time >= time);
        let Some(demand) = self.demand.as_ref() else {
            return;
        };
        for band in &demand.bands {
            for request in &band.requests {
                self.prefetched.remove(request);
            }
        }
    }

    fn emit(
        &mut self,
        pyramid: &Pyramid,
        atlas: &Atlas,
        time: u32,
        prefetch: bool,
    ) -> Option<Requests> {
        if self
            .demand
            .as_ref()
            .is_none_or(|demand| demand.key.time != time)
        {
            return None;
        }
        if self.restaged {
            let Self {
                demand,
                missing,
                restaged,
                ..
            } = self;
            if let Some(demand) = demand.as_ref() {
                missing.apply(demand, atlas);
            }
            *restaged = false;
        }
        self.observe(pyramid, time);
        let demand = self.demand.as_ref().expect("the demand is staged");
        let mut requests = Requests {
            time: demand.key.time,
            ..Default::default()
        };
        let mut view = ViewInfo {
            level: demand.key.levels.ladder.base,
            ..Default::default()
        };
        for (band, held) in demand.bands.iter().enumerate() {
            view.visible += held.info.visible;
            view.demand += held.info.demand;
            view.resident += held.info.demand - self.missing.absent[band];
        }
        self.refreshes.retain(|request| {
            let grid = BrickGrid::new(pyramid.level(request.level).extent());
            let index = grid.index(request.coord);
            atlas.slot(request.time, request.level, index).is_some()
        });
        requests.volume.extend(self.refreshes.iter().copied());
        requests
            .volume
            .extend(self.missing.requests().iter().copied());
        if requests.volume.is_empty()
            && prefetch
            && self.prefetch_stable
            && pyramid.frame_count() > 1
        {
            let next = carried_frame(demand.key.time, pyramid.frame_count());
            let budget = self.prefetch_budget(atlas, demand);
            if budget > 0 {
                let issued = {
                    let prefetch = Prefetch {
                        budget,
                        atlas,
                        time: next,
                        ladder: demand.key.levels.ladder,
                        demands: &demand.bands,
                        pyramid,
                    };
                    prefetch_requests(&prefetch, &mut self.prefetched)
                };
                if !issued.is_empty() {
                    match self.batches.iter_mut().find(|batch| batch.time == next) {
                        Some(batch) => batch.requests.extend(issued.iter().copied()),
                        None => self.batches.push(Batch {
                            time: next,
                            requests: issued.clone(),
                        }),
                    }
                    requests.prefetch = issued;
                }
            }
        }
        self.view = view;
        requests.view = view;
        Some(requests)
    }

    pub(crate) fn supply(&mut self, time: u32, level: usize, index: usize) {
        if self.missing.time != Some(time) {
            return;
        }
        self.missing.supply(level, index as u32);
    }

    pub(crate) fn release(&mut self, pyramid: &Pyramid, time: u32, level: usize, index: usize) {
        if self.missing.time != Some(time) {
            return;
        }
        if !self.missing.demanded(level, index as u32) {
            return;
        }
        if self.missing.wanted.contains_key(&(level, index as u32)) {
            return;
        }
        let Some(ladder) = self.demand.as_ref().map(|demand| demand.key.levels.ladder) else {
            return;
        };
        let Some(band) = level
            .checked_sub(ladder.base)
            .filter(|band| *band < ladder.count)
        else {
            return;
        };
        let grid = BrickGrid::new(pyramid.level(level).extent());
        let request = BrickRequest::new(time, level, grid.coord(index));
        self.missing
            .want(band, RESTORE_BUCKET, level, index as u32, request);
    }

    pub(crate) fn revalidate(&mut self) {
        self.restaged = true;
    }
}

fn demands_complete(demand: &Demand, assured: bool) -> bool {
    demand.requests.is_empty()
        && demand.info.resident == demand.info.demand
        && (!assured || demand.info.visible == demand.info.demand)
}

fn pose(camera: &Camera, size: [u32; 2], region: Region, time: u32, quantum: Resolution) -> u64 {
    let mut hasher = DefaultHasher::new();
    quantum.position(camera.eye().to_array()).hash(&mut hasher);
    quantum
        .position(camera.pivot().to_array())
        .hash(&mut hasher);
    quantise(camera.fov_y(), quantum.angle).hash(&mut hasher);
    camera.projection().index().hash(&mut hasher);
    camera
        .slice()
        .map(|slice| SliceKey::of(slice, quantum))
        .hash(&mut hasher);
    size.hash(&mut hasher);
    time.hash(&mut hasher);
    region.enabled().hash(&mut hasher);
    quantum
        .position(region.center().to_array())
        .hash(&mut hasher);
    quantum.position(region.half().to_array()).hash(&mut hasher);
    for axis in 0..3 {
        quantum
            .direction(region.axis(axis).to_array())
            .hash(&mut hasher);
    }
    hasher.finish()
}

struct Prefetch<'a> {
    budget: usize,
    atlas: &'a Atlas,
    time: u32,
    ladder: Ladder,
    demands: &'a [Demand],
    pyramid: &'a Pyramid,
}

fn prefetch_requests(
    prefetch: &Prefetch<'_>,
    issued: &mut HashSet<BrickRequest>,
) -> Vec<BrickRequest> {
    let Prefetch {
        budget,
        atlas,
        time,
        ladder,
        demands,
        pyramid,
    } = *prefetch;
    let mut budget = budget.min(PREFETCH_LIMIT);
    let mut requests = Vec::new();
    for (band, demand) in demands.iter().enumerate() {
        if budget == 0 {
            break;
        }
        carry(
            pyramid,
            atlas,
            time,
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
        let request = BrickRequest::new(time, level, grid.coord(*index as usize));
        if !issued.insert(request) {
            continue;
        }
        requests.push(request);
        *budget -= 1;
    }
}

fn parse_bands(
    bytes: &[u8],
    words: usize,
    ladder: Ladder,
    time: u32,
    pyramid: &Pyramid,
    planner: &Planner,
) -> Option<Vec<Demand>> {
    let width = words * size_of::<u32>();
    if bytes.len() < BAND * width {
        return None;
    }
    let mut bands = Vec::with_capacity(ladder.count);
    for band in 0..ladder.count {
        let slice = &bytes[band * width..(band + 1) * width];
        bands.push(decode_plan(
            slice,
            planner.view.bands[band].capacity(),
            time,
            ladder.level(band),
            pyramid,
        )?);
    }
    Some(bands)
}
