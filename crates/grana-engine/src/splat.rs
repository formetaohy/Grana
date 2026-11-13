use std::sync::Arc;

use glam::Vec3;
use grana_render::{Digest, SPLAT_LEVELS, SplatChunk, SplatGeometry, SplatLevel, SplatStyle};
use grana_scene::{Item, ObjectSet, ObjectSetId, Scene};

use crate::spot::{self, Appearance, Choice};

const FULL: usize = 0;
const PROXY: usize = 1;

const _: () = assert!(SPLAT_LEVELS == 2);

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
        world: Vec3,
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
        if set.frame(time).and_then(|frame| frame.spots()).is_none() {
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
        for slot in 0..SPLAT_LEVELS as usize {
            let stale = entry.levels[slot].as_ref().is_some_and(|level| {
                let stamp = stamps(set, level.choice, world, time);
                stamp.geometry != level.geometry_stamp || stamp.style != level.style_stamp
            });
            if stale {
                entry.levels[slot] = None;
            }
        }
        let active = match choice {
            Choice::Full => FULL,
            Choice::Grid(_) => PROXY,
        };
        match choice {
            Choice::Full => {
                if entry.levels[FULL].is_none() {
                    entry.levels[FULL] = Some(build(scene, set, Choice::Full, world, time));
                }
            }
            Choice::Grid(_) => {
                let settled = entry.levels[PROXY]
                    .as_ref()
                    .is_some_and(|level| level.choice == choice);
                if !settled {
                    entry.levels[PROXY] = Some(build(scene, set, choice, world, time));
                }
            }
        }
        let levels = &entry.levels;
        let mut chunks = Vec::new();
        for (slot, level) in levels.iter().enumerate() {
            let Some(level) = level else {
                continue;
            };
            chunks.push(SplatChunk::new(
                key,
                slot as u8,
                slot == active,
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
                levels: [
                    levels[FULL]
                        .as_ref()
                        .map(|level| Arc::clone(&level.members)),
                    levels[PROXY]
                        .as_ref()
                        .map(|level| Arc::clone(&level.members)),
                ],
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
    levels: [Option<Arc<Vec<u32>>>; SPLAT_LEVELS as usize],
}

impl SplatMembers {
    pub(crate) fn new(key: u64, set: ObjectSetId, members: Vec<u32>) -> Self {
        Self {
            key,
            set,
            levels: [Some(Arc::new(members)), None],
        }
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
    levels: [Option<Level>; SPLAT_LEVELS as usize],
}

struct Level {
    choice: Choice,
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

fn build(scene: &Scene, set: &ObjectSet, choice: Choice, world: Vec3, time: u32) -> Level {
    let stamp = stamps(set, choice, world, time);
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
    let detections = spot::detections(spots, choice, world, set.opacity(), appearance);
    Level {
        choice,
        geometry_stamp: stamp.geometry,
        style_stamp: stamp.style,
        geometry: Arc::new(detections.geometry),
        style: Arc::new(detections.style),
        members: Arc::new(detections.members),
    }
}

fn stamps(set: &ObjectSet, choice: Choice, world: Vec3, time: u32) -> Stamps {
    let proxy = matches!(choice, Choice::Grid(_));
    let mut geometry = Digest::new();
    geometry.mix(set.id().raw());
    geometry.mix(set.frame_stamp(time).map_or(u64::MAX, |stamp| stamp.raw()));
    geometry.mix(u64::from(time));
    geometry.real(world.x);
    geometry.real(world.y);
    geometry.real(world.z);
    if proxy {
        geometry.mix(choice.digest());
        geometry.mix(set.document().raw());
    }
    let mut style = Digest::new();
    style.mix(set.id().raw());
    style.mix(set.document().raw());
    style.real(set.opacity());
    style.mix(u64::from(time));
    if proxy {
        style.mix(choice.digest());
    }
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
