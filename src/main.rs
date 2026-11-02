use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: grana [--batch <protocol.json> <datasets> <output> [--jobs <n>]] | grana [--capture <image.png>]";

fn main() -> ExitCode {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    match arguments.as_slice() {
        [] => grana_app::gui(None),
        [flag, path] if flag == "--capture" => grana_app::gui(Some(PathBuf::from(path))),
        [flag, protocol, datasets, output] if flag == "--batch" => {
            grana_app::batch(protocol.as_ref(), datasets.as_ref(), output.as_ref(), None)
        }
        [flag, protocol, datasets, output, option, jobs]
            if flag == "--batch" && option == "--jobs" =>
        {
            match parse_jobs(jobs) {
                Some(jobs) => grana_app::batch(
                    protocol.as_ref(),
                    datasets.as_ref(),
                    output.as_ref(),
                    Some(jobs),
                ),
                None => usage(),
            }
        }
        _ => usage(),
    }
}

fn parse_jobs(value: &str) -> Option<usize> {
    value.parse::<usize>().ok().filter(|jobs| *jobs > 0)
}

fn usage() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}
