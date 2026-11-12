use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use grana_interop::Crate;
use grana_measure::{Aggregate, Table};
use grana_pipeline::{Capture, CaptureKind, CaptureRequest, Channels};
use grana_project::{Project, Source as ProjectSource, ViewState};
use grana_protocol::Protocol;
use grana_report::share;
use grana_scene::{Scene, TableRequest, Trace};

use super::*;

use crate::task::Progress;

const COPY_SHARE: usize = 70;
const PROJECT_SHARE: usize = 12;
const TABLE_SHARE: usize = 10;
const REPORT_SHARE: usize = 8;
const TOTAL: usize = 100;
const BUFFER: usize = 1 << 20;
const PROJECT_FILE: &str = "analysis.grana";
const PROTOCOL_FILE: &str = "protocol.json";
const REPORT_FILE: &str = "report.html";
const THUMBNAIL_FILE: &str = "thumbnail.png";
const METADATA_FILE: &str = "ro-crate-metadata.json";
const DATA_FOLDER: &str = "data";
const MEASUREMENT_FOLDER: &str = "measurements";
const THUMBNAIL_SIZE: [u32; 2] = [1440, 960];

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
    pub scene: Scene,
    pub channels: Option<Arc<Channels>>,
    pub capture: Option<Arc<dyn Capture>>,
    pub view: ViewState,
    pub series: (usize, usize),
}

pub(super) struct Set {
    pub key: String,
    pub label: String,
    pub kind: &'static str,
    pub trace: Trace,
    pub tracks: Option<usize>,
    pub table: TableRequest,
}

struct Measured {
    label: String,
    kind: &'static str,
    origin: String,
    objects: usize,
    tracks: Option<usize>,
    file: String,
    table: Table,
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
        scene,
        channels,
        capture,
        view,
        series,
    } = request;
    let bytes = measure(&source)?;
    copy(&source, &root.join(&data), bytes, cancel, progress)?;
    progress(COPY_SHARE, TOTAL);
    let analysis = root.join(PROJECT_FILE);
    let written = COPY_SHARE + PROJECT_SHARE;
    let mut steps = |done: usize, total: usize| {
        let share = PROJECT_SHARE * done / total.max(1);
        progress(COPY_SHARE + share, TOTAL);
    };
    project
        .write(&analysis, cancel, &mut steps)
        .map_err(|error| error.to_string())?;
    if stopped(cancel) {
        return Err("the share package was cancelled".to_string());
    }
    progress(written, TOTAL);
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
    let reproduced = match protocol.as_ref() {
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
    let mut measured = Vec::with_capacity(sets.len());
    for (index, set) in sets.into_iter().enumerate() {
        if stopped(cancel) {
            return Err("the share package was cancelled".to_string());
        }
        let Set {
            key,
            label,
            kind,
            trace,
            tracks,
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
        measured.push(Measured {
            label,
            kind,
            origin: format!("{} · v{}", trace.algorithm().title(), trace.version()),
            objects: trace.objects(),
            tracks,
            file,
            table,
        });
        let newest = written + TABLE_SHARE * (index + 1) / count;
        progress(newest.min(written + TABLE_SHARE), TOTAL);
    }
    let thumbnail = match (channels.as_deref(), capture.as_deref()) {
        (Some(channels), Some(capture)) => {
            thumbnail(&root, &scene, channels, capture, &view, cancel)?
        }
        _ => None,
    };
    progress(TOTAL - REPORT_SHARE, TOTAL);
    let document = document(
        &scene,
        fingerprint,
        &measured,
        protocol.as_ref(),
        &entry,
        series,
        thumbnail.is_some(),
    );
    let page = share::html(&share::Report {
        title: &document.title,
        facts: &document.facts,
        thumbnail: thumbnail.as_deref(),
        sections: &document.sections,
        steps: &document.steps,
        files: &document.files,
        version: env!("CARGO_PKG_VERSION"),
        created: &document.created,
    })?;
    let path = root.join(REPORT_FILE);
    std::fs::write(&path, page)
        .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
    record.date(&document.created);
    record.part(
        REPORT_FILE,
        REPORT_FILE,
        "text/html",
        "a self contained report that reads in a browser",
    );
    if thumbnail.is_some() {
        record.part(
            THUMBNAIL_FILE,
            THUMBNAIL_FILE,
            "image/png",
            "the recorded view of this report",
        );
    }
    record.write(&root).map_err(|error| error.to_string())?;
    progress(TOTAL, TOTAL);
    Ok(())
}

fn thumbnail(
    root: &Path,
    scene: &Scene,
    channels: &Channels,
    capture: &dyn Capture,
    view: &ViewState,
    cancel: &AtomicBool,
) -> Result<Option<Vec<u8>>, String> {
    if stopped(cancel) {
        return Err("the share package was cancelled".to_string());
    }
    let path = root.join(THUMBNAIL_FILE);
    let render = Channels::render(channels.image_shared(), scene);
    let request = CaptureRequest {
        view: view.clone(),
        size: THUMBNAIL_SIZE,
        transparent: false,
        kind: CaptureKind::Still { time: scene.time() },
    };
    capture.capture(&render, scene, &request, &path)?;
    std::fs::read(&path)
        .map(Some)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))
}

