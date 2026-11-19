use std::fs;
use std::panic;
use std::path::PathBuf;

use tracing::Level;
use tracing_appender::non_blocking::{NonBlockingBuilder, WorkerGuard};
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::filter::Targets;
use tracing_subscriber::fmt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry;
use tracing_subscriber::util::SubscriberInitExt;

const DIRECTORY: &str = "logs";
const PREFIX: &str = "grana";
const SUFFIX: &str = "log";
const LIMIT: usize = 10;

pub struct Log {
    guard: Option<WorkerGuard>,
}

impl Drop for Log {
    fn drop(&mut self) {
        tracing::info!(target: "grana::log", "grana stopped");
        if let Some(guard) = self.guard.take() {
            drop(guard);
        }
    }
}

pub fn start() -> Log {
    let directory = directory();
    fs::create_dir_all(&directory).unwrap_or_else(|error| {
        panic!(
            "failed to create the log directory {}: {error}",
            directory.display()
        )
    });
    let appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(PREFIX)
        .filename_suffix(SUFFIX)
        .max_log_files(LIMIT)
        .build(&directory)
        .unwrap_or_else(|error| {
            panic!(
                "failed to open the log files in {}: {error}",
                directory.display()
            )
        });
    let (writer, guard) = NonBlockingBuilder::default()
        .thread_name("grana-log")
        .lossy(false)
        .finish(appender);
    let format = fmt::layer()
        .with_writer(writer)
        .with_ansi(false)
        .with_target(true)
        .compact();
    let filter = Targets::new()
        .with_default(Level::WARN)
        .with_target("grana", Level::INFO);
    registry()
        .with(format)
        .with(filter)
        .try_init()
        .expect("the log subscriber installs once per process");
    install_panic_hook();
    tracing::info!(
        target: "grana::log",
        version = env!("CARGO_PKG_VERSION"),
        os = std::env::consts::OS,
        arch = std::env::consts::ARCH,
        directory = %directory.display(),
        "grana started"
    );
    Log { guard: Some(guard) }
}

fn directory() -> PathBuf {
    grana_dir::data()
        .expect("the system assigns no data directory for the application logs")
        .join(DIRECTORY)
}

fn install_panic_hook() {
    let previous = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        tracing::error!(target: "grana::log", "{info}");
        previous(info);
    }));
}
