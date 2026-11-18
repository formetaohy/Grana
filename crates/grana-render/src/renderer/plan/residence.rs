use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};

use super::super::signature::digest;
use super::super::view::View;
use super::super::*;
use super::{Input, levels};
use crate::planner::{Demand, decode_plan};
use crate::schema::Versions;

const FEEDBACK_SLOTS: usize = 4;

const PREFETCH_LIMIT: usize = 1024;

const RESTORE_BUCKET: u32 = 0;

const PREFETCH_SHARE: usize = 4;

#[derive(Clone, Copy, PartialEq, Eq)]
struct Passes {
    objects: u64,
    style: u64,
}

pub(crate) struct ResidencyDemand {
    pub(crate) key: ViewKey,
    pub(crate) bands: Vec<Demand>,
}

impl ResidencyDemand {
    pub(crate) fn covers(&self, key: &ViewKey) -> bool {
        view_key(&self.key) == view_key(key)
    }
}

pub(crate) struct Residence {
    feedback: Option<Readback>,
    frames: Vec<Option<ViewKey>>,
    words: usize,
    bytes: u64,
    epoch: u64,
    style: Option<u64>,
    content: u32,
    generation: u64,
    pin: u64,
    prefetched: HashSet<BrickRequest>,
    observed: HashSet<(usize, u32)>,
    prefetch_key: Option<u64>,
    prefetch_issued: Vec<BrickRequest>,
    refreshes: HashSet<BrickRequest>,
    prefetch_rate: f32,
    settled: Option<ViewKey>,
    planned: Option<ViewKey>,
    demand: Option<ResidencyDemand>,
    missing: Missing,
    stale: bool,
    passes: Option<Passes>,
    cap: Option<usize>,
    level: Option<usize>,
    measure: Vec<Option<usize>>,
    rejection: Option<(u64, usize)>,
    cache: Option<(u64, VolumeFit)>,
    view: ViewInfo,
}

#[derive(Default)]
struct Missing {
    time: Option<u32>,
    demanded: HashMap<usize, Vec<u64>>,
    entries: BTreeMap<(u32, usize, u32), (usize, BrickRequest)>,
    shown: HashMap<(usize, u32), u32>,
    absent: [usize; BAND],
}

impl Missing {
    fn begin(&mut self, time: u32) {
        self.time = Some(time);
        for bits in self.demanded.values_mut() {
            bits.fill(0);
        }
        self.entries.clear();
        self.shown.clear();
        self.absent = [0; BAND];
    }

    fn clear(&mut self) {
        self.time = None;
        self.demanded.clear();
        self.entries.clear();
        self.shown.clear();
        self.absent = [0; BAND];
    }

    fn demands(&mut self, level: usize, index: u32) {
        let word = index as usize / u64::BITS as usize;
        let bits = self.demanded.entry(level).or_default();
        let fitted = word + 1;
        if bits.len() < fitted {
            bits.resize(fitted, 0);
        }
        bits[word] |= 1 << (index % u64::BITS);
    }

    fn demanded(&self, level: usize, index: u32) -> bool {
        let word = index as usize / u64::BITS as usize;
        self.demanded.get(&level).is_some_and(|bits| {
            bits.get(word)
                .is_some_and(|bits| bits & (1 << (index % u64::BITS)) != 0)
        })
    }

    fn want(&mut self, bucket: u32, band: usize, level: usize, index: u32, request: BrickRequest) {
        if self.shown.insert((level, index), bucket).is_some() {
            return;
        }
        self.entries.insert((bucket, level, index), (band, request));
        self.absent[band] += 1;
    }

    fn supply(&mut self, level: usize, index: u32) {
        let Some(bucket) = self.shown.remove(&(level, index)) else {
            return;
        };
        if let Some((band, _)) = self.entries.remove(&(bucket, level, index)) {
            self.absent[band] -= 1;
        }
    }

