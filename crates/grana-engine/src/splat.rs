use std::sync::Arc;

use grana_object::{SpotWeight, SpotWeightSampler};
use grana_render::{Digest, SplatChunk, SplatGeometry, SplatLevel, SplatStyle};
use grana_scene::{Item, ObjectSet, ObjectSetId, Scene};

use crate::spot::{self, Appearance, Choice, Frame, LEVEL_COUNT};

const _: () = assert!(LEVEL_COUNT == grana_render::SPLAT_LEVELS);

const SPLAT_LIMIT: usize = 1 << 21;
const WEIGHT_FRAMES: usize = 48;

pub struct SplatCache {
    sets: Vec<SetLevels>,
}

impl Default for SplatCache {
    fn default() -> Self {
        Self::new()
    }
}

impl SplatCache {
    pub fn new() -> Self {
        Self { sets: Vec::new() }
    }

    pub fn clear(&mut self) {
        self.sets.clear();
    }

    pub(crate) fn retain(&mut self, present: &[u64]) {
        self.sets.retain(|entry| present.contains(&entry.set));
    }

    pub(crate) fn choose(
        &mut self,
        scene: &Scene,
        set: &ObjectSet,
        world_per_pixel: f32,
    ) -> Choice {
        let range = range(set, scene.time(), scene.frame_count());
        let projected = range[0] != range[1];
        let aggregated = set.object_count(scene.time()) >= spot::AGGREGATE_FLOOR;
        let stamp = set.revision().raw();
        let index = self.entry(set);
        let entry = &mut self.sets[index];
        if entry
            .ladder
            .as_ref()
            .is_none_or(|held| held.stamp != stamp || held.range != range)
        {
            let ladder = spot::Ladder::over(&frames(scene, set));
            entry.ladder = Some(LadderSample {
                stamp,
                range,
                ladder,
            });
        }
        let ladder = entry.ladder.as_ref().and_then(|held| held.ladder);
        spot::choose(projected, aggregated, ladder, world_per_pixel)
    }

    fn entry(&mut self, set: &ObjectSet) -> usize {
        match self
            .sets
            .iter()
            .position(|entry| entry.set == set.id().raw())
        {
            Some(index) => index,
            None => {
                self.sets.push(SetLevels {
                    set: set.id().raw(),
                    weight: None,
                    ladder: None,
                    levels: std::array::from_fn(|_| None),
                });
                self.sets.len() - 1
            }
        }
    }

    pub(crate) fn prepare(
        &mut self,
        scene: &Scene,
        set: &ObjectSet,
        key: u64,
        choice: Choice,
    ) -> Prepared {
        let range = range(set, scene.time(), scene.frame_count());
        let projected = range[0] != range[1];
        let present = (range[0]..=range[1]).any(|time| set.frame_stamp(time).is_some());
        let index = self.entry(set);
        let entry = &mut self.sets[index];
        if !present {
            entry.levels = std::array::from_fn(|_| None);
            return Prepared {
                chunks: Vec::new(),
                members: SplatMembers {
                    key,
                    set: set.id(),
                    levels: std::array::from_fn(|_| None),
                },
            };
        }
        let stamp = set.revision().raw();
        if entry.weight.as_ref().is_none_or(|held| {
            held.stamp != stamp || held.range != range || held.projected != projected
        }) {
            entry.weight = Some(MeasuredWeight {
                stamp,
                range,
                projected,
                weight: measure_weight(set, range, projected),
            });
        }
        let weight = entry.weight.as_ref().and_then(|held| held.weight);
        let choice = if projected {
            Choice::Level(choice.level().max(1))
        } else {
            choice
        };
        let wanted = if projected {
            vec![choice.level()]
        } else if set.object_count(scene.time()) >= spot::AGGREGATE_FLOOR {
            (0..LEVEL_COUNT).collect()
        } else {
            vec![0]
        };
        for (slot, level) in entry.levels.iter_mut().enumerate() {
            if !wanted.contains(&(slot as u8)) {
                *level = None;
                continue;
            }
            if level.as_ref().is_some_and(|level| {
                let stamp = stamps(set, slot as u8, range);
                stamp.geometry != level.geometry_stamp || stamp.style != level.style_stamp
            }) {
                *level = None;
            }
        }
        let mut held = None;
        for slot in wanted {
            let level = &mut entry.levels[slot as usize];
            if level.is_none() {
                let frames = held.get_or_insert_with(|| frames(scene, set));
                *level = Some(build(scene, set, slot, frames, range, weight));
            }
        }
        let levels = &entry.levels;
        let active = choice.level();
        let mut chunks = Vec::new();
        for (slot, level) in levels.iter().enumerate() {
            let Some(level) = level else {
                continue;
            };
            chunks.push(
                SplatChunk::new(
                    key,
                    slot as u8,
                    slot as u8 == active,
                    SplatLevel::new(
                        level.geometry_stamp,
                        level.style_stamp,
                        Arc::clone(&level.geometry),
                        Arc::clone(&level.style),
                    ),
                )
                .pickable(level.members.is_some()),
            );
        }
        Prepared {
            chunks,
            members: SplatMembers {
                key,
                set: set.id(),
                levels: std::array::from_fn(|slot| {
                    levels[slot]
                        .as_ref()
                        .and_then(|level| level.members.as_ref().map(Arc::clone))
                }),
            },
        }
    }
}

