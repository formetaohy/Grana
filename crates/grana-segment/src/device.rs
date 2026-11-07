use std::sync::OnceLock;

use burn::tensor::{Device, DeviceKind};

use crate::Result;
use crate::error::Error;

static DEVICE: OnceLock<Device> = OnceLock::new();

const SOFTWARE_ADAPTER: &str =
    "the model needs a graphics device, but only a software adapter is available";

pub(crate) fn open() -> Result<Device> {
    if let Some(device) = DEVICE.get() {
        return Ok(device.clone());
    }
    match acquire() {
        Ok(device) => Ok(DEVICE.get_or_init(|| device).clone()),
        Err(error) => DEVICE.get().cloned().ok_or(error),
    }
}

fn acquire() -> Result<Device> {
    let device = Device::wgpu_options()
        .device_kind(DeviceKind::DefaultDevice)
        .init()
        .map_err(|error| Error::device(format!("the graphics device cannot start: {error}")))?;
    match device.identity() {
        Some(identity) if identity.physical.is_some() => Ok(device),
        Some(identity) => Err(Error::device(format!(
            "`{}` is not a graphics device",
            identity.name
        ))),
        None => Err(Error::device(SOFTWARE_ADAPTER)),
    }
}
