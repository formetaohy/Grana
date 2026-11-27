use std::collections::{HashMap, HashSet};

use super::super::residency::{Mode, Resolution, ViewState};
use super::super::signature::digest;
use super::super::view::View;
use super::super::*;
use super::{Input, levels};
use crate::hollow::Cursor;
use crate::planner::{Demand, decode_plan};
use crate::schema::Versions;

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

pub(crate) struct Poll<'a> {
    pub(crate) device: &'a Device,
    pub(crate) pyramid: &'a Pyramid,
    pub(crate) planner: &'a Planner,
    pub(crate) atlas: &'a mut Atlas,
    pub(crate) time: u32,
    pub(crate) prefetch: bool,
    pub(crate) pending: &'a HashSet<BrickRequest>,
}

pub(crate) struct ResidencyDemand {
    pub(crate) key: ViewKey,
    pub(crate) demand: Demand,
}

impl ResidencyDemand {
    pub(crate) fn covers(&self, key: &ViewKey) -> bool {
        self.key.same_view(key)
    }
}

pub(crate) struct Residence {
    feedback: Option<Readback>,
    frames: Vec<Option<ViewKey>>,
    words: usize,
    bytes: u64,
    epoch: u64,
    plan: u64,
    style: Option<u64>,
    content: u32,
    generation: u64,
    prefetched: HashSet<BrickRequest>,
    prefetch_key: Option<u64>,
    prefetch_stable: bool,
    batches: Vec<Batch>,
    refreshes: HashSet<BrickRequest>,
    denial: Vec<usize>,
    prefetch_rate: f32,
    prefetch_sent: u64,
    prefetch_hit: u64,
    settled: Option<ViewKey>,
    planned: Option<ViewKey>,
    demand: Option<ResidencyDemand>,
    missing: Missing,
    restaged: bool,
    passes: Option<Passes>,
    grids: Vec<BrickGrid>,
    view: ViewInfo,
    kept: Option<usize>,
}

#[derive(Default)]
struct Missing {
    time: Option<u32>,
    epoch: u32,
    level: Option<usize>,
    demanded: HashMap<usize, Vec<Word>>,
    wanted: HashMap<(usize, u32), Wanted>,
    order: Vec<BrickRequest>,
    ordered: bool,
    absent: usize,
    unfetchable: HashMap<usize, usize>,
    marked: Vec<BrickRequest>,
}

#[derive(Clone, Copy, Default)]
struct Word {
    epoch: u32,
    bits: u64,
}

#[derive(Clone, Copy)]
struct Wanted {
    bucket: u32,
    request: BrickRequest,
}

impl Missing {
    fn begin(&mut self, time: Option<u32>) {
        self.time = time;
        self.epoch = self.epoch.wrapping_add(1);
        self.level = None;
        self.demanded.clear();
        self.wanted.clear();
        self.order.clear();
        self.ordered = true;
        self.absent = 0;
        self.unfetchable.clear();
        self.marked.clear();
    }

    fn clear(&mut self) {
        self.begin(None);
    }

    fn apply(&mut self, demand: &ResidencyDemand, atlas: &Atlas) {
        let key = &demand.key;
        let time = key.time;
        let level = key.levels.level;
        let continued = self.time == Some(time) && self.level == Some(level);
        if !continued {
            self.begin(Some(time));
            self.level = Some(level);
        }
        self.epoch = self.epoch.wrapping_add(1);
        self.unfetchable.clear();
        self.marked.clear();
        for index in &demand.demand.indices {
            self.demands(level, *index);
        }
        for (position, (index, bucket)) in demand.demand.missing.iter().enumerate() {
            if atlas.slot(time, level, *index as usize).is_some()
                || atlas.is_hollow(time, level, *index as usize)
            {
                self.supply(level, *index);
                continue;
            }
            if atlas.is_unavailable(time, level, *index as usize) {
                self.unfetchable_mark(level, *index);
                self.marked.push(demand.demand.requests[position]);
                continue;
            }
            self.want(*bucket, level, *index, demand.demand.requests[position]);
        }
        self.prune();
    }

