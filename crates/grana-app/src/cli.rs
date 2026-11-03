use std::path::{Path, PathBuf};
use std::process::ExitCode;

use grana_session::{Batch, BatchEvent, Outcome, Plan, Protocol, discover};
use winit::event_loop::{ControlFlow, EventLoop};

use crate::app::App;

pub fn gui(capture: Option<PathBuf>) -> ExitCode {
    let event_loop = EventLoop::new().expect("failed to create the event loop");
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = App::new(capture);
    event_loop
        .run_app(&mut app)
        .expect("the event loop terminated abnormally");
    ExitCode::SUCCESS
}

pub fn batch(
    protocol: &Path,
    datasets: &Path,
    output: &Path,
    jobs: Option<usize>,
    project: bool,
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
        jobs: jobs.unwrap_or_else(grana_session::default_jobs),
        project,
    });
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
