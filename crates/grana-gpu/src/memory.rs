use wgpu::{AdapterInfo, DeviceType};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Memory {
    vendor: u32,
    device: u32,
    local: u64,
    shared: u64,
    integrated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pool {
    local: u64,
    non_local: u64,
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

    pub fn pool(self) -> Pool {
        budget(self.vendor, self.device).unwrap_or(Pool {
            local: self.local,
            non_local: self.shared,
        })
    }

    pub(crate) fn of(info: &AdapterInfo) -> Option<Self> {
        let (local, shared) = inventory(info.vendor, info.device)?;
        Some(Self {
            vendor: info.vendor,
            device: info.device,
            local,
            shared,
            integrated: matches!(
                info.device_type,
                DeviceType::IntegratedGpu | DeviceType::Cpu
            ),
        })
    }
}

impl Pool {
    pub fn local(self) -> u64 {
        self.local
    }

    pub fn non_local(self) -> u64 {
        self.non_local
    }
}

#[cfg(windows)]
fn adapters(
    vendor: u32,
    device: u32,
) -> Option<Vec<windows::Win32::Graphics::Dxgi::IDXGIAdapter1>> {
    use windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, DXGI_ERROR_NOT_FOUND, IDXGIFactory1,
    };
    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }.ok()?;
    let mut chosen = Vec::new();
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
            || description.VendorId != vendor
            || description.DeviceId != device
        {
            continue;
        }
        chosen.push(adapter);
    }
    Some(chosen)
}

#[cfg(windows)]
fn inventory(vendor: u32, device: u32) -> Option<(u64, u64)> {
    let mut chosen: Option<(u64, u64)> = None;
    for adapter in &adapters(vendor, device)? {
        let Ok(description) = (unsafe { adapter.GetDesc1() }) else {
            continue;
        };
        let pair = (
            description.DedicatedVideoMemory as u64,
            description.SharedSystemMemory as u64,
        );
        if chosen.is_none_or(|held| pair.0 > held.0) {
            chosen = Some(pair);
        }
    }
    chosen
}

#[cfg(windows)]
fn budget(vendor: u32, device: u32) -> Option<Pool> {
    use windows::Win32::Graphics::Dxgi::{
        DXGI_MEMORY_SEGMENT_GROUP_LOCAL, DXGI_MEMORY_SEGMENT_GROUP_NON_LOCAL,
        DXGI_QUERY_VIDEO_MEMORY_INFO, IDXGIAdapter3,
    };
    use windows::core::Interface;
    let mut chosen: Option<Pool> = None;
    for adapter in &adapters(vendor, device)? {
        let Ok(adapter) = adapter.cast::<IDXGIAdapter3>() else {
            continue;
        };
        let mut local = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
        if unsafe { adapter.QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut local) }
            .is_err()
        {
            continue;
        }
        let mut non_local = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
        let shared = unsafe {
            adapter.QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_NON_LOCAL, &mut non_local)
        }
        .is_ok();
        let pool = Pool {
            local: local.Budget,
            non_local: if shared { non_local.Budget } else { 0 },
        };
        if pool.local == 0 {
            continue;
        }
        if chosen.is_none_or(|held| pool.local > held.local) {
            chosen = Some(pool);
        }
    }
    chosen
}

#[cfg(not(windows))]
fn inventory(_: u32, _: u32) -> Option<(u64, u64)> {
    None
}

#[cfg(not(windows))]
fn budget(_: u32, _: u32) -> Option<Pool> {
    None
}