pub(crate) fn range(set: &ObjectSet, time: u32, frames: u32) -> [u32; 2] {
    set.localization()
        .map_or([time, time], |source| source.projected_frames(time, frames))
}

pub(crate) fn frames(scene: &Scene, set: &ObjectSet) -> Vec<Frame> {
    let [start, end] = range(set, scene.time(), scene.frame_count());
    (start..=end)
        .filter_map(|time| Some(Frame::new(time, set.spot_frame(time)?)))
        .collect()
}

fn measure_weight(set: &ObjectSet, range: [u32; 2], projected: bool) -> Option<SpotWeight> {
    set.localization()?;
    let sampled = if projected {
        sample_frames(range[0], range[1])
    } else {
        sample_frames(0, set.frame_count().saturating_sub(1))
    };
    let mut picked = Vec::with_capacity(sampled.len());
    for time in sampled {
        if let Some(spots) = set.spot_frame(time) {
            picked.push(spots);
        }
    }
    let total = picked.iter().map(|spots| spots.len()).sum::<usize>();
    if total == 0 {
        return None;
    }
    let mut sampler = SpotWeightSampler::new(total);
    for spots in &picked {
        sampler.push(spots.spots());
    }
    Some(sampler.finish())
}

fn sample_frames(start: u32, end: u32) -> Vec<u32> {
    if end <= start {
        return vec![start];
    }
    let span = u64::from(end - start);
    let sampled = (span + 1).min(WEIGHT_FRAMES as u64);
    (0..sampled)
        .map(|index| start + (index * span / (sampled - 1)) as u32)
        .collect()
}

pub(crate) struct Prepared {
    pub(crate) chunks: Vec<SplatChunk>,
    pub(crate) members: SplatMembers,
}

#[derive(Clone)]
pub struct SplatMembers {
    key: u64,
    set: ObjectSetId,
    levels: [Option<Arc<Vec<u32>>>; LEVEL_COUNT as usize],
}

impl SplatMembers {
    pub(crate) fn new(key: u64, set: ObjectSetId, members: Vec<u32>) -> Self {
        let mut levels = std::array::from_fn(|_| None);
        levels[0] = Some(Arc::new(members));
        Self { key, set, levels }
    }

    pub(crate) fn standalone(key: u64, set: ObjectSetId, count: usize) -> Self {
        Self::new(key, set, vec![crate::spot::STANDALONE; count])
    }

    pub fn key(&self) -> u64 {
        self.key
    }

    pub fn members(&self, level: u8) -> Option<&[u32]> {
        self.levels
            .get(level as usize)?
            .as_ref()
            .map(|members| members.as_slice())
    }

