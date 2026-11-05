use std::ops::Deref;
use std::sync::{Mutex, MutexGuard, OnceLock};

use grana_gpu::{HEADLESS_FORMAT, Headless};
use wgpu::{Device, Queue, TextureFormat};

static SHARED: OnceLock<Option<(Device, Queue)>> = OnceLock::new();
static SERIAL: Mutex<()> = Mutex::new(());

pub(crate) struct TestGpu {
    device: Device,
    _serial: MutexGuard<'static, ()>,
}

impl Deref for TestGpu {
    type Target = Device;

    fn deref(&self) -> &Device {
        &self.device
    }
}

pub(crate) fn gpu() -> (TestGpu, Queue) {
    let serial = SERIAL.lock().unwrap_or_else(|error| error.into_inner());
    let (device, queue) = SHARED
        .get_or_init(|| {
            Headless::new()
                .ok()
                .map(|headless| (headless.device, headless.queue))
        })
        .clone()
        .expect("the headless device is available");
    (
        TestGpu {
            device,
            _serial: serial,
        },
        queue,
    )
}

pub(crate) fn format() -> TextureFormat {
    HEADLESS_FORMAT
}
