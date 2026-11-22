use wgpu::{AdapterInfo, DeviceType};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Memory {
    local: u64,
    shared: u64,
    integrated: bool,
}

impl Memory {
    pub fn local(self) -> u64 {
        self.local
    }

    pub fn shared(self) -> u64 {
        self.shared
    }

    pub fn integrated(self) -> bool {
        self.integrated
    }

    pub(crate) fn of(info: &AdapterInfo) -> Option<Self> {
        let (local, shared) = probe(info)?;
        Some(Self {
            local,
            shared,
            integrated: matches!(
                info.device_type,
                DeviceType::IntegratedGpu | DeviceType::Cpu
            ),
        })
    }
}

#[cfg(windows)]
fn probe(info: &AdapterInfo) -> Option<(u64, u64)> {
    use windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, DXGI_ERROR_NOT_FOUND, IDXGIFactory1,
    };
    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }.ok()?;
    let mut chosen: Option<(u64, u64)> = None;
    let mut index = 0;
    loop {
        let adapter = match unsafe { factory.EnumAdapters1(index) } {
            Ok(adapter) => adapter,
            Err(error) if error.code() == DXGI_ERROR_NOT_FOUND => break,
            Err(_) => return None,
        };
        index += 1;
        let Ok(description) = (unsafe { adapter.GetDesc1() }) else {
            continue;
        };
        if description.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0
            || description.VendorId != info.vendor
            || description.DeviceId != info.device
        {
            continue;
        }
        let local = description.DedicatedVideoMemory as u64;
        let shared = description.SharedSystemMemory as u64;
        if chosen.is_none_or(|held| local > held.0) {
            chosen = Some((local, shared));
        }
    }
    chosen
}

#[cfg(not(windows))]
fn probe(_: &AdapterInfo) -> Option<(u64, u64)> {
    None
}
