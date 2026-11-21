use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use windows_sys::Win32::Foundation::{CloseHandle, GetLastError};
use windows_sys::Win32::System::Threading::{
    GetExitCodeProcess, INFINITE, OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
};
use windows_sys::Win32::UI::Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};
use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

use crate::Error;

const WAIT: &str = "--wait";
const LAUNCH: &str = "--launch";
const VERB: &str = "runas";
const PARAMETERS: &str = "-quiet -norestart";

pub fn assist(arguments: &[String]) -> Result<(), Error> {
    let (setup, pid, launch) = parse(arguments)?;
    wait_for(pid);
    match elevate(&setup) {
        Ok(code) => grana_log::info!(target: "grana::update", code, "the installer finished"),
        Err(error) => {
            grana_log::error!(target: "grana::update", %error, "the installer did not finish");
        }
    }
    Command::new(&launch)
        .spawn()
        .map_err(|error| Error::storage(&launch, error))?;
    Ok(())
}

fn parse(arguments: &[String]) -> Result<(PathBuf, u32, PathBuf), Error> {
    let [setup, wait, pid, launch, executable] = arguments else {
        return Err(Error::Installer {
            detail: "the updater agent takes a setup, a wait and a launch".to_string(),
        });
    };
    if wait != WAIT || launch != LAUNCH {
        return Err(Error::Installer {
            detail: "the updater agent takes a setup, a wait and a launch".to_string(),
        });
    }
    let pid = pid.parse().map_err(|_| Error::Installer {
        detail: format!("`{pid}` is not a process"),
    })?;
    Ok((PathBuf::from(setup), pid, PathBuf::from(executable)))
}

fn wait_for(process: u32) {
    let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, process) };
    if handle.is_null() {
        return;
    }
    unsafe {
        WaitForSingleObject(handle, INFINITE);
        CloseHandle(handle);
    }
}

fn elevate(setup: &Path) -> Result<u32, Error> {
    let file = wide(setup.as_os_str());
    let verb = wide(OsStr::new(VERB));
    let parameters = wide(OsStr::new(PARAMETERS));
    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOCLOSEPROCESS;
    info.lpVerb = verb.as_ptr();
    info.lpFile = file.as_ptr();
    info.lpParameters = parameters.as_ptr();
    info.nShow = SW_HIDE;
    let started = unsafe { ShellExecuteExW(&mut info) };
    if started == 0 {
        let error = unsafe { GetLastError() };
        return Err(Error::Installer {
            detail: format!("the installer did not start (windows error {error})"),
        });
    }
    let mut code = 0;
    unsafe {
        WaitForSingleObject(info.hProcess, INFINITE);
        GetExitCodeProcess(info.hProcess, &mut code);
        CloseHandle(info.hProcess);
    }
    Ok(code)
}

fn wide(text: &OsStr) -> Vec<u16> {
    text.encode_wide().chain(std::iter::once(0)).collect()
}
