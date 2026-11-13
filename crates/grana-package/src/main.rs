use std::path::PathBuf;
use std::process::ExitCode;

mod art;
mod canvas;
mod icon;

fn main() -> ExitCode {
    let mut arguments = std::env::args_os();
    let _ = arguments.next();
    match (arguments.next(), arguments.next()) {
        (Some(root), None) => match art::write(&PathBuf::from(root)) {
            Ok(()) => ExitCode::SUCCESS,
            Err(detail) => {
                eprintln!("grana-package: {detail}");
                ExitCode::FAILURE
            }
        },
        _ => {
            eprintln!("usage: grana-package <installer-directory>");
            ExitCode::from(2)
        }
    }
}
