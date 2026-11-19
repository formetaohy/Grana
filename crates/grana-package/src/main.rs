use std::ffi::OsStr;
use std::path::Path;
use std::process::ExitCode;

mod art;
mod canvas;
mod dist;
mod icon;

fn main() -> ExitCode {
    let mut arguments = std::env::args_os();
    let _ = arguments.next();
    let command = arguments.next();
    let rest = arguments.next();
    let outcome = match (command.as_deref(), rest) {
        (Some(command), None) if command == OsStr::new("installer") => {
            art::write(&workspace().join("installer"))
        }
        (Some(command), None) if command == OsStr::new("dist") => {
            dist::write(workspace()).map(|folder| println!("dist: {}", folder.display()))
        }
        _ => {
            eprintln!("usage: grana-package <installer|dist>");
            return ExitCode::from(2);
        }
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(detail) => {
            eprintln!("grana-package: {detail}");
            ExitCode::FAILURE
        }
    }
}

fn workspace() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the package lives in the workspace")
}
