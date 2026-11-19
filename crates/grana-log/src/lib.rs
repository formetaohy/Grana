use std::fs::{self, File, OpenOptions};
use std::panic;
use std::path::{Path, PathBuf};

use time::format_description::well_known::Rfc3339;
use time::{OffsetDateTime, UtcOffset};
use tracing::Level;
use tracing_appender::non_blocking::{NonBlockingBuilder, WorkerGuard};
use tracing_subscriber::filter::Targets;
use tracing_subscriber::fmt;
use tracing_subscriber::fmt::time::OffsetTime;
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
    let offset = UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);
    let path = directory.join(file_name(offset));
    let file = open(&path);
    prune(&directory, &path);
    let (writer, guard) = NonBlockingBuilder::default()
        .thread_name("grana-log")
        .lossy(false)
        .finish(file);
    let format = fmt::layer()
        .with_writer(writer)
        .with_timer(OffsetTime::new(offset, Rfc3339))
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
        file = %path.display(),
        "grana started"
    );
    Log { guard: Some(guard) }
}

fn directory() -> PathBuf {
    grana_dir::data()
        .expect("the system assigns no data directory for the application logs")
        .join(DIRECTORY)
}

fn file_name(offset: UtcOffset) -> String {
    let now = OffsetDateTime::now_utc().to_offset(offset);
    format!(
        "{PREFIX}.{:04}-{:02}-{:02}-{:02}-{:02}-{:02}-{:03}.{SUFFIX}",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second(),
        now.millisecond()
    )
}

fn open(path: &Path) -> File {
    OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
        .unwrap_or_else(|error| panic!("failed to open the log file {}: {error}", path.display()))
}

fn prune(directory: &Path, current: &Path) {
    loop {
        let mut names = log_names(directory);
        if names.len() <= LIMIT {
            return;
        }
        names.sort();
        let mut excess = names.len() - LIMIT;
        for name in names {
            if excess == 0 {
                break;
            }
            let path = directory.join(&name);
            if path == current {
                continue;
            }
            match fs::remove_file(&path) {
                Ok(()) => excess -= 1,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => excess -= 1,
                Err(error) => panic!(
                    "failed to remove the old log file {}: {error}",
                    path.display()
                ),
            }
        }
        if excess > 0 {
            return;
        }
    }
}

fn log_names(directory: &Path) -> Vec<String> {
    let entries = fs::read_dir(directory).unwrap_or_else(|error| {
        panic!(
            "failed to read the log directory {}: {error}",
            directory.display()
        )
    });
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.unwrap_or_else(|error| {
            panic!(
                "failed to inspect the log directory {}: {error}",
                directory.display()
            )
        });
        let file = entry.file_type().unwrap_or_else(|error| {
            panic!(
                "failed to inspect the log file {}: {error}",
                entry.path().display()
            )
        });
        if !file.is_file() {
            continue;
        }
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if name.starts_with(PREFIX) && name.ends_with(SUFFIX) {
            names.push(name);
        }
    }
    names
}

fn install_panic_hook() {
    let previous = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        tracing::error!(target: "grana::log", "{info}");
        previous(info);
    }));
}
