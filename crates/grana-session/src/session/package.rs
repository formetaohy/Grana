use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use grana_interop::Crate;
use grana_measure::Aggregate;
use grana_project::{Project, Source as ProjectSource};
use grana_scene::{TableRequest, Trace};

use super::*;

use crate::task::Progress;

const COPY_SHARE: usize = 80;
const ANALYSIS_SHARE: usize = 10;
const TOTAL: usize = 100;
const BUFFER: usize = 1 << 20;
const PROJECT_FILE: &str = "analysis.grana";
const PROTOCOL_FILE: &str = "protocol.json";
const DATA_FOLDER: &str = "data";
const MEASUREMENT_FOLDER: &str = "measurements";

pub(super) struct Export {
    task: Progress<Result<PathBuf, String>>,
}

pub(super) struct Request {
    pub root: PathBuf,
    pub name: String,
    pub fingerprint: Fingerprint,
    pub project: Project,
    pub protocol: Option<Protocol>,
    pub sets: Vec<Set>,
    pub source: PathBuf,
    pub data: PathBuf,
}

pub(super) struct Set {
    pub key: String,
    pub label: String,
    pub trace: Trace,
    pub table: TableRequest,
}

impl Export {
    pub fn start(request: Request) -> Self {
        let task = Progress::spawn(grana_exec::Role::Write, move |cancel, progress| {
            write(request, cancel, progress)
        });
        Self { task }
    }

    pub fn label(&self) -> &str {
        "Exporting the share package"
    }

    pub fn progress(&self) -> (usize, usize) {
        self.task.progress()
    }

    pub fn stepped(&mut self) -> bool {
        self.task.stepped()
    }

    pub fn poll(&mut self) -> Poll<Result<PathBuf, String>> {
        self.task.poll()
    }
}

