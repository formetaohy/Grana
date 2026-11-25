use std::sync::OnceLock;

use burn::tensor::Device;
use burn::tensor::wgpu::WgpuSetup;

use crate::Result;
use crate::error::Error;

static DEVICE: OnceLock<Device> = OnceLock::new();

pub(crate) fn open() -> Result<Device> {
    if let Some(device) = DEVICE.get() {
        return Ok(device.clone());
    }
    let device = acquire()?;
    Ok(DEVICE.get_or_init(|| device).clone())
}

fn acquire() -> Result<Device> {
    let graphics = grana_gpu::Headless::shared()
        .map_err(|detail| Error::device(format!("the graphics device cannot start: {detail}")))?;
    let graphics = graphics.graphics();
    let setup = WgpuSetup {
        instance: graphics.instance().clone(),
        adapter: graphics.adapter().clone(),
        device: graphics.device().clone(),
        queue: graphics.queue().clone(),
        backend: graphics.backend(),
    };
    Device::wgpu_options()
        .setup(setup)
        .init()
        .map_err(|error| Error::device(format!("the graphics device cannot start: {error}")))
}
