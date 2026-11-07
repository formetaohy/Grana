use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: grana [--batch <protocol.json> <datasets> <output> [--jobs <n>] [--project] [--channels]] | grana [--capture <image.png>] | grana [--clear-cache]";

fn main() -> ExitCode {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    match run(&arguments) {
        Some(outcome) => outcome,
        None => usage(),
    }
}

fn run(arguments: &[String]) -> Option<ExitCode> {
    match arguments.first().map(String::as_str) {
        None => Some(grana_app::gui(None)),
        Some("--capture") => match arguments {
            [_, path] => Some(grana_app::gui(Some(PathBuf::from(path)))),
            _ => None,
        },
        Some("--clear-cache") => match arguments {
            [_] => Some(grana_app::clear_cache()),
            _ => None,
        },
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
        Some(_) => None,
    }
}

fn parse_jobs(value: &str) -> Option<usize> {
    value.parse::<usize>().ok().filter(|jobs| *jobs > 0)
}

fn usage() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}
