use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use grana_pipeline::{Batch, BatchEvent, Control, Outcome, Plan, discover};
use grana_protocol::Protocol;
use winit::event_loop::{ControlFlow, EventLoop};

use crate::app::App;
use crate::snapshot::Snapshot;

pub fn gui(capture: Option<PathBuf>) -> ExitCode {
    let event_loop = EventLoop::new().expect("failed to create the event loop");
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = App::new(capture);
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
            BatchEvent::Comparison(comparison) => {
                for row in comparison {
                    println!(
                        "  {} · {} ({}) over {} datasets",
                        row.group, row.name, row.kind, row.datasets
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