fn write(
    request: Request,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<PathBuf, String> {
    let root = request.root.clone();
    if root.exists() {
        return Err(format!("{} already exists", root.display()));
    }
    std::fs::create_dir_all(&root)
        .map_err(|error| format!("failed to create {}: {error}", root.display()))?;
    match assemble(request, cancel, progress) {
        Ok(()) => Ok(root),
        Err(detail) => {
            let _ = std::fs::remove_dir_all(&root);
            Err(detail)
        }
    }
}

fn assemble(
    request: Request,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<(), String> {
    let Request {
        root,
        name,
        fingerprint,
        project,
        protocol,
        sets,
        source,
        data,
    } = request;
    let bytes = measure(&source)?;
    copy(&source, &root.join(&data), bytes, cancel, progress)?;
    progress(COPY_SHARE, TOTAL);
    let analysis = root.join(PROJECT_FILE);
    let mut steps = |done: usize, total: usize| {
        let share = ANALYSIS_SHARE * done / total.max(1);
        progress(COPY_SHARE + share, TOTAL);
    };
    project
        .write(&analysis, cancel, &mut steps)
        .map_err(|error| error.to_string())?;
    if stopped(cancel) {
        return Err("the share package was cancelled".to_string());
    }
    let mut record = Crate::new(
        &name,
        "A share package that records an analysis",
        "Grana",
        env!("CARGO_PKG_VERSION"),
    );
    record.fingerprint(&fingerprint.hex(), grana_image::FINGERPRINT_COVERAGE);
    record.part(
        PROJECT_FILE,
        PROJECT_FILE,
        "application/octet-stream",
        "the recorded analysis",
    );
    let reproduced = match protocol {
        Some(protocol) => {
            let path = root.join(PROTOCOL_FILE);
            protocol.write(&path).map_err(|error| error.to_string())?;
            record.part(
                PROTOCOL_FILE,
                PROTOCOL_FILE,
                "application/json",
                "the protocol that reproduces the analysis",
            );
            Some(PROTOCOL_FILE.to_string())
        }
        None => None,
    };
    let entry = relative(&data);
    record.collection(
        &entry,
        &name,
        "the dataset this analysis reads",
        None,
        Some(bytes),
    );
    let count = sets.len().max(1);
    for (index, set) in sets.into_iter().enumerate() {
        if stopped(cancel) {
            return Err("the share package was cancelled".to_string());
        }
        let Set {
            key,
            label,
            trace,
            table,
        } = set;
        let Some(table) = table.build() else {
            continue;
        };
        let file = format!("{MEASUREMENT_FOLDER}/{key}-objects.csv");
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
        let mut results = vec![file.clone()];
        results.extend(reproduced.clone());
        record.action(
            &format!("#create-{key}"),
            &format!("{} of {label}", trace.algorithm().title()),
            &format!(
                "{} v{} over {} objects",
                trace.algorithm().title(),
                trace.version(),
                trace.objects()
            ),
            &results,
        );
        let newest = COPY_SHARE + ANALYSIS_SHARE + ANALYSIS_SHARE * (index + 1) / count;
        progress(newest.min(TOTAL), TOTAL);
    }
    record.write(&root).map_err(|error| error.to_string())?;
    progress(TOTAL, TOTAL);
    Ok(())
}

fn stopped(cancel: &AtomicBool) -> bool {
    cancel.load(Ordering::Relaxed)
}

fn measure(path: &Path) -> Result<u64, String> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    if !metadata.is_dir() {
        return Ok(metadata.len());
    }
    let entries = std::fs::read_dir(path)
        .map_err(|error| format!("failed to list {}: {error}", path.display()))?;
    let mut total = 0;
    for entry in entries {
        let entry = entry.map_err(|error| format!("failed to list {}: {error}", path.display()))?;
        total += measure(&entry.path())?;
    }
    Ok(total)
}

fn copy(
    source: &Path,
    target: &Path,
    total: u64,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<(), String> {
    let mut copied = 0u64;
    copy_entry(source, target, total, &mut copied, cancel, progress)
}

fn copy_entry(
    source: &Path,
    target: &Path,
    total: u64,
    copied: &mut u64,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<(), String> {
    if stopped(cancel) {
        return Err("the share package was cancelled".to_string());
    }
    let metadata = std::fs::metadata(source)
        .map_err(|error| format!("failed to read {}: {error}", source.display()))?;
    if metadata.is_dir() {
        std::fs::create_dir_all(target)
            .map_err(|error| format!("failed to create {}: {error}", target.display()))?;
        let entries = std::fs::read_dir(source)
            .map_err(|error| format!("failed to list {}: {error}", source.display()))?;
        for entry in entries {
            let entry =
                entry.map_err(|error| format!("failed to list {}: {error}", source.display()))?;
            copy_entry(
                &entry.path(),
                &target.join(entry.file_name()),
                total,
                copied,
                cancel,
                progress,
            )?;
        }
        return Ok(());
    }
    copy_file(source, target, total, copied, cancel, progress)
}

fn copy_file(
    source: &Path,
    target: &Path,
    total: u64,
    copied: &mut u64,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<(), String> {
    let mut reader = std::fs::File::open(source)
        .map_err(|error| format!("failed to open {}: {error}", source.display()))?;
    if let Some(folder) = target.parent() {
        std::fs::create_dir_all(folder)
            .map_err(|error| format!("failed to create {}: {error}", folder.display()))?;
    }
    let mut writer = std::fs::File::create(target)
        .map_err(|error| format!("failed to create {}: {error}", target.display()))?;
    let mut buffer = vec![0u8; BUFFER];
    loop {
        if stopped(cancel) {
            return Err("the share package was cancelled".to_string());
        }
        let read = reader
            .read(&mut buffer)
            .map_err(|error| format!("failed to read {}: {error}", source.display()))?;
        if read == 0 {
            break;
        }
        writer
            .write_all(&buffer[..read])
            .map_err(|error| format!("failed to write {}: {error}", target.display()))?;
        *copied += read as u64;
        let share = COPY_SHARE * (*copied).min(total) as usize / total.max(1) as usize;
        progress(share.min(COPY_SHARE), TOTAL);
    }
    writer
        .sync_all()
        .map_err(|error| format!("failed to flush {}: {error}", target.display()))?;
    Ok(())
}

fn relative(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

impl Session {
    pub(super) fn export_package(&mut self) {
        let Some(parent) = rfd::FileDialog::new()
            .set_title("Export the share package into a folder")
            .pick_folder()
        else {
            return;
        };
        self.package_into(parent);
    }

    pub fn package_into(&mut self, parent: PathBuf) -> bool {
        if self.tasks.writing() {
            self.message = Some(Message::error("an export is already running"));
            return false;
        }
        if self.scene.is_none() {
            self.message = Some(Message::info(
                "open a dataset before exporting a share package",
            ));
            return false;
        }
        let Some(source) = self
            .source
            .as_ref()
            .and_then(|loader| loader.source().path().map(Path::to_path_buf))
        else {
            self.message = Some(Message::error(
                "a share package needs a local dataset; a remote or in-memory dataset cannot be packaged",
            ));
            return false;
        };
        let Some(fingerprint) = self.fingerprint else {
            self.message = Some(Message::error(
                "the loaded dataset carries no fingerprint for a share package",
            ));
            return false;
        };
        let Some(view) = self.current_view.clone() else {
            self.message = Some(Message::error(
                "the current view is not ready for a share package",
            ));
            return false;
        };
        let Some(name) = source.file_name().map(|name| name.to_os_string()) else {
            self.message = Some(Message::error(format!(
                "{} does not name a dataset",
                source.display()
            )));
            return false;
        };
        if self.protocol.is_none() {
            self.capture_protocol();
        }
        let protocol = self.protocol.clone();
        let Some(scene) = self.scene.as_ref() else {
            return false;
        };
        let mut sets = Vec::new();
        for set in scene.objects.iter() {
            if set.provenance().is_attached() {
                continue;
            }
            let Some(table) = TableRequest::new(scene, set.id(), Group::Object, Aggregate::MEAN)
            else {
                continue;
            };
            sets.push(Set {
                key: crate::slug(set.key().as_str()),
                label: set.name().to_string(),
                trace: Trace::of(set, Some(fingerprint)),
                table,
            });
        }
        let data = PathBuf::from(DATA_FOLDER).join(&name);
        let package = Project::capture(
            scene,
            &self.measure,
            ProjectSource::File {
                name: scene.dataset.name().to_string(),
                path: data.clone(),
                series: self.series_index,
            },
            view,
            self.views.clone(),
            fingerprint,
        );
        let root = parent.join(format!(
            "{}.{}",
            crate::slug(scene.dataset.name()),
            grana_project::PACKAGE_EXTENSION
        ));
        let title = scene.dataset.name().to_string();
        self.message = None;
        self.tasks.package = Some(Export::start(Request {
            root,
            name: title,
            fingerprint,
            project: package,
            protocol,
            sets,
            source,
            data,
        }));
        true
    }

    pub(super) fn finish_package(&mut self, outcome: Result<PathBuf, String>) {
        self.message = Some(match outcome {
            Ok(path) => Message::info(format!("the share package is at {}", path.display())),
            Err(detail) => Message::error(detail),
        });
    }
}
