use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use crate::Error;
use crate::target::Target;

const AGENT: &str = "grana.exe";
const APPLY: &str = "--apply-update";
const WAIT: &str = "--wait";
const LAUNCH: &str = "--launch";
const OPEN: &str = "open";
const SHELL_LIMIT: isize = 32;

pub fn relaunch(executable: &Path) -> Result<(), Error> {
    self_replace::self_replace(executable).map_err(|error| Error::storage(executable, error))?;
    let current = std::env::current_exe().map_err(|error| Error::storage("grana", error))?;
    Command::new(&current)
        .args(std::env::args_os().skip(1))
        .spawn()
        .map_err(|error| Error::storage(&current, error))?;
    Ok(())
}

pub fn hand_over(setup: &Path, target: &Target) -> Result<(), Error> {
    let Target::Installed { root } = target else {
        return Err(Error::Installer {
            detail: "the application is not installed".to_string(),
        });
    };
    let agent = agent()?;
    let current = std::env::current_exe().map_err(|error| Error::storage("grana", error))?;
    std::fs::copy(&current, &agent).map_err(|error| Error::storage(&agent, error))?;
    Command::new(&agent)
        .arg(APPLY)
        .arg(setup)
        .arg(WAIT)
        .arg(std::process::id().to_string())
        .arg(LAUNCH)
        .arg(root.join(AGENT))
        .spawn()
        .map_err(|error| Error::storage(&agent, error))?;
    Ok(())
}

fn agent() -> Result<PathBuf, Error> {
    let root = grana_dir::cache().ok_or_else(|| Error::Storage {
        path: "the cache".to_string(),
        detail: "the system assigns no cache directory".to_string(),
    })?;
    let directory = root.join("update").join("agent");
    std::fs::create_dir_all(&directory).map_err(|error| Error::storage(&directory, error))?;
    Ok(directory.join(AGENT))
}

pub fn open(url: &str) -> Result<(), Error> {
    let operation = wide(OsStr::new(OPEN));
    let target = wide(OsStr::new(url));
    let outcome = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            target.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    if (outcome as isize) <= SHELL_LIMIT {
        return Err(Error::Browser {
            detail: format!("`{url}` (windows code {})", outcome as isize),
        });
    }
    Ok(())
}

fn wide(text: &OsStr) -> Vec<u16> {
    text.encode_wide().chain(std::iter::once(0)).collect()
}