    fn requests(&self) -> impl Iterator<Item = BrickRequest> + '_ {
        self.entries.values().map(|(_, request)| *request)
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
            pin: 0,
            prefetched: HashSet::new(),
            observed: HashSet::new(),
            prefetch_key: None,
            prefetch_issued: Vec::new(),
            refreshes: HashSet::new(),
            prefetch_rate: 1.0,
            settled: None,
            planned: None,
            demand: None,
            missing: Missing::default(),
            stale: true,
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
        self.pin = 0;
        self.prefetched.clear();
        self.prefetch_key = None;
        self.prefetch_issued.clear();
        self.refreshes.clear();
        self.prefetch_rate = 1.0;
        self.settled = None;
        self.planned = None;
        self.demand = None;
        self.missing.clear();
        self.stale = true;
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
        self.stale = true;
        self.prefetched.clear();
        self.prefetch_key = None;
        self.prefetch_issued.clear();
        self.refreshes.clear();
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
        self.stale = true;
        self.cap = None;
        self.level = None;
        self.rejection = None;
        self.planned = None;
        self.prefetched.clear();
        self.prefetch_key = None;
        self.prefetch_issued.clear();
        self.refreshes.clear();
        self.prefetch_rate = 1.0;
        self.frames.fill(None);
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

    pub(crate) fn pin(&self) -> u64 {
        self.pin
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
        self.prefetch_issued.clear();
    }

