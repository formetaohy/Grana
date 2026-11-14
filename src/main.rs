#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: grana [<dataset>] | grana --view <project|dataset> | grana --capture <image.png> [<dataset>] | grana --capture <image.png> --view <project|dataset> | grana --render <project|dataset> <image.png|image.ome.tif|image.svg|image.pdf|movie.mp4|movie.mov|folder> [--width <n>] [--height <n>] [--time <n>] [--view <name>] [--lineage <set>] [--transparent] | grana --batch <protocol.json> <datasets> <output> [--jobs <n>] [--project] [--channels] | grana --stitch <folder> <output.zarr> <rows>x<cols> [--channel <n>] [--series <n>] | grana --clear-cache";

fn main() -> ExitCode {
    console();
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    match run(&arguments) {
        Some(outcome) => outcome,
        None => usage(),
    }
}

#[cfg(target_os = "windows")]
fn console() {
    use std::ffi::c_void;

    const ATTACH_PARENT_PROCESS: u32 = 0xFFFF_FFFF;
    const STD_OUTPUT_HANDLE: u32 = 0xFFFF_FFF5;
    const STD_ERROR_HANDLE: u32 = 0xFFFF_FFF4;
    const GENERIC_WRITE: u32 = 0x4000_0000;
    const FILE_SHARE_READ: u32 = 1;
    const FILE_SHARE_WRITE: u32 = 2;
    const OPEN_EXISTING: u32 = 3;
    const INVALID_HANDLE_VALUE: isize = -1;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn AttachConsole(process: u32) -> i32;
        fn CreateFileW(
            name: *const u16,
            access: u32,
            share: u32,
            security: *const c_void,
            disposition: u32,
            flags: u32,
            template: *mut c_void,
        ) -> isize;
        fn GetStdHandle(handle: u32) -> isize;
        fn SetStdHandle(handle: u32, value: isize) -> i32;
    }

    let output = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
    let error = unsafe { GetStdHandle(STD_ERROR_HANDLE) };
    if output != 0 && output != INVALID_HANDLE_VALUE && error != 0 && error != INVALID_HANDLE_VALUE
    {
        return;
    }
    if unsafe { AttachConsole(ATTACH_PARENT_PROCESS) } == 0 {
        return;
    }
    let name = "CONOUT$\0".encode_utf16().collect::<Vec<u16>>();
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return;
    }
    if output == 0 || output == INVALID_HANDLE_VALUE {
        unsafe {
            SetStdHandle(STD_OUTPUT_HANDLE, handle);
        }
    }
    if error == 0 || error == INVALID_HANDLE_VALUE {
        unsafe {
            SetStdHandle(STD_ERROR_HANDLE, handle);
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn console() {}

fn run(arguments: &[String]) -> Option<ExitCode> {
    match arguments.first().map(String::as_str) {
        None => Some(grana_app::gui(None, None)),
        Some("--capture") => match arguments {
            [_, path] => Some(grana_app::gui(Some(PathBuf::from(path)), None)),
            [_, path, dataset] => Some(grana_app::gui(
                Some(PathBuf::from(path)),
                Some(dataset.clone()),
            )),
            [_, path, flag, dataset] if flag == "--view" => Some(grana_app::viewer(
                Some(PathBuf::from(path)),
                Some(dataset.clone()),
            )),
            _ => None,
        },
        Some("--view") => match arguments {
            [_, path] => Some(grana_app::viewer(None, Some(path.clone()))),
            _ => None,
        },
        Some("--stitch") => grana_app::stitch_command(&arguments[1..]),
        Some("--clear-cache") => match arguments {
            [_] => Some(grana_app::clear_cache()),
            _ => None,
        },
        Some("--render") => grana_app::render_command(&arguments[1..]),
        Some("--batch") => {
            let mut paths = Vec::new();
            let mut jobs = None;
            let mut project = false;
            let mut channels = false;
            let mut index = 1;
            while index < arguments.len() {
                match arguments[index].as_str() {
                    "--jobs" => {
                        jobs = Some(parse_jobs(arguments.get(index + 1)?)?);
                        index += 2;
                    }
                    "--project" => {
                        project = true;
                        index += 1;
                    }
                    "--channels" => {
                        channels = true;
                        index += 1;
                    }
                    option if option.starts_with("--") => return None,
                    value => {
                        paths.push(PathBuf::from(value));
                        index += 1;
                    }
                }
            }
            match paths.as_slice() {
                [protocol, datasets, output] => Some(grana_app::batch(
                    protocol.as_path(),
                    datasets.as_path(),
                    output.as_path(),
                    jobs,
                    project,
                    channels,
                )),
                _ => None,
            }
        }
        Some(option) if option.starts_with("--") => None,
        Some(dataset) => match arguments {
            [_] => Some(grana_app::gui(None, Some(dataset.to_string()))),
            _ => None,
        },
    }
}

fn parse_jobs(value: &str) -> Option<usize> {
    value.parse::<usize>().ok().filter(|jobs| *jobs > 0)
}

fn usage() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}