struct Document<'a> {
    title: String,
    facts: Vec<(String, String)>,
    sections: Vec<share::Section<'a>>,
    steps: Vec<String>,
    files: Vec<String>,
    created: String,
}

fn document<'a>(
    scene: &Scene,
    fingerprint: Fingerprint,
    measured: &'a [Measured],
    protocol: Option<&Protocol>,
    data: &str,
    series: (usize, usize),
    thumbnail: bool,
) -> Document<'a> {
    let dataset = &scene.dataset;
    let sections = measured
        .iter()
        .map(|measured| share::Section {
            name: &measured.label,
            kind: measured.kind,
            origin: &measured.origin,
            objects: measured.objects,
            tracks: measured.tracks,
            table: &measured.table,
            file: &measured.file,
        })
        .collect::<Vec<_>>();
    let steps = protocol
        .map(|protocol| {
            protocol
                .summary()
                .split(" · ")
                .filter(|clause| !clause.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut files = vec![
        format!("{data} — the dataset this analysis reads"),
        format!("{PROJECT_FILE} — the recorded analysis, opens in Grana"),
        format!("{REPORT_FILE} — this report"),
        format!("{METADATA_FILE} — the manifest of this package"),
    ];
    if thumbnail {
        files.push(format!(
            "{THUMBNAIL_FILE} — the recorded view of this report"
        ));
    }
    if protocol.is_some() {
        files.push(format!(
            "{PROTOCOL_FILE} — the steps that reproduce the analysis"
        ));
    }
    for measured in measured {
        files.push(format!(
            "{} — every object of {} at every frame",
            measured.file, measured.label
        ));
    }
    Document {
        title: dataset.name().to_string(),
        facts: facts(scene, fingerprint, measured, series),
        sections,
        steps,
        files,
        created: grana_interop::utc_now(),
    }
}

fn facts(
    scene: &Scene,
    fingerprint: Fingerprint,
    measured: &[Measured],
    series: (usize, usize),
) -> Vec<(String, String)> {
    let dataset = &scene.dataset;
    let extent = dataset.extent();
    let spacing = dataset.spacing();
    let world = dataset.world_size();
    let channels = dataset
        .channels()
        .iter()
        .map(|channel| channel.name().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let frames = dataset.frame_count();
    let objects = measured
        .iter()
        .map(|measured| measured.objects)
        .sum::<usize>();
    let mut facts = vec![
        (
            "dimensions".to_string(),
            format!("{} × {} × {} voxels", extent.x(), extent.y(), extent.z()),
        ),
        (
            "voxel size".to_string(),
            format!(
                "{:.3} × {:.3} × {:.3} µm",
                spacing.x(),
                spacing.y(),
                spacing.z()
            ),
        ),
        (
            "field of view".to_string(),
            format!("{:.2} × {:.2} × {:.2} µm", world.x, world.y, world.z),
        ),
        (
            "time".to_string(),
            if frames > 1 {
                format!("{frames} frames · {:.3} s apart", dataset.frame_interval())
            } else {
                "one frame".to_string()
            },
        ),
        ("channels".to_string(), channels),
        (
            "object sets".to_string(),
            format!("{} · {objects} objects", measured.len()),
        ),
        (
            "stored levels".to_string(),
            format!("{} of {}", dataset.stored_levels(), dataset.level_count()),
        ),
    ];
    if series.1 > 1 {
        facts.push((
            "series".to_string(),
            format!("{} of {}", series.0 + 1, series.1),
        ));
    }
    facts.push((
        "source fingerprint".to_string(),
        format!(
            "{}…",
            fingerprint.hex().chars().take(16).collect::<String>()
        ),
    ));
    facts
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
                kind: set.kind().name(),
                trace: Trace::of(set, Some(fingerprint)),
                tracks: set.tracks().map(|layer| layer.set().len()),
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
            view.clone(),
            self.views.clone(),
            fingerprint,
        );
        let root = parent.join(format!(
            "{}.{}",
            crate::slug(scene.dataset.name()),
            grana_project::PACKAGE_EXTENSION
        ));
        let title = scene.dataset.name().to_string();
        let scene = scene.snapshot().into_scene();
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
            scene,
            channels: self.channels.clone(),
            capture: self.capturer.as_ref().map(|factory| factory()),
            view,
            series: (self.series_index, self.series.len()),
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
