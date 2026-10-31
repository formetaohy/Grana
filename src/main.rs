mod app;
mod demo;
mod gpu;
mod loader;
mod snapshot;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use grana_pipeline::{Batch, BatchEvent, Outcome, Plan, Protocol, discover};

const USAGE: &str = "usage: grana [--batch <protocol.json> <datasets> <output>]";

fn main() -> ExitCode {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    if arguments.is_empty() {
        let event_loop =
            winit::event_loop::EventLoop::new().expect("failed to create the event loop");
        event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);
        let mut app = app::App::new();
        event_loop
            .run_app(&mut app)
            .expect("the event loop terminated abnormally");
        return ExitCode::SUCCESS;
    }
    batch(&arguments)
}

fn batch(arguments: &[String]) -> ExitCode {
    if arguments.len() != 4 || arguments[0] != "--batch" {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }
    let protocol = match Protocol::read(Path::new(&arguments[1])) {
        Ok(protocol) => protocol,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let datasets = match discover(Path::new(&arguments[2])) {
        Ok(datasets) if datasets.is_empty() => {
            eprintln!("{} holds no OME-Zarr dataset", arguments[2]);
            return ExitCode::FAILURE;
        }
        Ok(datasets) => datasets,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let output = PathBuf::from(&arguments[3]);
    let running = Batch::start(Plan {
        protocol,
        datasets,
        output,
    });
    let mut failures = 0;
    while let Some(event) = running.wait() {
        match event {
            BatchEvent::Started { name, .. } => println!("{name}"),
            BatchEvent::Step { label, .. } => println!("  {label}"),
            BatchEvent::Progress { .. } => {}
            BatchEvent::Dataset { outcome, .. } => match outcome {
                Outcome::Complete { objects, detail } => println!("  {objects} objects · {detail}"),
                Outcome::Failed { detail } => {
                    eprintln!("  failed: {detail}");
                    failures += 1;
                }
            },
            BatchEvent::Failed(detail) => {
                eprintln!("{detail}");
                failures += 1;
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