    pub(crate) fn refreshes(&mut self) -> &mut HashSet<BrickRequest> {
        &mut self.refreshes
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
                objects: versions.objects(),
                style: versions.style(),
            })
    }

    pub(crate) fn load_passes(&mut self, versions: Versions) {
        self.passes = Some(Passes {
            objects: versions.objects(),
            style: versions.style(),
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
        let current = if assured { None } else { self.level };
        let measured = !assured
            && current.is_some_and(|level| self.measure.get(level).copied().flatten().is_some());
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
            self.stamp(versions.data()),
            Mode { prefetch, assured },
            time,
            ViewState {
                camera,
                size,
                region,
                levels: planned.levels,
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

    pub(crate) fn poll(
        &mut self,
        device: &Device,
        pyramid: &Pyramid,
        planner: &Planner,
        atlas: &mut Atlas,
        time: u32,
        prefetch: bool,
    ) -> Option<Requests> {
        while let Some((key, bands)) = self.decode(device, pyramid, planner) {
            self.accept(key, bands, pyramid, atlas);
        }
        self.emit(pyramid, atlas, time, prefetch)
    }

    fn decode(
        &mut self,
        device: &Device,
        pyramid: &Pyramid,
        planner: &Planner,
    ) -> Option<(ViewKey, Vec<Demand>)> {
        let Self {
            feedback,
            frames,
            words,
            ..
        } = self;
        let words = *words;
        feedback.as_mut()?.take_with(device, |slot, bytes| {
            let key = frames[slot].take()?;
            let bands = parse_bands(bytes, words, key.levels.ladder, key.time, pyramid, planner)?;
            Some((key, bands))
        })?
    }

    fn accept(&mut self, key: ViewKey, bands: Vec<Demand>, pyramid: &Pyramid, atlas: &mut Atlas) {
        if key.epoch != self.epoch {
            return;
        }
        self.pin += 1;
        let pin = self.pin;
        atlas.clear_holds();
        for band in &bands {
            if let Some(slot) = self.measure.get_mut(band.info.level) {
                *slot = Some(band.info.visible);
            }
            atlas.touch(key.time, band.info.level, band.indices.iter().copied(), pin);
        }
        self.observe(&key, &bands, pyramid);
        self.settled = bands
            .iter()
            .all(|band| demands_complete(band, key.assured))
            .then_some(key);
        self.demand = Some(ResidencyDemand { key, bands });
        self.stale = true;
    }

    fn observe(&mut self, key: &ViewKey, bands: &[Demand], pyramid: &Pyramid) {
        let view = view_key(key);
        if self.prefetch_key != Some(view) {
            self.prefetch_key = Some(view);
            self.prefetched.clear();
        }
        if !self.prefetch_issued.is_empty() {
            self.observed.clear();
            for (level, index) in bands.iter().flat_map(|band| {
                band.indices
                    .iter()
                    .map(move |index| (band.info.level, *index))
            }) {
                self.observed.insert((level, index));
            }
            let mut issued = 0;
            let mut hit = 0;
            for request in &self.prefetch_issued {
                if request.time != key.time {
                    continue;
                }
                issued += 1;
                if self
                    .observed
                    .contains(&(request.level, index_of(request, pyramid)))
                {
                    hit += 1;
                }
            }
            if issued > 0 {
                let rate = hit as f32 / issued as f32;
                self.prefetch_rate = 0.7 * self.prefetch_rate + 0.3 * rate.clamp(0.25, 1.0);
            }
            self.prefetch_issued.clear();
        }
        for band in bands {
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
        if self.stale {
            self.rebuild(pyramid, atlas);
        }
        let demand = self.demand.as_ref().expect("the demand is staged");
        let mut requests = Requests {
            time: demand.key.time,
            ..Default::default()
        };
        let mut view = ViewInfo {
            level: demand.key.levels.ladder.base,
            ..Default::default()
        };
        for (band, demand) in demand.bands.iter().enumerate() {
            view.visible += demand.info.visible;
            view.demand += demand.info.demand;
            view.resident += demand.info.demand - self.missing.absent[band];
        }
        self.refreshes.retain(|request| {
            let grid = BrickGrid::new(pyramid.level(request.level).extent());
            let index = grid.index(request.coord);
            atlas.slot(request.time, request.level, index).is_some()
        });
        requests.volume.extend(self.refreshes.iter().copied());
        requests.volume.extend(self.missing.requests());
        if requests.volume.is_empty() && prefetch {
            let issued = prefetch_requests(
                &Prefetch {
                    rate: self.prefetch_rate,
                    atlas,
                    time: demand.key.time,
                    ladder: demand.key.levels.ladder,
                    demands: &demand.bands,
                    pyramid,
                },
                &mut self.prefetched,
            );
            self.prefetch_issued.extend_from_slice(&issued);
            requests.prefetch = issued;
        }
        self.view = view;
        requests.view = view;
        Some(requests)
    }

    fn rebuild(&mut self, pyramid: &Pyramid, atlas: &Atlas) {
        self.stale = false;
        let Some(demand) = self.demand.as_ref() else {
            self.missing.clear();
            return;
        };
        self.missing.begin(demand.key.time);
        for (band, entries) in demand.bands.iter().enumerate() {
            let grid = BrickGrid::new(pyramid.level(entries.info.level).extent());
            for index in &entries.indices {
                self.missing.demands(entries.info.level, *index);
            }
            for (request, bucket) in entries.requests.iter().zip(&entries.buckets) {
                let index = grid.index(request.coord);
                if atlas.slot(request.time, request.level, index).is_some()
                    || atlas.is_hollow(request.time, request.level, index)
                {
                    continue;
                }
                self.missing
                    .want(*bucket, band, request.level, index as u32, *request);
            }
        }
    }

    pub(crate) fn supply(&mut self, time: u32, level: usize, index: usize) {
        if self.stale
            || self
                .demand
                .as_ref()
                .is_none_or(|demand| demand.key.time != time)
        {
            return;
        }
        self.missing.supply(level, index as u32);
    }

    pub(crate) fn release(&mut self, pyramid: &Pyramid, time: u32, level: usize, index: usize) {
        if self.stale {
            return;
        }
        let Some(demand) = self.demand.as_ref() else {
            return;
        };
        if demand.key.time != time {
            return;
        }
        let ladder = demand.key.levels.ladder;
        let Some(band) = level
            .checked_sub(ladder.base)
            .filter(|band| *band < ladder.count)
        else {
            return;
        };
        if !self.missing.demanded(level, index as u32) {
            return;
        }
        let grid = BrickGrid::new(pyramid.level(level).extent());
        let request = BrickRequest::new(time, level, grid.coord(index));
        self.missing
            .want(RESTORE_BUCKET, band, level, index as u32, request);
    }

    pub(crate) fn restale(&mut self) {
        self.stale = true;
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

fn view_key(meta: &ViewKey) -> u64 {
    let mut hasher = DefaultHasher::new();
    meta.epoch.hash(&mut hasher);
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

struct Prefetch<'a> {
    rate: f32,
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
        rate,
        atlas,
        time,
        ladder,
        demands,
        pyramid,
    } = *prefetch;
    let total = ((atlas.slots() as f32 / PREFETCH_SHARE as f32) * rate.clamp(0.25, 1.0))
        .min(PREFETCH_LIMIT as f32) as usize;
    let next = carried_frame(time, pyramid.frame_count());
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
