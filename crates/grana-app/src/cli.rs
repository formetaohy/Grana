use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use grana_pipeline::{Batch, BatchEvent, Control, Outcome, Plan, discover};
use grana_protocol::Protocol;
use winit::event_loop::{ControlFlow, EventLoop};

use crate::app::App;
use crate::render::Request;
use crate::snapshot::Snapshot;

pub fn gui(capture: Option<PathBuf>, startup: Option<String>) -> ExitCode {
    let event_loop = EventLoop::new().expect("failed to create the event loop");
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = App::new(capture, startup);
    event_loop
        .run_app(&mut app)
        .expect("the event loop terminated abnormally");
    ExitCode::SUCCESS
}

pub fn clear_cache() -> ExitCode {
    match grana_image::clear_cache() {
        Ok(bytes) => {
            println!(
                "cleared {:.1} MiB of derived pyramid tiles",
                bytes as f64 / (1024.0 * 1024.0)
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("failed to clear the derived pyramid cache: {error}");
            ExitCode::FAILURE
        }
    }
}

pub fn batch(
    protocol: &Path,
    datasets: &Path,
    output: &Path,
    jobs: Option<usize>,
    project: bool,
    channels: bool,
) -> ExitCode {
    let protocol = match Protocol::read(protocol) {
        Ok(protocol) => protocol,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let datasets = match discover(datasets) {
        Ok(found) if found.is_empty() => {
            eprintln!("{} holds no dataset", datasets.display());
            return ExitCode::FAILURE;
        }
        Ok(found) => found,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let running = Batch::start(Plan {
        protocol,
        datasets,
        output: output.to_path_buf(),
        jobs: jobs.unwrap_or_else(grana_pipeline::default_jobs),
        project,
        channels,
        snapshot: Some(Arc::new(Snapshot::new())),
    });
    listen(running.control());
    let mut failures = 0;
    while let Some(event) = running.wait() {
        match event {
            BatchEvent::Started { name, .. } => println!("{name}"),
            BatchEvent::Step { label, .. } => println!("  {label}"),
            BatchEvent::Progress { .. } => {}
            BatchEvent::Dataset { outcome, .. } => match outcome {
                Outcome::Complete {
                    objects,
                    detail,
                    resumed,
                } => {
                    if resumed {
                        println!("  {objects} objects · {detail} (resumed)");
                    } else {
                        println!("  {objects} objects · {detail}");
                    }
                }
                Outcome::Failed { detail } => {
                    eprintln!("  failed: {detail}");
                    failures += 1;
                }
            },
            BatchEvent::Failed(detail) => {
                eprintln!("{detail}");
                failures += 1;
            }
            BatchEvent::Comparison(report) => {
                for row in &report.groups {
                    println!(
                        "  {} · {} ({}) over {} datasets",
                        row.group, row.name, row.kind, row.datasets
                    );
                }
                for test in report.tests.iter().filter(|test| test.significant()) {
                    println!(
                        "  Welch {} vs {} · {} · {}: n={}/{} p={} q={} g={} {}",
                        test.group_a,
                        test.group_b,
                        test.name,
                        test.metric.name(),
                        test.test.n_a(),
                        test.test.n_b(),
                        test.p_text(),
                        test.q_text(),
                        test.effect
                            .map_or_else(|| "-".to_string(), |effect| format!("{effect:.2}")),
                        test.mark()
                    );
                }
            }
            BatchEvent::Done { cancelled } => {
                if cancelled {
                    eprintln!("the batch was cancelled");
                    failures += 1;
                }
                return if failures == 0 {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::FAILURE
                };
            }
        }
    }
    eprintln!("the batch stopped before it finished");
    ExitCode::FAILURE
}

pub fn render(arguments: &[String]) -> Option<ExitCode> {
    let mut paths = Vec::new();
    let mut width = None;
    let mut height = None;
    let mut time = None;
    let mut view = None;
    let mut transparent = false;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "--width" => {
                width = Some(arguments.get(index + 1)?.parse().ok()?);
                index += 2;
            }
            "--height" => {
                height = Some(arguments.get(index + 1)?.parse().ok()?);
                index += 2;
            }
            "--time" => {
                time = Some(arguments.get(index + 1)?.parse().ok()?);
                index += 2;
            }
            "--view" => {
                view = Some(arguments.get(index + 1)?.clone());
                index += 2;
            }
            "--transparent" => {
                transparent = true;
                index += 1;
            }
            option if option.starts_with("--") => return None,
            value => {
                paths.push(PathBuf::from(value));
                index += 1;
            }
        }
    }
    let [input, output] = paths.as_slice() else {
        return None;
    };
    let request = Request {
        input: input.clone(),
        output: output.clone(),
        width,
        height,
        time,
        view,
        transparent,
    };
    match crate::render::run(&request) {
        Ok(path) => {
            println!("{}", path.display());
            Some(ExitCode::SUCCESS)
        }
        Err(detail) => {
            eprintln!("{detail}");
            Some(ExitCode::FAILURE)
        }
    }
}

fn listen(control: Control) {
    println!("press p to pause or resume the batch, c to cancel it");
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut line = String::new();
        loop {
            line.clear();
            match stdin.read_line(&mut line) {
                Ok(0) | Err(_) => return,
                Ok(_) => {}
            }
            match line.trim().to_ascii_lowercase().as_str() {
                "p" if control.paused() => {
                    control.resume();
                    println!("the batch resumed");
                }
                "p" => {
                    control.pause();
                    println!("the batch paused");
                }
                "c" => {
                    control.cancel();
                    println!("the batch is cancelling");
                    return;
                }
                "" => {}
                _ => println!("press p to pause or resume the batch, c to cancel it"),
            }
        }
    });
}
