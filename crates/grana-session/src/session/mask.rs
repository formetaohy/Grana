use grana_pipeline::Channels;
use grana_scene::ObjectSetId;

use super::*;

impl Session {
    pub(super) fn install_channels(&mut self) {
        let Some(image) = self
            .source
            .as_ref()
            .and_then(Loader::channels)
            .map(|channels| channels.image_shared())
        else {
            return;
        };
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let channels = Channels::new(image, scene);
        if let Some(loader) = self.source.as_ref() {
            loader.set_channels(channels);
        }
    }

    pub(super) fn add_mask_channel(&mut self, set: ObjectSetId) {
        if self.recipe.is_some() {
            self.message = Some(Message::error(
                "a protocol replay builds its own mask channels",
            ));
            return;
        }
        let name = {
            let Some(scene) = self.scene.as_mut() else {
                return;
            };
            let channel = if scene.selected < scene.base_channels() {
                scene.selected
            } else {
                scene
                    .objects
                    .get(set)
                    .map_or(scene.selected, |object| object.channel())
            };
            match scene.add_mask(set, channel) {
                Ok(channel) => scene.dataset.channels()[channel].name().to_string(),
                Err(detail) => {
                    self.message = Some(Message::error(detail));
                    return;
                }
            }
        };
        self.message = None;
        self.after_mask_layout();
        self.message = Some(Message::info(format!(
            "masked channel {name} counts the signal inside the objects"
        )));
    }

    pub(super) fn remove_mask_channel(&mut self, channel: usize) {
        if self.recipe.is_some() {
            self.message = Some(Message::error(
                "a protocol replay keeps the mask channels it staged",
            ));
            return;
        }
        let removed = {
            let Some(scene) = self.scene.as_mut() else {
                return;
            };
            match scene.remove_mask(channel) {
                Ok(()) => true,
                Err(detail) => {
                    self.message = Some(Message::error(detail));
                    false
                }
            }
        };
        if removed {
            self.message = None;
            self.after_mask_layout();
        }
    }

    pub(super) fn after_mask_layout(&mut self) {
        self.history.clear();
        self.in_flight.clear();
        self.failed.clear();
        self.bricks.clear();
        self.mask_stamp = None;
        self.install_channels();
        self.reload += 1;
    }

    pub(super) fn refresh_masks(&mut self) -> bool {
        let Some(scene) = self.scene.as_mut() else {
            self.mask_stamp = None;
            return false;
        };
        scene.sync_masks();
        let time = scene.time();
        let stamp = scene.mask_stamp(time);
        let changed = self
            .mask_stamp
            .is_some_and(|(held, value)| held == time && value != stamp);
        self.mask_stamp = Some((time, stamp));
        if changed {
            self.in_flight.clear();
            self.bricks.clear();
            self.reload += 1;
        }
        changed
    }
}
