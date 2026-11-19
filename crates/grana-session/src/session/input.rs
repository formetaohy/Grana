use grana_scene::ObjectSetId;

use super::*;

impl Session {
    pub(super) fn coloc_input(&self) -> Option<Input> {
        let scene = self.scene.as_ref()?;
        let layout = self.channels.as_ref()?.layout();
        Some(Input::coloc(
            layout,
            scene.drifts().digest(),
            scene.coloc.pair()?,
            scene.coloc.settings(),
            scene.region,
            scene.mask_key(),
        ))
    }

    pub(super) fn object_coloc_input(&self, set: ObjectSetId) -> Option<Input> {
        let scene = self.scene.as_ref()?;
        let layout = self.channels.as_ref()?.layout();
        let object = scene.objects.get(set)?;
        Some(Input::object_coloc(
            layout,
            scene.drifts().digest(),
            scene.coloc.pair()?,
            scene.region,
            set,
            (0..object.frame_count())
                .map(|time| object.frame_stamp(time))
                .collect(),
        ))
    }

    pub(super) fn detect_input(&self, set: ObjectSetId) -> Option<Input> {
        let scene = self.scene.as_ref()?;
        let layout = self.channels.as_ref()?.layout();
        let object = scene.objects.get(set)?;
        let filter_target = scene
            .mask_filter
            .is_some_and(|filter| filter.target() == set);
        Some(Input::detect(
            layout,
            scene.drifts().digest(),
            object.channel(),
            object.params(),
            scene.region,
            (!filter_target).then(|| scene.mask_key()).flatten(),
            seed_digest(scene, object),
        ))
    }

    pub(super) fn track_input(&self, set: ObjectSetId) -> Option<Input> {
        let object = self.scene.as_ref()?.objects.get(set)?;
        let params = object.track_style()?.params();
        Some(Input::track(
            (0..object.frame_count())
                .map(|time| object.frame_stamp(time))
                .collect(),
            params,
        ))
    }

    pub(super) fn drift_input(&self, channel: usize, reference: u32) -> Input {
        Input::drift(crate::input::DriftInput::channels(channel, reference))
    }

    pub(super) fn localization_drift_input(&self, set: ObjectSetId) -> Option<Input> {
        let scene = self.scene.as_ref()?;
        let object = scene.objects.get(set)?;
        let channel = object.channel();
        let base = crate::input::drift_base(scene, channel);
        Some(Input::drift(crate::input::DriftInput::localizations(
            channel,
            base.reference(),
            set,
            object.revision(),
            base.digest(),
        )))
    }

    pub(super) fn register_input(&self, reference: usize) -> Input {
        Input::register(crate::input::RegisterInput::channels(reference))
    }

    pub(super) fn fiducial_input(&self, reference: usize, sets: &[ObjectSetId]) -> Option<Input> {
        let scene = self.scene.as_ref()?;
        let sources = sets
            .iter()
            .map(|set| Some((*set, scene.objects.get(*set)?.revision())))
            .collect::<Option<Vec<_>>>()?;
        Some(Input::register(crate::input::RegisterInput::localizations(
            reference, sources,
        )))
    }

    pub(super) fn drift_current(&self, held: &crate::input::DriftInput) -> bool {
        self.scene
            .as_ref()
            .and_then(|scene| held.current(scene))
            .is_some_and(|current| &current == held)
    }

    pub(super) fn register_current(&self, held: &crate::input::RegisterInput) -> bool {
        self.scene
            .as_ref()
            .and_then(|scene| held.current(scene))
            .is_some_and(|current| &current == held)
    }
}

fn seed_digest(scene: &grana_scene::Scene, object: &grana_scene::ObjectSet) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    match object.seed_source() {
        Some(source) => {
            source.raw().hash(&mut hasher);
            let frames = scene
                .objects
                .get(source)
                .map_or(0, grana_scene::ObjectSet::frame_count);
            frames.hash(&mut hasher);
            for time in 0..frames {
                scene
                    .objects
                    .get(source)
                    .and_then(|held| held.frame_stamp(time))
                    .map(grana_scene::Revision::raw)
                    .hash(&mut hasher);
            }
        }
        None => {
            object.frame_count().hash(&mut hasher);
            for time in 0..object.frame_count() {
                let seeds = object.seeds(time);
                seeds.len().hash(&mut hasher);
                for seed in seeds {
                    for coordinate in seed {
                        coordinate.to_bits().hash(&mut hasher);
                    }
                }
            }
        }
    }
    hasher.finish()
}
