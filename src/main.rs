use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: grana [<dataset>] | grana --capture <image.png> [<dataset>] | grana --render <project|dataset> <image.png|image.ome.tif|image.svg|image.pdf|movie.mp4|movie.mov|folder> [--width <n>] [--height <n>] [--time <n>] [--view <name>] [--transparent] | grana --batch <protocol.json> <datasets> <output> [--jobs <n>] [--project] [--channels] | grana --clear-cache";

fn main() -> ExitCode {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    match run(&arguments) {
        Some(outcome) => outcome,
        None => usage(),
    }
}

fn run(arguments: &[String]) -> Option<ExitCode> {
    match arguments.first().map(String::as_str) {
        None => Some(grana_app::gui(None, None)),
        Some("--capture") => match arguments {
            [_, path] => Some(grana_app::gui(Some(PathBuf::from(path)), None)),
            [_, path, dataset] => Some(grana_app::gui(
                Some(PathBuf::from(path)),
                Some(dataset.clone()),
            )),
            _ => None,
        },
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