    fn unfetchable(&self, level: usize) -> usize {
        self.unfetchable.get(&level).copied().unwrap_or(0)
    }

    fn unfetchable_mark(&mut self, level: usize, index: u32) {
        if self.wanted.remove(&(level, index)).is_some() {
            self.absent = self.absent.saturating_sub(1);
            self.ordered = false;
        }
        *self.unfetchable.entry(level).or_default() += 1;
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

    fn want(&mut self, bucket: u32, level: usize, index: u32, request: BrickRequest) {
        match self.wanted.entry((level, index)) {
            std::collections::hash_map::Entry::Occupied(mut held) => {
                if held.get().bucket == bucket {
                    return;
                }
                held.get_mut().bucket = bucket;
            }
            std::collections::hash_map::Entry::Vacant(held) => {
                held.insert(Wanted { bucket, request });
                self.absent += 1;
            }
        }
        self.ordered = false;
    }

    fn supply(&mut self, level: usize, index: u32) {
        if self.wanted.remove(&(level, index)).is_some() {
            self.absent = self.absent.saturating_sub(1);
            self.ordered = false;
        }
    }

    fn prune(&mut self) {
        let before = self.wanted.len();
        let epoch = self.epoch;
        let demanded = &self.demanded;
        self.wanted.retain(|(level, index), _| {
            let word = *index as usize / u64::BITS as usize;
            demanded.get(level).is_some_and(|words| {
                words.get(word).is_some_and(|stamp| {
                    stamp.epoch == epoch && stamp.bits & (1 << (*index % u64::BITS)) != 0
                })
            })
        });
        if self.wanted.len() == before {
            return;
        }
        self.absent = self.wanted.len();
        self.ordered = false;
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
    pub(crate) budget: usize,
    pub(crate) size: [u32; 2],
    pub(crate) time: u32,
    pub(crate) versions: Versions,
    pub(crate) awaits: bool,
    pub(crate) assured: bool,
    pub(crate) prefetch: bool,
    pub(crate) hollow: Option<Cursor<'a>>,
}

pub(crate) struct Outlook {
    pub(crate) levels: Levels,
    pub(crate) key: ViewKey,
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
            plan: 0,
            style: None,
            content: 0,
            generation: 0,
            prefetched: HashSet::new(),
            prefetch_key: None,
            prefetch_stable: false,
            batches: Vec::new(),
            refreshes: HashSet::new(),
            denial: Vec::new(),
            prefetch_rate: 1.0,
            prefetch_sent: 0,
            prefetch_hit: 0,
            settled: None,
            planned: None,
            demand: None,
            missing: Missing::default(),
            restaged: false,
            passes: None,
            grids: Vec::new(),
            view: ViewInfo::default(),
            kept: None,
        }
    }

    pub(crate) fn setup(&mut self, device: &Device, slots: usize, pyramid: &Pyramid) {
        self.words = planner::plan_words(slots);
        self.bytes = (self.words * size_of::<u32>()) as u64;
        self.feedback = Some(Readback::new(
            device,
            "grana.feedback",
            self.bytes,
            planner::READBACK_SLOTS,
        ));
        self.frames = vec![None; planner::READBACK_SLOTS];
        self.denial = vec![0; pyramid.level_count()];
        self.grids = pyramid
            .levels()
            .iter()
            .map(|level| BrickGrid::new(level.extent()))
            .collect();
        self.epoch += 1;
        self.forget();
    }

