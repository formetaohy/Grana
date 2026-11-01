use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str =
    "usage: grana [--batch <protocol.json> <datasets> <output>] | grana [--capture <image.png>]";

fn main() -> ExitCode {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    match arguments.as_slice() {
        [] => grana_app::gui(None),
        [flag, path] if flag == "--capture" => grana_app::gui(Some(PathBuf::from(path))),
        [flag, protocol, datasets, output] if flag == "--batch" => {
            grana_app::batch(protocol.as_ref(), datasets.as_ref(), output.as_ref())
        }
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}
