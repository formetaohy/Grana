use grana_analysis::{SpotParams, SpotSet};
use grana_render::{ChannelDisplay, ChannelParams, Marker, RenderParams, Shading};
use grana_volume::{ChannelInfo, Intensity};

use crate::dataset::Dataset;
use crate::transfer_editor::TransferEditor;

pub struct ChannelView {
    pub enabled: bool,
    pub opacity: f32,
    pub window: Intensity,
    pub default_window: Intensity,
    pub display: ChannelDisplay,
    transfer: TransferEditor,
}

struct SpotLayer {
    visible: bool,
    set: SpotSet,
}

pub struct Scene {
    pub dataset: Dataset,
    pub channels: Vec<ChannelView>,
    pub selected: usize,
    pub shading: Shading,
    pub step_scale: f32,
    pub spot_params: SpotParams,
    spots: Option<SpotLayer>,
}

impl Scene {
    pub fn new(dataset: Dataset) -> Self {
        let channels = dataset
            .channels()
            .iter()
            .map(|channel| ChannelView {
                enabled: true,
                opacity: 1.0,
                window: channel.window(),
                default_window: channel.window(),
                display: ChannelDisplay::Volume,
                transfer: TransferEditor::new(channel.tint()),
            })
            .collect();
        let spot_params = SpotParams::new(4.0 * dataset.voxel_size().min_element(), 0.2);
        Self {
            dataset,
            channels,
            selected: 0,
            shading: Shading::default(),
            step_scale: 1.0,
            spot_params,
            spots: None,
        }
    }

    pub fn channel(&self, index: usize) -> &ChannelInfo {
        &self.dataset.channels()[index]
    }

    pub fn params(&self) -> RenderParams {
        RenderParams {
            shading: self.shading,
            step_scale: self.step_scale,
            channels: self
                .channels
                .iter()
                .map(|channel| ChannelParams {
                    visible: channel.enabled,
                    opacity: channel.opacity,
                    window: channel.window,
                    transfer: channel.transfer.transfer(),
                    display: channel.display,
                })
                .collect(),
        }
    }

    pub fn selected_transfer(&mut self) -> &mut TransferEditor {
        &mut self.channels[self.selected].transfer
    }

    pub fn select(&mut self, index: usize) {
        assert!(index < self.channels.len(), "channel index out of bounds");
        self.selected = index;
    }

    pub fn set_display(&mut self, index: usize, display: ChannelDisplay) {
        assert!(index < self.channels.len(), "channel index out of bounds");
        self.channels[index].display = display;
    }

    pub fn set_window(&mut self, index: usize, start: f32, end: f32) {
        let data = self.dataset.channels()[index].intensity();
        let low = start.min(end).clamp(data.min(), data.max());
        let high = start.max(end).clamp(data.min(), data.max());
        let minimum = (data.span() * 1.0e-3).max(f32::EPSILON);
        let high = if high - low < minimum {
            (low + minimum).min(data.max())
        } else {
            high
        };
        if high - low <= 0.0 {
            return;
        }
        self.channels[index].window = Intensity::new(low, high);
    }

    pub fn reset_window(&mut self, index: usize) {
        self.channels[index].window = self.channels[index].default_window;
    }

    pub fn spots(&self) -> Option<&SpotSet> {
        self.spots.as_ref().map(|layer| &layer.set)
    }

    pub fn spot_visible(&self) -> bool {
        self.spots.as_ref().is_some_and(|layer| layer.visible)
    }

    pub fn set_spot_visible(&mut self, visible: bool) {
        if let Some(layer) = &mut self.spots {
            layer.visible = visible;
        }
    }

    pub fn install_spots(&mut self, set: SpotSet) {
        assert!(
            set.channel() < self.channels.len(),
            "the spot set belongs to an existing channel"
        );
        self.spots = Some(SpotLayer { visible: true, set });
    }

    pub fn clear_spots(&mut self) {
        self.spots = None;
    }

    pub fn markers(&self, out: &mut Vec<Marker>) {
        out.clear();
        let Some(layer) = &self.spots else {
            return;
        };
        if !layer.visible {
            return;
        }
        let channel = layer.set.channel();
        if !self.channels[channel].enabled {
            return;
        }
        let color = self.dataset.channels()[channel].tint();
        for spot in layer.set.spots() {
            out.push(Marker::new(spot.center(), spot.radius(), color, 1.0));
        }
    }
}
