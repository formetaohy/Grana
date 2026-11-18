use grana_pipeline::Channels;
use grana_scene::{DerivedSpec, ObjectSetId};
use grana_volume::{ChannelSet, Damage, Intensity};

use super::derived::{PendingChannel, calibrate};

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
        let render = Channels::render(Arc::clone(&image), scene);
        self.channels = Some(Arc::new(Channels::analysis(image, scene)));
        if let Some(loader) = self.source.as_ref() {
            loader.set_channels(render);
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
        self.relayout_channels();
        self.message = Some(Message::info(format!(
            "masked channel {name} counts the signal inside the objects"
        )));
    }

    pub(super) fn add_restore_channel(&mut self, source: usize, psf: Psf, iterations: u32) -> bool {
        if self.recipe.is_some() {
            self.message = Some(Message::error(
                "a protocol replay builds its own derived channels",
            ));
            return false;
        }
        let Some(channels) = self.source.as_ref().and_then(Loader::channels) else {
            self.message = Some(Message::error("open a dataset before restoring it"));
            return false;
        };
        let (fundus, name) = {
            let Some(scene) = self.scene.as_mut() else {
                return false;
            };
            if source >= scene.base_channels() {
                self.message = Some(Message::error(
                    "a restored channel restores a stored channel",
                ));
                return false;
            }
            let fundus = scene.dataset.channels()[source].clone();
            let name = scene.unique_channel_name(&format!("{} restored", fundus.name()));
            (fundus, name)
        };
        let restoring = psf.clone();
        let task = calibrate(Arc::clone(&channels), move |channels| {
            channels
                .restored_range(source, restoring, iterations)
                .map_err(|error| error.to_string())
        });
        self.message = Some(Message::info(format!(
            "calibrating restored channel {name}"
        )));
        self.tasks.start_channel(PendingChannel::AddRestore {
            channels,
            source,
            name,
            tint: fundus.tint(),
            psf,
            iterations,
            task,
        });
        true
    }

    pub(super) fn add_smooth_channel(&mut self, source: usize, sigma: f32) {
        if self.recipe.is_some() {
            self.message = Some(Message::error(
                "a protocol replay builds its own derived channels",
            ));
            return;
        }
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let Some(info) = scene.dataset.channels().get(source) else {
            self.message = Some(Message::error("the smoothed channel is missing"));
            return;
        };
        if info.intensity().span() <= 0.0 {
            self.message = Some(Message::error(
                "the channel has no intensity range to smooth",
            ));
            return;
        }
        let name = scene.unique_channel_name(&format!("{} smoothed", info.name()));
        let tint = info.tint();
        let intensity = info.intensity();
        let source_name = info.name().to_string();
        let Some(scene) = self.scene.as_mut() else {
            return;
        };
        match scene.add_derived(DerivedSpec::smoothed(source, name, tint, intensity, sigma)) {
            Ok(channel) => {
                let name = scene.dataset.channels()[channel].name().to_string();
                self.message = None;
                self.relayout_channels();
                self.message = Some(Message::info(format!(
                    "smoothed channel {name} smooths {source_name}"
                )));
            }
            Err(detail) => self.message = Some(Message::error(detail)),
        }
    }

    pub(super) fn add_background_channel(&mut self, source: usize, radius: f32) {
        if self.recipe.is_some() {
            self.message = Some(Message::error(
                "a protocol replay builds its own derived channels",
            ));
            return;
        }
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let Some(info) = scene.dataset.channels().get(source) else {
            self.message = Some(Message::error("the background channel is missing"));
            return;
        };
        let name = scene.unique_channel_name(&format!("{} background removed", info.name()));
        let tint = info.tint();
        let source_name = info.name().to_string();
        let max = info.intensity().max().max(0.0);
        let intensity = Intensity::new(0.0, if max > 0.0 { max } else { 1.0 });
        let Some(scene) = self.scene.as_mut() else {
            return;
        };
        match scene.add_derived(DerivedSpec::background_subtracted(
            source, name, tint, intensity, radius,
        )) {
            Ok(channel) => {
                let name = scene.dataset.channels()[channel].name().to_string();
                self.message = None;
                self.relayout_channels();
                self.message = Some(Message::info(format!(
                    "background channel {name} removes the background of {source_name}"
                )));
            }
            Err(detail) => self.message = Some(Message::error(detail)),
        }
    }

    pub(super) fn add_ratio_channel(&mut self, numerator: usize, denominator: usize) {
        if self.recipe.is_some() {
            self.message = Some(Message::error(
                "a protocol replay builds its own derived channels",
            ));
            return;
        }
        if numerator == denominator {
            self.message = Some(Message::error(
                "a ratio channel divides two distinct channels",
            ));
            return;
        }
        let Some(channels) = self.source.as_ref().and_then(Loader::channels) else {
            self.message = Some(Message::error(
                "open a dataset before deriving a ratio channel",
            ));
            return;
        };
        let (up, down, name) = {
            let Some(scene) = self.scene.as_mut() else {
                return;
            };
            let (Some(up), Some(down)) = (
                scene.dataset.channels().get(numerator).cloned(),
                scene.dataset.channels().get(denominator).cloned(),
            ) else {
                self.message = Some(Message::error("the ratio channels are missing"));
                return;
            };
            let name = scene.unique_channel_name(&format!("{} / {}", up.name(), down.name()));
            (up, down, name)
        };
        let tint = [0, 1, 2]
            .map(|axis| 0.5 * (up.tint()[axis] + down.tint()[axis]))
            .map(|value| value.clamp(0.0, 1.0));
        let task = calibrate(Arc::clone(&channels), move |channels| {
            channels
                .ratio_range(numerator, denominator)
                .map_err(|error| error.to_string())
        });
        self.message = Some(Message::info(format!("calibrating ratio channel {name}")));
        self.tasks.start_channel(PendingChannel::AddRatio {
            channels,
            numerator,
            denominator,
            name,
            tint,
            task,
        });
    }

    pub(super) fn add_bleach_channel(&mut self, source: usize) {
        if self.recipe.is_some() {
            self.message = Some(Message::error(
                "a protocol replay builds its own derived channels",
            ));
            return;
        }
        let Some(channels) = self.source.as_ref().and_then(Loader::channels) else {
            self.message = Some(Message::error(
                "open a dataset before correcting its bleaching",
            ));
            return;
        };
        if self
            .scene
            .as_ref()
            .is_some_and(|scene| source >= scene.base_channels())
        {
            self.message = Some(Message::error(
                "a bleach correction normalizes a stored channel",
            ));
            return;
        }
        let prepared = (|| {
            let scene = self.scene.as_ref()?;
            let info = scene.dataset.channels().get(source)?;
            if info.intensity().span() <= 0.0 {
                return None;
            }
            let name = scene.unique_channel_name(&format!("{} bleached", info.name()));
            Some((
                name,
                info.tint(),
                info.name().to_string(),
                scene.drifts().clone(),
            ))
        })();
        let Some((name, tint, source_name, drifts)) = prepared else {
            self.message = Some(Message::error(
                "the channel has no intensity range to normalize",
            ));
            return;
        };
        let task = calibrate(Arc::clone(&channels), move |channels| {
            channels
                .bleach_range(source, &drifts)
                .map_err(|error| error.to_string())
        });
        self.message = Some(Message::info(format!(
            "normalizing the bleaching of {source_name}"
        )));
        self.tasks.start_channel(PendingChannel::AddBleach {
            channels,
            source,
            name,
            tint,
            task,
        });
    }

    pub(super) fn set_smooth_channel(&mut self, channel: usize, sigma: f32) {
        if self.recipe.is_some() {
            self.message = Some(Message::error(
                "a protocol replay builds its own derived channels",
            ));
            return;
        }
        let outcome = match self.scene.as_mut() {
            Some(scene) => scene.set_smooth(channel, sigma),
            None => return,
        };
        match outcome {
            Ok(true) => self.relayout_channels(),
            Ok(false) => {}
            Err(detail) => self.message = Some(Message::error(detail)),
        }
    }

    pub(super) fn set_background_channel(&mut self, channel: usize, radius: f32) {
        if self.recipe.is_some() {
            self.message = Some(Message::error(
                "a protocol replay builds its own derived channels",
            ));
            return;
        }
        let outcome = match self.scene.as_mut() {
            Some(scene) => scene.set_background(channel, radius),
            None => return,
        };
        match outcome {
            Ok(true) => self.relayout_channels(),
            Ok(false) => {}
            Err(detail) => self.message = Some(Message::error(detail)),
        }
    }

    pub(super) fn set_ratio_channel(&mut self, channel: usize, denominator: usize) {
        if self.recipe.is_some() {
            self.message = Some(Message::error(
                "a protocol replay builds its own derived channels",
            ));
            return;
        }
        let Some(numerator) = self.scene.as_ref().and_then(|scene| {
            scene
                .derived()
                .get(channel.checked_sub(scene.base_channels())?)
                .map(|entry| entry.spec().source())
        }) else {
            self.message = Some(Message::error("the ratio channel is missing"));
            return;
        };
        let Some(channels) = self.source.as_ref().and_then(Loader::channels) else {
            self.message = Some(Message::error(
                "open a dataset before deriving a ratio channel",
            ));
            return;
        };
        let task = calibrate(Arc::clone(&channels), move |channels| {
            channels
                .ratio_range(numerator, denominator)
                .map_err(|error| error.to_string())
        });
        self.tasks.start_channel(PendingChannel::SetRatio {
            channels,
            channel,
            numerator,
            denominator,
            task,
        });
    }

    pub(super) fn set_restore_channel(&mut self, channel: usize, psf: Psf, iterations: u32) {
        if self.recipe.is_some() {
            self.message = Some(Message::error(
                "a protocol replay builds its own derived channels",
            ));
            return;
        }
        let outcome = match self.scene.as_mut() {
            Some(scene) => scene.set_restore(channel, psf, iterations),
            None => return,
        };
        match outcome {
            Ok(true) => self.relayout_channels(),
            Ok(false) => {}
            Err(detail) => self.message = Some(Message::error(detail)),
        }
    }

    pub(super) fn relayout_channels(&mut self) {
        self.mask_stamps.clear();
        self.mask_extents.clear();
        self.install_channels();
        self.reload_streamed();
    }

    pub(super) fn remove_derived_channel(&mut self, channel: usize) {
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
            match scene.remove_derived(channel) {
                Ok(()) => true,
                Err(detail) => {
                    self.message = Some(Message::error(detail));
                    false
                }
            }
        };
        if removed {
            self.message = None;
            self.relayout_channels();
        }
    }

    pub(super) fn refresh_masks(&mut self) -> bool {
        let Some(scene) = self.scene.as_mut() else {
            self.mask_stamps.clear();
            self.mask_extents.clear();
            return false;
        };
        scene.sync_masks();
        let stamps = scene.mask_stamps();
        if stamps.len() != self.mask_stamps.len() {
            self.mask_stamps = stamps;
            self.mask_extents = scene.mask_extents(scene.time());
            return false;
        }
        let mut changed = ChannelSet::NONE;
        for (index, (held, stamp)) in self.mask_stamps.iter().zip(&stamps).enumerate() {
            if held != stamp {
                changed = changed.with(scene.base_channels() + index);
            }
        }
        if changed.is_empty() {
            return false;
        }
        self.mask_stamps = stamps;
        let time = scene.time();
        let extents = scene.mask_extents(time);
        let channels = scene.damaged_channels(changed);
        let damage = scene
            .mask_damage(channels, &extents, &self.mask_extents)
            .and_then(|region| Damage::new(region, channels));
        self.mask_extents = extents;
        match damage {
            Some(damage) => {
                self.damages.push(damage);
                self.discard_streamed();
            }
            None => self.reload_streamed(),
        }
        true
    }

    pub(super) fn finish_channel(
        &mut self,
        pending: PendingChannel,
        outcome: Result<Intensity, String>,
    ) {
        match outcome {
            Ok(intensity) => {
                let current = self.source.as_ref().and_then(Loader::channels);
                match current {
                    Some(current) if current.layout() == pending.channels().layout() => {
                        self.accept_channel(pending, intensity);
                    }
                    _ => {
                        self.message = Some(Message::info(
                            "the derived channel finished after the channels it measured changed, so its range was discarded",
                        ));
                    }
                }
            }
            Err(detail) => self.message = Some(Message::error(detail)),
        }
    }

    fn accept_channel(&mut self, pending: PendingChannel, intensity: Intensity) {
        let Some(recording) = self.begin_change(Edit::new(Action::Derived)) else {
            return;
        };
        self.install_channel(pending, intensity);
        self.commit_edit(recording);
    }

    fn install_channel(&mut self, pending: PendingChannel, intensity: Intensity) {
        match pending {
            PendingChannel::AddRestore {
                source,
                name,
                tint,
                psf,
                iterations,
                ..
            } => {
                let Some(scene) = self.scene.as_mut() else {
                    return;
                };
                let fundus = scene
                    .dataset
                    .channels()
                    .get(source)
                    .map_or_else(String::new, |channel| channel.name().to_string());
                match scene.add_derived(DerivedSpec::restored(
                    source,
                    name.clone(),
                    tint,
                    intensity,
                    psf,
                    iterations,
                )) {
                    Ok(_) => {
                        self.message = None;
                        self.relayout_channels();
                        self.message = Some(Message::info(format!(
                            "restored channel {name} deconvolves {fundus}"
                        )));
                    }
                    Err(detail) => {
                        self.message = Some(Message::error(detail));
                    }
                }
            }
            PendingChannel::AddRatio {
                numerator,
                denominator,
                name,
                tint,
                ..
            } => {
                let Some(scene) = self.scene.as_mut() else {
                    return;
                };
                let names = (
                    scene
                        .dataset
                        .channels()
                        .get(numerator)
                        .map(|channel| channel.name().to_string()),
                    scene
                        .dataset
                        .channels()
                        .get(denominator)
                        .map(|channel| channel.name().to_string()),
                );
                let (Some(up), Some(down)) = names else {
                    self.message = Some(Message::error("the ratio channels are missing"));
                    return;
                };
                match scene.add_derived(DerivedSpec::ratio(
                    numerator,
                    denominator,
                    name.clone(),
                    tint,
                    intensity,
                )) {
                    Ok(_) => {
                        self.message = None;
                        self.relayout_channels();
                        self.message = Some(Message::info(format!(
                            "ratio channel {name} divides {up} by {down}"
                        )));
                    }
                    Err(detail) => {
                        self.message = Some(Message::error(detail));
                    }
                }
            }
            PendingChannel::AddBleach {
                source, name, tint, ..
            } => {
                let Some(scene) = self.scene.as_mut() else {
                    return;
                };
                let fundus = scene
                    .dataset
                    .channels()
                    .get(source)
                    .map_or_else(String::new, |channel| channel.name().to_string());
                match scene.add_derived(DerivedSpec::bleached(
                    source,
                    name.clone(),
                    tint,
                    intensity,
                )) {
                    Ok(_) => {
                        self.message = None;
                        self.relayout_channels();
                        self.message = Some(Message::info(format!(
                            "bleach channel {name} normalizes {fundus}"
                        )));
                    }
                    Err(detail) => {
                        self.message = Some(Message::error(detail));
                    }
                }
            }
            PendingChannel::SetRatio {
                channel,
                numerator,
                denominator,
                ..
            } => {
                let source = self.scene.as_ref().and_then(|scene| {
                    channel
                        .checked_sub(scene.base_channels())
                        .and_then(|index| scene.derived().get(index))
                        .map(|entry| entry.spec().source())
                });
                if source != Some(numerator) {
                    self.message = Some(Message::error(
                        "the ratio channel changed while it was calibrating",
                    ));
                    return;
                }
                let outcome = self
                    .scene
                    .as_mut()
                    .map(|scene| scene.set_ratio(channel, denominator, intensity));
                match outcome {
                    Some(Ok(true)) => self.relayout_channels(),
                    Some(Ok(false)) => {}
                    Some(Err(detail)) => self.message = Some(Message::error(detail)),
                    None => {}
                }
            }
        }
    }
}
