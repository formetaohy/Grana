use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

mod art;
mod canvas;
mod dist;
mod icon;
mod manifest;

fn main() -> ExitCode {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    let outcome = match arguments.as_slice() {
        [command, rest @ ..] if command == OsStr::new("installer") && rest.is_empty() => {
            art::write(&workspace().join("installer"))
        }
        [command, rest @ ..] if command == OsStr::new("dist") => match binary(rest) {
            Some(binary) => dist::write(workspace(), binary.as_deref())
                .map(|folder| println!("dist: {}", folder.display())),
            None => return usage(),
        },
        [command, rest @ ..] if command == OsStr::new("manifest") && rest.is_empty() => {
            manifest::write(workspace()).map(|path| println!("manifest: {}", path.display()))
        }
        _ => return usage(),
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(detail) => {
            eprintln!("grana-package: {detail}");
            ExitCode::FAILURE
        }
    }
}

fn binary(arguments: &[std::ffi::OsString]) -> Option<Option<PathBuf>> {
    match arguments {
        [] => Some(None),
        [flag, path] if flag == OsStr::new("--binary") => Some(Some(PathBuf::from(path))),
        _ => None,
    }
}

fn usage() -> ExitCode {
    eprintln!("usage: grana-package <installer|dist [--binary <path>]|manifest>");
    ExitCode::from(2)
}

fn workspace() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the package lives in the workspace")
}
