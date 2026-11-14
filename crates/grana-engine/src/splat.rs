use std::cell::OnceCell;
use std::sync::Arc;

use grana_object::{SpotWeight, SpotWeightSampler};
use grana_render::{Digest, SplatChunk, SplatGeometry, SplatLevel, SplatStyle};
use grana_scene::{Item, ObjectSet, ObjectSetId, Scene};

use crate::spot::{self, Choice, Frame, LEVEL_COUNT};

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
                    levels: std::array::from_fn(|_| Level::default()),
                    color: None,
                    seq: 0,
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
            entry.levels = std::array::from_fn(|_| Level::default());
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
        let object_frames = frames(scene, set);
        let group_colored = set
            .localization()
            .is_some_and(grana_scene::LocalizationSource::group_colored);
        let per_object = object_frames.len() == 1 && !group_colored;
        let colors = OnceCell::new();
        let color = |index: usize, spot: &grana_object::Spot| -> [f32; 3] {
            if group_colored {
                grana_display::group_tint(spot.group())
            } else if per_object {
                colors
                    .get_or_init(|| scene.object_colors(set))
                    .as_ref()
                    .and_then(|colors| colors.get(index).copied())
                    .unwrap_or_else(|| set.color())
            } else {
                set.color()
            }
        };
        let visible = |time: u32, index: usize| set.visible_object(time, index);
        let look = spot::Look {
            color: &color,
            visible: &visible,
            weight,
            categorical: group_colored,
        };
        let colored = if per_object {
            let state = set.object_coloring().state();
            let count = object_frames[0].spots.len();
            let table = scene.table_revision(set.id());
            let revision = set.revision().raw();
            let time = scene.time();
            let fresh = entry.color.as_ref().is_none_or(|held| {
                held.state != state
                    || held.color != set.color()
                    || held.count != count
                    || held.table != table
                    || held.revision != revision
                    || held.time != time
            });
            if fresh {
                entry.seq += 1;
                entry.color = Some(ColorSample {
                    state,
                    color: set.color(),
                    count,
                    table,
                    revision,
                    time,
                    stamp: stamp_of(&[set.id().raw(), entry.seq]),
                });
            }
            entry
                .color
                .as_ref()
                .expect("the color sample is fresh")
                .stamp
        } else {
            color_stamp(set, group_colored)
        };
        let shape = set.shape().raw();
        let opacity = u64::from(set.opacity().to_bits());
        for (slot, level) in entry.levels.iter_mut().enumerate() {
            if !wanted.contains(&(slot as u8)) {
                *level = Level::default();
                continue;
            }
            let choice = if slot == 0 {
                Choice::Full
            } else {
                Choice::Level(slot as u8)
            };
            let kind = spot::kind(&object_frames, choice);
            let data = data_stamp(set, slot as u8, range);
            let geometry = match kind {
                spot::Kind::Cells => stamp_of(&[data, shape, colored]),
                spot::Kind::Spots => stamp_of(&[data]),
            };
            let style = match kind {
                spot::Kind::Cells => stamp_of(&[geometry, opacity]),
                spot::Kind::Spots => stamp_of(&[data, shape, colored, opacity]),
            };
            if level
                .geometry
                .as_ref()
                .is_none_or(|held| held.stamp != geometry)
            {
                let pickable = match kind {
                    spot::Kind::Spots => true,
                    spot::Kind::Cells => object_frames.len() == 1,
                };
                let mut rung = match kind {
                    spot::Kind::Spots => 0,
                    spot::Kind::Cells => (slot as u8).max(1),
                };
                let built = loop {
                    let built = spot::geometry(&object_frames, kind, rung, look, pickable);
                    if built.len() <= spot::SPLAT_LIMIT || rung >= LEVEL_COUNT - 1 {
                        break built;
                    }
                    rung += 1;
                };
                level.geometry = Some(CachedGeometry {
                    stamp: geometry,
                    members: built
                        .members
                        .as_ref()
                        .map(|members| Arc::new(members.clone())),
                    cells: Arc::clone(&built.cells),
                    paint: built.paint,
                });
            }
            if level.style.as_ref().is_none_or(|held| held.stamp != style) {
                let held = level
                    .geometry
                    .as_ref()
                    .expect("the level geometry is cached before its styles");
                let styles = spot::styles(&held.paint, set.opacity(), look);
                level.style = Some(CachedStyle {
                    stamp: style,
                    styles: Arc::new(styles),
                });
            }
        }
        let mut chunks = Vec::new();
        let active = choice.level();
        for (slot, level) in entry.levels.iter().enumerate() {
            let (Some(geometry), Some(style)) = (level.geometry.as_ref(), level.style.as_ref())
            else {
                continue;
            };
            chunks.push(
                SplatChunk::new(
                    key,
                    slot as u8,
                    slot as u8 == active,
                    SplatLevel::new(
                        geometry.stamp,
                        style.stamp,
                        Arc::clone(&geometry.cells),
                        Arc::clone(&style.styles),
                    ),
                )
                .pickable(geometry.members.is_some()),
            );
        }
        Prepared {
            chunks,
            members: SplatMembers {
                key,
                set: set.id(),
                levels: std::array::from_fn(|slot| {
                    entry.levels[slot]
                        .geometry
                        .as_ref()
                        .and_then(|geometry| geometry.members.as_ref().map(Arc::clone))
                }),
            },
        }
    }
}

struct CachedGeometry {
    stamp: u64,
    members: Option<Arc<Vec<u32>>>,
    cells: Arc<Vec<SplatGeometry>>,
    paint: spot::Paint,
}

struct CachedStyle {
    stamp: u64,
    styles: Arc<Vec<SplatStyle>>,
}

#[derive(Default)]
struct Level {
    geometry: Option<CachedGeometry>,
    style: Option<CachedStyle>,
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
    levels: [Level; LEVEL_COUNT as usize],
    color: Option<ColorSample>,
    seq: u64,
}

struct ColorSample {
    state: grana_display::ColorState<grana_scene::ObjectColor>,
    color: [f32; 3],
    count: usize,
    table: Option<u64>,
    revision: u64,
    time: u32,
    stamp: u64,
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

fn data_stamp(set: &ObjectSet, level: u8, range: [u32; 2]) -> u64 {
    let mut digest = Digest::new();
    digest.mix(set.id().raw());
    for time in range[0]..=range[1] {
        digest.mix(set.frame_stamp(time).map_or(u64::MAX, |stamp| stamp.raw()));
    }
    digest.mix(u64::from(range[0]));
    digest.mix(u64::from(range[1]));
    digest.mix(u64::from(level));
    digest.finish()
}

fn color_stamp(set: &ObjectSet, group_colored: bool) -> u64 {
    let mut digest = Digest::new();
    digest.flag(group_colored);
    if group_colored {
        return digest.finish();
    }
    for component in set.color() {
        digest.real(component);
    }
    digest.finish()
}

fn stamp_of(words: &[u64]) -> u64 {
    let mut digest = Digest::new();
    for word in words {
        digest.mix(*word);
    }
    digest.finish()
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
