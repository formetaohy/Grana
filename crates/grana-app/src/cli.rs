use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use grana_pipeline::{Batch, BatchEvent, Control, Outcome, Plan, discover};
use grana_protocol::Protocol;
use grana_session::Access;
use winit::event_loop::{ControlFlow, EventLoop};

use crate::app::App;
use crate::capture::OfflineCapture;
use crate::render::Request;

pub fn gui(capture: Option<PathBuf>, startup: Option<String>) -> ExitCode {
    open(capture, startup, Access::Author)
}

pub fn viewer(capture: Option<PathBuf>, startup: Option<String>) -> ExitCode {
    open(capture, startup, Access::Viewer)
}

fn open(capture: Option<PathBuf>, startup: Option<String>, access: Access) -> ExitCode {
    tracing::info!(
        target: "grana::app",
        viewer = !access.author(),
        ?capture,
        ?startup,
        "opening the application"
    );
    let event_loop = EventLoop::<crate::app::Wake>::with_user_event()
        .build()
        .expect("failed to create the event loop");
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = App::new(capture, startup, access, event_loop.create_proxy());
    event_loop
        .run_app(&mut app)
        .expect("the event loop terminated abnormally");
    if app.failed().is_some() {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

pub fn clear_cache() -> ExitCode {
    match grana_image::clear_cache() {
        Ok(bytes) => {
            tracing::info!(target: "grana::cache", bytes, "cleared the derived pyramid cache");
            println!(
                "cleared {:.1} MiB of derived pyramid tiles",
                bytes as f64 / (1024.0 * 1024.0)
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            tracing::error!(target: "grana::cache", %error, "failed to clear the derived pyramid cache");
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
    tracing::info!(
        target: "grana::batch",
        protocol = %protocol.display(),
        datasets = %datasets.display(),
        output = %output.display(),
        jobs,
        project,
        channels,
        "starting the batch"
    );
    let protocol = match Protocol::read(protocol) {
        Ok(protocol) => protocol,
        Err(error) => {
            tracing::error!(target: "grana::batch", %error, "failed to read the protocol");
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let datasets = match discover(datasets, protocol.stitch().is_some()) {
        Ok(found) if found.is_empty() => {
            tracing::error!(target: "grana::batch", "the dataset folder is empty");
            eprintln!("{} holds no dataset", datasets.display());
            return ExitCode::FAILURE;
        }
        Ok(found) => found,
        Err(error) => {
            tracing::error!(target: "grana::batch", %error, "failed to discover the datasets");
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
        capture: Some(Arc::new(OfflineCapture::new())),
    });
    listen(running.control());
    let mut failures = 0;
    while let Some(event) = running.wait() {
        match event {
            BatchEvent::Started { index, name } => {
                tracing::info!(target: "grana::batch", index, %name, "a dataset started");
                println!("{name}");
            }
            BatchEvent::Step { label, .. } => println!("  {label}"),
            BatchEvent::Progress { .. } => {}
            BatchEvent::Dataset { index, outcome } => match outcome {
                Outcome::Complete {
                    objects,
                    detail,
                    resumed,
                } => {
                    tracing::info!(
                        target: "grana::batch",
                        index,
                        objects,
                        %detail,
                        resumed,
                        "a dataset finished"
                    );
                    if resumed {
                        println!("  {objects} objects · {detail} (resumed)");
                    } else {
                        println!("  {objects} objects · {detail}");
                    }
                }
                Outcome::Failed { detail } => {
                    tracing::error!(target: "grana::batch", index, %detail, "a dataset failed");
                    eprintln!("  failed: {detail}");
                    failures += 1;
                }
            },
            BatchEvent::Failed(detail) => {
                tracing::error!(target: "grana::batch", %detail, "the batch failed");
                eprintln!("{detail}");
                failures += 1;
            }
            BatchEvent::Comparison(report) => {
                for row in &report.groups {
                    println!(
                        "  {} · {} ({}) over {} objects in {} images",
                        row.group, row.name, row.kind, row.objects, row.images
                    );
                }
                for test in report.tests.iter().filter(|test| test.significant()) {
                    println!(
                        "  {} vs {} · {} · {} on {}: {} samples in {} images per group, Welch p={} q={} · image rank p={} q={} g={} Δ={:.2} {}",
                        test.group_a,
                        test.group_b,
                        test.name,
                        test.metric.name(),
                        test.level().key(),
                        test.test.samples_a(),
                        test.test.clusters_a(),
                        test.p_text(),
                        test.q_text(),
                        test.rank_p_text(),
                        test.rank_q_text(),
                        test.effect
                            .map_or_else(|| "-".to_string(), |effect| format!("{effect:.2}")),
                        test.rank.delta(),
                        test.mark()
                    );
                }
            }
            BatchEvent::Done { cancelled } => {
                if cancelled {
                    tracing::warn!(target: "grana::batch", "the batch was cancelled");
                    eprintln!("the batch was cancelled");
                    failures += 1;
                }
                tracing::info!(target: "grana::batch", failures, cancelled, "the batch finished");
                return if failures == 0 {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::FAILURE
                };
            }
        }
    }
    tracing::error!(target: "grana::batch", "the batch stopped before it finished");
    eprintln!("the batch stopped before it finished");
    ExitCode::FAILURE
}

pub fn render(arguments: &[String]) -> Option<ExitCode> {
    let mut paths = Vec::new();
    let mut width = None;
    let mut height = None;
    let mut time = None;
    let mut view = None;
    let mut lineage = None;
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
            "--lineage" => {
                lineage = Some(arguments.get(index + 1)?.clone());
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
        lineage,
        transparent,
    };
    match crate::render::run(&request) {
        Ok(path) => {
            tracing::info!(target: "grana::render", output = %path.display(), "the render finished");
            println!("{}", path.display());
            Some(ExitCode::SUCCESS)
        }
        Err(detail) => {
            tracing::error!(target: "grana::render", %detail, "the render failed");
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
