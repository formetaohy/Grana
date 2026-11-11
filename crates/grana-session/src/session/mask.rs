use grana_pipeline::Channels;
use grana_scene::{DerivedSpec, ObjectSetId};
use grana_volume::Intensity;

use crate::task::{Poll, Task};

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
        self.after_mask_layout();
        self.message = Some(Message::info(format!(
            "masked channel {name} counts the signal inside the objects"
        )));
    }

    pub(super) fn add_restore_channel(&mut self, source: usize, psf: Psf, iterations: u32) {
        if self.recipe.is_some() {
            self.message = Some(Message::error(
                "a protocol replay builds its own derived channels",
            ));
            return;
        }
        let Some(channels) = self.source.as_ref().and_then(Loader::channels) else {
            self.message = Some(Message::error("open a dataset before restoring it"));
            return;
        };
        let (fundus, name) = {
            let Some(scene) = self.scene.as_mut() else {
                return;
            };
            if source >= scene.base_channels() {
                self.message = Some(Message::error(
                    "a restored channel restores a stored channel",
                ));
                return;
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
        self.channels_pending.push(PendingChannel::AddRestore {
            channels,
            source,
            name,
            tint: fundus.tint(),
            psf,
            iterations,
            task,
        });
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
                self.after_mask_layout();
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
                self.after_mask_layout();
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
        self.channels_pending.push(PendingChannel::AddRatio {
            channels,
            numerator,
            denominator,
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
            Ok(true) => self.refresh_derived_layout(),
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
            Ok(true) => self.refresh_derived_layout(),
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
        self.channels_pending.push(PendingChannel::SetRatio {
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
            Ok(true) => self.refresh_derived_layout(),
            Ok(false) => {}
            Err(detail) => self.message = Some(Message::error(detail)),
        }
    }

    pub(super) fn refresh_derived_layout(&mut self) {
        self.in_flight.clear();
        self.failed.clear();
        self.bricks.clear();
        self.mask_stamp = None;
        self.install_channels();
        self.reloaded();
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
            self.after_mask_layout();
        }
    }

    pub(super) fn after_mask_layout(&mut self) {
        self.in_flight.clear();
        self.failed.clear();
        self.bricks.clear();
        self.mask_stamp = None;
        self.install_channels();
        self.reloaded();
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
            self.reloaded();
        }
        changed
    }

    pub(super) fn poll_channels(&mut self) -> bool {
        if self.channels_pending.is_empty() {
            return false;
        }
        let mut repaint = false;
        let mut index = 0;
        while index < self.channels_pending.len() {
            match self.channels_pending[index].task().poll() {
                Poll::Event(Ok(intensity)) => {
                    let pending = self.channels_pending.remove(index);
                    if self
                        .source
                        .as_ref()
                        .and_then(Loader::channels)
                        .is_some_and(|current| Arc::ptr_eq(&current, pending.channels()))
                    {
                        repaint |= self.install_channel(pending, intensity);
                    }
                }
                Poll::Event(Err(detail)) => {
                    self.channels_pending.remove(index);
                    self.message = Some(Message::error(detail));
                }
                Poll::Closed => {
                    self.channels_pending.remove(index);
                    self.message = Some(Message::error(
                        "a derived channel stopped before it was calibrated",
                    ));
                }
                Poll::Empty => index += 1,
            }
        }
        repaint
    }

    fn install_channel(&mut self, pending: PendingChannel, intensity: Intensity) -> bool {
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
                    return false;
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
                        self.after_mask_layout();
                        self.message = Some(Message::info(format!(
                            "restored channel {name} deconvolves {fundus}"
                        )));
                        true
                    }
                    Err(detail) => {
                        self.message = Some(Message::error(detail));
                        false
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
                    return false;
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
                    return false;
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
                        self.after_mask_layout();
                        self.message = Some(Message::info(format!(
                            "ratio channel {name} divides {up} by {down}"
                        )));
                        true
                    }
                    Err(detail) => {
                        self.message = Some(Message::error(detail));
                        false
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
                    return false;
                }
                let outcome = self
                    .scene
                    .as_mut()
                    .map(|scene| scene.set_ratio(channel, denominator, intensity));
                match outcome {
                    Some(Ok(true)) => {
                        self.refresh_derived_layout();
                        true
                    }
                    Some(Ok(false)) => false,
                    Some(Err(detail)) => {
                        self.message = Some(Message::error(detail));
                        false
                    }
                    None => false,
                }
            }
        }
    }
}

pub(super) enum PendingChannel {
    AddRestore {
        channels: Arc<Channels>,
        source: usize,
        name: String,
        tint: [f32; 3],
        psf: Psf,
        iterations: u32,
        task: Task<Result<Intensity, String>>,
    },
    AddRatio {
        channels: Arc<Channels>,
        numerator: usize,
        denominator: usize,
        name: String,
        tint: [f32; 3],
        task: Task<Result<Intensity, String>>,
    },
    SetRatio {
        channels: Arc<Channels>,
        channel: usize,
        numerator: usize,
        denominator: usize,
        task: Task<Result<Intensity, String>>,
    },
}

impl PendingChannel {
    fn task(&self) -> &Task<Result<Intensity, String>> {
        match self {
            Self::AddRestore { task, .. } => task,
            Self::AddRatio { task, .. } => task,
            Self::SetRatio { task, .. } => task,
        }
    }

    fn channels(&self) -> &Arc<Channels> {
        match self {
            Self::AddRestore { channels, .. } => channels,
            Self::AddRatio { channels, .. } => channels,
            Self::SetRatio { channels, .. } => channels,
        }
    }
}

fn calibrate(
    channels: Arc<Channels>,
    work: impl FnOnce(&Channels) -> Result<Intensity, String> + Send + 'static,
) -> Task<Result<Intensity, String>> {
    Task::spawn(grana_exec::Role::Compute, move |cancel, sink| {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        sink(work(&channels));
    })
}
