use std::path::{Path, PathBuf};

use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY, RegCloseKey, RegOpenKeyExW,
    RegQueryValueExW,
};

const KEY: &str = "SOFTWARE\\Grana";
const VALUE: &str = "Install";
const LENGTH: usize = 512;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Portable,
    Installed { root: PathBuf },
}

pub fn detect() -> Target {
    let Some(root) = install_root() else {
        return Target::Portable;
    };
    let Some(directory) = application_dir() else {
        return Target::Portable;
    };
    if same(&directory, &root) {
        Target::Installed { root }
    } else {
        Target::Portable
    }
}

fn application_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()?
        .parent()
        .map(Path::to_path_buf)
}

fn same(left: &Path, right: &Path) -> bool {
    match (std::fs::canonicalize(left), std::fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn install_root() -> Option<PathBuf> {
    let key = wide(KEY);
    let mut handle: HKEY = std::ptr::null_mut();
    let opened = unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            0,
            KEY_READ | KEY_WOW64_64KEY,
            &mut handle,
        )
    };
    if opened != ERROR_SUCCESS {
        return None;
    }
    let name = wide(VALUE);
    let mut buffer = [0u16; LENGTH];
    let mut bytes = std::mem::size_of_val(&buffer) as u32;
    let read = unsafe {
        RegQueryValueExW(
            handle,
            name.as_ptr(),
            std::ptr::null(),
            std::ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    unsafe { RegCloseKey(handle) };
    if read != ERROR_SUCCESS {
        return None;
    }
    let length = (bytes as usize / 2).saturating_sub(1).min(buffer.len());
    let text = String::from_utf16(&buffer[..length]).ok()?;
    let text = text.trim();
    (!text.is_empty()).then(|| PathBuf::from(text))
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}
