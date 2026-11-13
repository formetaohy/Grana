use std::sync::Arc;

use grana_render::{Digest, SplatChunk, SplatGeometry, SplatLevel, SplatStyle};
use grana_scene::{Item, ObjectSet, ObjectSetId, Scene};

use crate::spot::{self, Appearance, Choice, LEVEL_COUNT};

const _: () = assert!(LEVEL_COUNT == grana_render::SPLAT_LEVELS);

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

    pub(crate) fn prepare(
        &mut self,
        scene: &Scene,
        set: &ObjectSet,
        key: u64,
        choice: Choice,
    ) -> Prepared {
        let time = scene.time();
        let index = match self
            .sets
            .iter()
            .position(|entry| entry.set == set.id().raw())
        {
            Some(index) => index,
            None => {
                self.sets.push(SetLevels {
                    set: set.id().raw(),
                    levels: std::array::from_fn(|_| None),
                });
                self.sets.len() - 1
            }
        };
        let entry = &mut self.sets[index];
        let Some(spots) = set.frame(time).and_then(|frame| frame.spots()) else {
            entry.levels = std::array::from_fn(|_| None);
            return Prepared {
                chunks: Vec::new(),
                members: SplatMembers {
                    key,
                    set: set.id(),
                    levels: std::array::from_fn(|_| None),
                },
            };
        };
        let rungs = if spot::aggregated(spots) {
            LEVEL_COUNT
        } else {
            1
        };
        for (_, level) in entry.levels.iter_mut().enumerate().skip(rungs as usize) {
            *level = None;
        }
        for (slot, level) in entry.levels.iter_mut().enumerate().take(rungs as usize) {
            if level.as_ref().is_some_and(|level| {
                let stamp = stamps(set, slot as u8, time);
                stamp.geometry != level.geometry_stamp || stamp.style != level.style_stamp
            }) {
                *level = None;
            }
        }
        for (slot, level) in entry.levels.iter_mut().enumerate().take(rungs as usize) {
            if level.is_none() {
                *level = Some(build(scene, set, slot as u8, time));
            }
        }
        let levels = &entry.levels;
        let active = choice.level();
        let mut chunks = Vec::new();
        for (slot, level) in levels.iter().enumerate() {
            let Some(level) = level else {
                continue;
            };
            chunks.push(SplatChunk::new(
                key,
                slot as u8,
                slot as u8 == active,
                SplatLevel::new(
                    level.geometry_stamp,
                    level.style_stamp,
                    Arc::clone(&level.geometry),
                    Arc::clone(&level.style),
                ),
            ));
        }
        Prepared {
            chunks,
            members: SplatMembers {
                key,
                set: set.id(),
                levels: std::array::from_fn(|slot| {
                    levels[slot]
                        .as_ref()
                        .map(|level| Arc::clone(&level.members))
                }),
            },
        }
    }
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
    levels: [Option<Level>; LEVEL_COUNT as usize],
}

struct Level {
    geometry_stamp: u64,
    style_stamp: u64,
    geometry: Arc<Vec<SplatGeometry>>,
    style: Arc<Vec<SplatStyle>>,
    members: Arc<Vec<u32>>,
}

#[derive(Clone, Copy)]
struct Stamps {
    geometry: u64,
    style: u64,
}

fn build(scene: &Scene, set: &ObjectSet, level: u8, time: u32) -> Level {
    let choice = match level {
        0 => Choice::Full,
        level => Choice::Level(level),
    };
    let stamp = stamps(set, level, time);
    let spots = set
        .frame(time)
        .and_then(|frame| frame.spots())
        .expect("a spot set carries spots on its frame");
    let colors = scene.object_colors(set);
    let appearance = |index: usize| Appearance {
        color: colors
            .as_ref()
            .and_then(|colors| colors.get(index).copied())
            .unwrap_or_else(|| set.color()),
        visible: set.visible_object(time, index),
    };
    let detections = spot::detections(spots, choice, set.opacity(), appearance);
    Level {
        geometry_stamp: stamp.geometry,
        style_stamp: stamp.style,
        geometry: Arc::new(detections.geometry),
        style: Arc::new(detections.style),
        members: Arc::new(detections.members),
    }
}

fn stamps(set: &ObjectSet, level: u8, time: u32) -> Stamps {
    let mut geometry = Digest::new();
    geometry.mix(set.id().raw());
    geometry.mix(set.frame_stamp(time).map_or(u64::MAX, |stamp| stamp.raw()));
    geometry.mix(u64::from(time));
    geometry.mix(u64::from(level));
    let mut style = Digest::new();
    style.mix(set.id().raw());
    style.mix(set.document().raw());
    style.real(set.opacity());
    style.mix(u64::from(time));
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
