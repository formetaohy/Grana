use grana_render::TransferFunction;
use grana_volume::Intensity;

pub struct Channel {
    pub enabled: bool,
    pub opacity: f32,
    pub window: Intensity,
    pub default_window: Intensity,
    transfer: TransferFunction,
}

impl Channel {
    pub fn new(window: Intensity, default_window: Intensity, transfer: TransferFunction) -> Self {
        Self {
            enabled: true,
            opacity: 1.0,
            window,
            default_window,
            transfer,
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
}
