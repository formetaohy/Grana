use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use grana_interop::Crate;
use grana_scene::{TableRequest, Trace};

use super::*;

pub(super) struct Export {
    task: Task<Event>,
    progress: (usize, usize),
}

pub(super) enum Event {
    Progress(usize, usize),
    Done(Result<PathBuf, String>),
}

pub(super) struct Request {
    pub root: PathBuf,
    pub name: String,
    pub fingerprint: Option<Fingerprint>,
    pub source: Option<PathBuf>,
    pub protocol: Protocol,
    pub sets: Vec<Set>,
}

pub(super) struct Set {
    pub key: String,
    pub label: String,
    pub trace: Trace,
    pub table: TableRequest,
}

impl Export {
    pub fn start(request: Request) -> Self {
        let total = request.sets.len();
        let task = Task::spawn(move |cancel, sink| {
            let outcome = write(request, cancel, &mut |done: usize, total: usize| {
                sink(Event::Progress(done, total));
            });
            sink(Event::Done(outcome));
        });
        Self {
            task,
            progress: (0, total),
        }
    }

    pub fn progress(&self) -> (usize, usize) {
        self.progress
    }

    pub fn poll(&mut self) -> Poll<Event> {
        loop {
            match self.task.poll() {
                Poll::Event(Event::Progress(done, total)) => self.progress = (done, total),
                other => return other,
            }
        }
    }
}

fn write(
    request: Request,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<PathBuf, String> {
    let Request {
        root,
        name,
        fingerprint,
        source,
        protocol,
        sets,
    } = request;
    let stopped = || cancel.load(Ordering::Relaxed);
    let protocol_path = root.join("protocol.json");
    protocol
        .write(&protocol_path)
        .map_err(|error| error.to_string())?;
    let mut record = Crate::new(
        &name,
        "The analysis of a microscopy dataset",
        "Grana",
        env!("CARGO_PKG_VERSION"),
    );
    if let Some(fingerprint) = fingerprint {
        record.fingerprint(&fingerprint.hex(), grana_image::FINGERPRINT_COVERAGE);
    }
    if let Some(path) = source.as_deref() {
        record.source(path);
    }
    record.part(
        "protocol.json",
        "protocol.json",
        "application/json",
        "the protocol that reproduces the analysis",
    );
    let total = sets.len();
    for (done, set) in sets.into_iter().enumerate() {
        if stopped() {
            return Err("the provenance export was cancelled".to_string());
        }
        progress(done, total);
        let Set {
            key,
            label,
            trace,
            table,
        } = set;
        let Some(table) = table.build() else {
            continue;
        };
        let file = format!("measurements/{key}-objects.csv");
        let path = root.join(&file);
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder)
                .map_err(|error| format!("failed to create {}: {error}", folder.display()))?;
        }
        let rows = (0..table.rows()).collect::<Vec<_>>();
        std::fs::write(&path, table.csv(&rows))
            .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
        record.part(
            &file,
            &format!("{key}-objects.csv"),
            "text/csv",
            &format!("the objects of {label} at every frame"),
        );
        record.action(
            &format!("#create-{key}"),
            &format!("{} of {label}", trace.algorithm().title()),
            &format!(
                "{} v{} over {} objects",
                trace.algorithm().title(),
                trace.version(),
                trace.objects()
            ),
            &[file, "protocol.json".to_string()],
        );
    }
    progress(total, total);
    record.write(&root).map_err(|error| error.to_string())
}
