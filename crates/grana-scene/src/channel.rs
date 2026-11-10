use grana_display::{TransferFunction, VolumeStyle};
use grana_volume::Intensity;

pub struct Channel {
    pub enabled: bool,
    pub opacity: f32,
    pub window: Intensity,
    transfer: TransferFunction,
    volume: VolumeStyle,
}

impl Channel {
    pub fn new(window: Intensity, transfer: TransferFunction) -> Self {
        Self {
            enabled: true,
            opacity: 1.0,
            window,
            transfer,
            volume: VolumeStyle::default(),
        }
    }

    pub fn transfer(&self) -> &TransferFunction {
        &self.transfer
    }

    pub fn transfer_mut(&mut self) -> &mut TransferFunction {
        &mut self.transfer
    }

    pub fn set_transfer(&mut self, transfer: TransferFunction) {
        self.transfer = transfer;
    }

    pub fn volume(&self) -> VolumeStyle {
        self.volume
    }

    pub fn set_volume(&mut self, volume: VolumeStyle) {
        self.volume = volume;
    }
}