    pub fn resolve(&self, level: u8, index: u32) -> Option<(ObjectSetId, u32)> {
        let members = self.levels.get(level as usize)?.as_ref()?;
        members
            .get(index as usize)
            .map(|member| (self.set, *member))
    }
}

struct SetLevels {
    set: u64,
    weight: Option<MeasuredWeight>,
    ladder: Option<LadderSample>,
    levels: [Option<Level>; LEVEL_COUNT as usize],
}

struct MeasuredWeight {
    stamp: u64,
    range: [u32; 2],
    projected: bool,
    weight: Option<SpotWeight>,
}

struct LadderSample {
    stamp: u64,
    range: [u32; 2],
    ladder: Option<spot::Ladder>,
}

struct Level {
    geometry_stamp: u64,
    style_stamp: u64,
    geometry: Arc<Vec<SplatGeometry>>,
    style: Arc<Vec<SplatStyle>>,
    members: Option<Arc<Vec<u32>>>,
}

#[derive(Clone, Copy)]
struct Stamps {
    geometry: u64,
    style: u64,
}

fn build(
    scene: &Scene,
    set: &ObjectSet,
    level: u8,
    frames: &[Frame],
    range: [u32; 2],
    weight: Option<SpotWeight>,
) -> Level {
    let mut choice = match level {
        0 => Choice::Full,
        level => Choice::Level(level),
    };
    if frames.len() > 1 && choice == Choice::Full {
        choice = Choice::Level(1);
    }
    let stamp = stamps(set, level, range);
    let group_colored = set
        .localization()
        .is_some_and(grana_scene::LocalizationSource::group_colored);
    let colors = (frames.len() == 1 && !group_colored)
        .then(|| scene.object_colors(set))
        .flatten();
    let appearance = |time: u32, index: usize, spot: &grana_object::Spot| Appearance {
        color: if group_colored {
            grana_display::group_tint(spot.group())
        } else {
            colors
                .as_ref()
                .and_then(|colors| colors.get(index).copied())
                .unwrap_or_else(|| set.color())
        },
        visible: set.visible_object(time, index),
        categorical: group_colored,
        weight: weight.map_or(1.0, |scale| scale.of(spot)),
    };
    let mut detections = spot::detections(frames, choice, set.opacity(), appearance);
    let mut rung = choice.level();
    while detections.len() > SPLAT_LIMIT && rung < LEVEL_COUNT - 1 {
        rung += 1;
        detections = spot::detections(frames, Choice::Level(rung), set.opacity(), appearance);
    }
    let members = (frames.len() == 1).then(|| Arc::new(detections.members));
    Level {
        geometry_stamp: stamp.geometry,
        style_stamp: stamp.style,
        geometry: Arc::new(detections.geometry),
        style: Arc::new(detections.style),
        members,
    }
}

fn stamps(set: &ObjectSet, level: u8, range: [u32; 2]) -> Stamps {
    let mut geometry = Digest::new();
    geometry.mix(set.id().raw());
    geometry.mix(
        set.frame_stamp(range[0])
            .map_or(u64::MAX, |stamp| stamp.raw()),
    );
    geometry.mix(
        set.frame_stamp(range[1])
            .map_or(u64::MAX, |stamp| stamp.raw()),
    );
    geometry.mix(u64::from(range[0]));
    geometry.mix(u64::from(range[1]));
    geometry.mix(u64::from(level));
    let mut style = Digest::new();
    style.mix(set.id().raw());
    style.mix(set.document().raw());
    style.real(set.opacity());
    style.mix(u64::from(range[0]));
    style.mix(u64::from(range[1]));
    style.mix(u64::from(level));
    Stamps {
        geometry: geometry.finish(),
        style: style.finish(),
    }
}

pub(crate) fn selection_digest(scene: &Scene, set: &ObjectSet) -> u64 {
    let mut digest = Digest::new();
    for held in scene.selections() {
        if held.set == set.id() {
            digest.mix(match held.item {
                Item::Object(index) => (index as u64) * 2,
                Item::Track(index) => (index as u64) * 2 + 1,
            });
        }
    }
    digest.finish()
}
