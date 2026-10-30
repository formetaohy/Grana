use grana_render::{ChannelParams, RenderParams, Shading};

use crate::dataset::Dataset;
use crate::transfer_editor::TransferEditor;

pub struct ChannelView {
    pub enabled: bool,
    pub opacity: f32,
    transfer: TransferEditor,
}

pub struct Scene {
    pub dataset: Dataset,
    pub channels: Vec<ChannelView>,
    pub selected: usize,
    pub shading: Shading,
    pub step_scale: f32,
}

impl Scene {
    pub fn new(dataset: Dataset) -> Self {
        let channels = dataset
            .channels
            .iter()
            .map(|channel| ChannelView {
                enabled: true,
                opacity: 1.0,
                transfer: TransferEditor::new(channel.tint),
            })
            .collect();
        Self {
            dataset,
            channels,
            selected: 0,
            shading: Shading::default(),
            step_scale: 1.0,
        }
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
                    transfer: channel.transfer.transfer(),
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
}
