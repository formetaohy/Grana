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
        ))
    }

    pub(super) fn track_input(&self, set: ObjectSetId) -> Option<Input> {
        let object = self.scene.as_ref()?.objects.get(set)?;
        Some(Input::track(
            (0..object.frame_count())
                .map(|time| object.frame_stamp(time))
                .collect(),
        ))
    }
}