    pub(crate) fn bytes(&self) -> u64 {
        self.feedback.as_ref().map_or(0, Readback::bytes)
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
        self.denial.fill(0);
        self.prefetch_rate = 1.0;
        self.prefetch_sent = 0;
        self.prefetch_hit = 0;
        self.settled = None;
        self.planned = None;
        self.demand = None;
        self.missing.clear();
        self.restaged = false;
        self.passes = None;
        self.view = ViewInfo::default();
        self.kept = None;
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
        self.denial.fill(0);
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
        self.planned = None;
        self.prefetched.clear();
        self.prefetch_key = None;
        self.prefetch_stable = false;
        self.batches.clear();
        self.refreshes.clear();
        self.denial.fill(0);
        self.prefetch_rate = 1.0;
        self.frames.fill(None);
        self.view = ViewInfo::default();
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
            self.refreshes.remove(request);
        }
    }

    pub(crate) fn denied(&mut self, requests: &[BrickRequest]) {
        for request in requests {
            if let Some(count) = self.denial.get_mut(request.level) {
                *count = count.saturating_add(1);
            }
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
            budget,
            size,
            time,
            versions,
            awaits,
            assured,
            prefetch,
            hollow,
        } = adapt;
        let target = volume_target(TargetView {
            pyramid,
            camera,
            height: view.height as f32,
            first: arena.first(),
            kept: self.kept,
        });
        self.kept = Some(target);
        let quantum = Resolution::of(pyramid, target, camera, size[1]);
        let planned = levels(Input {
            pyramid,
            camera,
            region,
            view,
            target,
            hollow,
            denial: &self.denial,
            budget,
        });
        for count in &mut self.denial {
            *count /= 2;
        }
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
        let ready_at_time = settled && (!assured || !planned.truncated);
        let ready = if awaits || assured {
            ready_at_time
        } else {
            true
        };
        Outlook {
            levels: planned.levels,
            key,
            replan,
            settled,
            truncated: planned.truncated,
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
    ) {
        if let Some(feedback) = self.feedback.as_ref() {
            feedback.stage(encoder, slot, 0, planner.plan.buffer(), self.bytes);
        }
        self.frames[slot] = Some(key);
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

    pub(crate) fn poll(&mut self, stage: Poll<'_>) -> Result<Option<Requests>, String> {
        let Poll {
            device,
            pyramid,
            planner,
            atlas,
            time,
            prefetch,
            pending,
        } = stage;
        while let Some((key, demand)) = self.decode(device, pyramid, planner)? {
            self.accept(pyramid, key, demand, atlas);
        }
        Ok(self.emit(pyramid, atlas, time, prefetch, pending))
    }

    fn decode(
        &mut self,
        device: &Device,
        pyramid: &Pyramid,
        planner: &Planner,
    ) -> Result<Option<(ViewKey, Demand)>, String> {
        let Self {
            feedback, frames, ..
        } = self;
        let Some(feedback) = feedback.as_mut() else {
            return Ok(None);
        };
        let decoded = feedback.take_with(device, |slot, bytes| {
            let key = frames[slot].take()?;
            let demand = decode_plan(
                bytes,
                planner.plan.capacity(),
                key.time,
                key.levels.level,
                pyramid,
            )?;
            Some((key, demand))
        })?;
        Ok(decoded.flatten())
    }

    fn accept(&mut self, pyramid: &Pyramid, key: ViewKey, demand: Demand, atlas: &mut Atlas) {
        if key.epoch != self.epoch {
            return;
        }
        atlas.advance_holds();
        atlas.touch(key.time, demand.info.level, demand.indices.iter().copied());
        if self
            .demand
            .as_ref()
            .is_none_or(|held| !held.key.same_plan(&key))
        {
            self.plan = self.plan.wrapping_add(1);
        }
        let demand = ResidencyDemand { key, demand };
        self.missing.apply(&demand, atlas);
        let unfetchable = self.missing.unfetchable(demand.demand.info.level);
        let settled = (self.missing.absent == 0
            && demand.demand.info.resident + unfetchable == demand.demand.info.demand
            && (!key.assured || demand.demand.info.visible == demand.demand.info.demand))
            .then_some(key);
        self.settled = settled;
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
        let wanted = demand.demand.indices.len();
        let free = atlas.vacancies().saturating_sub(self.prefetched.len());
        let budget = (free.min(wanted) as f32 * self.prefetch_rate) as usize;
        budget.min(PREFETCH_LIMIT)
    }

    fn observe(&mut self, time: u32) {
        self.prefetched.retain(|request| request.time > time);
        self.batches.retain(|batch| batch.time >= time);
        let Some(demand) = self.demand.as_ref() else {
            return;
        };
        for request in &demand.demand.requests {
            self.prefetched.remove(request);
        }
    }

    fn emit(
        &mut self,
        pyramid: &Pyramid,
        atlas: &Atlas,
        time: u32,
        prefetch: bool,
        pending: &HashSet<BrickRequest>,
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
        self.observe(time);
        self.prefetched.retain(|request| pending.contains(request));
        let demand = self.demand.as_ref().expect("the demand is staged");
        let mut requests = Requests {
            time: demand.key.time,
            epoch: self.plan,
            unavailable: self.missing.marked.clone(),
            ..Default::default()
        };
        let mut view = ViewInfo::default();
        let level = demand.demand.info.level;
        let unfetchable = self.missing.unfetchable(level);
        view.level = level;
        view.visible = demand.demand.info.visible;
        view.demand = demand.demand.info.demand;
        view.unavailable = unfetchable;
        view.resident = view
            .demand
            .saturating_sub(self.missing.absent + unfetchable);
        self.refreshes.retain(|request| {
            let grid = BrickGrid::new(pyramid.level(request.level).extent());
            let index = grid.index(request.coord);
            atlas.slot(request.time, request.level, index).is_some()
        });
        let mut room = atlas.available().saturating_sub(pending.len());
        let mut emitted = pending.clone();
        for request in self.refreshes.iter().copied() {
            if emitted.insert(request) {
                requests.volume.push(request);
            }
        }
        for request in self.missing.requests() {
            if room == 0 {
                break;
            }
            if emitted.insert(*request) {
                requests.volume.push(*request);
                room -= 1;
            }
        }
        if requests.volume.is_empty()
            && prefetch
            && self.prefetch_stable
            && pyramid.frame_count() > 1
        {
            let next = carried_frame(demand.key.time, pyramid.frame_count());
            let budget = self.prefetch_budget(atlas, demand);
            if next > demand.key.time && budget > 0 {
                let issued = {
                    let prefetch = Prefetch {
                        budget,
                        atlas,
                        time: next,
                        level: demand.key.levels.level,
                        demand: &demand.demand,
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

    pub(crate) fn supply(&mut self, request: BrickRequest) {
        if self.missing.time != Some(request.time) {
            return;
        }
        let Some(grid) = self.grids.get(request.level) else {
            return;
        };
        self.missing
            .supply(request.level, grid.index(request.coord) as u32);
    }

    pub(crate) fn abandoned(&mut self, request: BrickRequest) {
        if self.missing.time != Some(request.time) {
            return;
        }
        let Some(grid) = self.grids.get(request.level) else {
            return;
        };
        self.missing
            .unfetchable_mark(request.level, grid.index(request.coord) as u32);
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
        let Some(current) = self.demand.as_ref().map(|demand| demand.key.levels.level) else {
            return;
        };
        if level != current {
            return;
        }
        let grid = BrickGrid::new(pyramid.level(level).extent());
        let request = BrickRequest::new(time, level, grid.coord(index));
        self.missing
            .want(RESTORE_BUCKET, level, index as u32, request);
    }

    pub(crate) fn revalidate(&mut self) {
        self.restaged = true;
    }
}

struct Prefetch<'a> {
    budget: usize,
    atlas: &'a Atlas,
    time: u32,
    level: usize,
    demand: &'a Demand,
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
        level,
        demand,
        pyramid,
    } = *prefetch;
    let mut budget = budget.min(PREFETCH_LIMIT);
    let mut requests = Vec::new();
    carry(
        pyramid,
        atlas,
        time,
        level,
        &demand.indices,
        &mut requests,
        &mut budget,
        issued,
    );
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
            || atlas.is_unavailable(time, level, *index as usize)
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
