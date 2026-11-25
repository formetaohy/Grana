use std::sync::Arc;

use grana_render::{Digest, Style};
use grana_scene::{ObjectSet, ObjectSetId, Scene, Selection};

use crate::splat::selection_digest;
use crate::view::TRACK_PART;

pub(crate) struct StyleCache {
    entries: Vec<Entry>,
}

struct Entry {
    set: ObjectSetId,
    part: u32,
    digest: u64,
    classification: u64,
    styles: Arc<[Style]>,
    ids: Arc<[Selection]>,
}

pub(crate) struct Rows {
    digest: u64,
    classification: u64,
    styles: Arc<[Style]>,
    ids: Arc<[Selection]>,
}

impl Rows {
    pub(crate) fn digest(&self) -> u64 {
        self.digest
    }

    pub(crate) fn classification(&self) -> u64 {
        self.classification
    }

    pub(crate) fn styles(&self) -> Arc<[Style]> {
        Arc::clone(&self.styles)
    }

    pub(crate) fn ids(&self) -> Arc<[Selection]> {
        Arc::clone(&self.ids)
    }
}

impl StyleCache {
    pub(crate) fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
    }

    pub(crate) fn retain(&mut self, present: &[ObjectSetId]) {
        self.entries.retain(|entry| present.contains(&entry.set));
    }

    pub(crate) fn rows(
        &mut self,
        scene: &Scene,
        set: &ObjectSet,
        part: u32,
        count: usize,
        fill: impl FnOnce(&mut Vec<Style>, &mut Vec<Selection>),
    ) -> Rows {
        let digest = digest(scene, set, part, count);
        let index = match self
            .entries
            .iter()
            .position(|entry| entry.set == set.id() && entry.part == part)
        {
            Some(index) => index,
            None => {
                self.entries.push(Entry {
                    set: set.id(),
                    part,
                    digest: 0,
                    classification: 0,
                    styles: Arc::from(Vec::new()),
                    ids: Arc::from(Vec::new()),
                });
                self.entries.len() - 1
            }
        };
        let entry = &mut self.entries[index];
        if entry.digest != digest {
            let mut styles = Vec::with_capacity(count);
            let mut ids = Vec::with_capacity(count);
            fill(&mut styles, &mut ids);
            assert_eq!(
                styles.len(),
                count,
                "a style cache fills one style per object"
            );
            assert_eq!(ids.len(), count, "a style cache fills one id per object");
            entry.classification = classification(&styles);
            entry.styles = Arc::from(styles);
            entry.ids = Arc::from(ids);
            entry.digest = digest;
        }
        Rows {
            digest: entry.digest,
            classification: entry.classification,
            styles: Arc::clone(&entry.styles),
            ids: Arc::clone(&entry.ids),
        }
    }
}

fn classification(styles: &[Style]) -> u64 {
    let mut digest = Digest::new();
    for style in styles {
        digest.flag(style.visible());
        digest.flag(style.transparent());
    }
    digest.finish()
}

fn digest(scene: &Scene, set: &ObjectSet, part: u32, count: usize) -> u64 {
    let mut digest = Digest::new();
    let material = set.material();
    let time = scene.time();
    digest.mix(set.id().raw());
    digest.mix(u64::from(part));
    digest.mix(count as u64);
    digest.mix(set.document().raw());
    digest.mix(set.revision().raw());
    digest.mix(
        set.frame_appearance(time)
            .map_or(u64::MAX, |held| held.raw()),
    );
    digest.mix(
        set.tracks()
            .map_or(u64::MAX, |tracks| tracks.revision().raw()),
    );
    if part == TRACK_PART
        && let Some(tracks) = set.tracks()
    {
        for track in tracks.set().tracks() {
            digest.mix(u64::from(track.id().raw()));
        }
    }
    digest.mix(scene.table_revision(set.id()).unwrap_or(u64::MAX));
    digest.mix(selection_digest(scene, set));
    digest.mix(u64::from(set.opacity().to_bits()));
    digest.real(material.roughness());
    digest.real(material.metallic());
    digest.finish()
}
