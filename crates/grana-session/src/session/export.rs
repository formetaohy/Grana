use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use grana_measure::{Aggregate, Group, Table};
use grana_mesh::Format as MeshFormat;
use grana_scene::{MeasureKey, TableRequest, Trace};

use super::*;

use crate::task::Progress;

fn write(
    path: &Path,
    table: &Table,
    rows: &[u32],
    progress: &mut dyn FnMut(usize, usize),
) -> Result<PathBuf, String> {
    let fault = |error: std::io::Error| {
        format!("failed to export statistics to {}: {error}", path.display())
    };
    stream(path, |writer| table.write_csv_rows(writer, rows, progress)).map_err(fault)?;
    table.release();
    Ok(path.to_path_buf())
}

pub(super) fn write_whole(path: &Path, table: &Table) -> Result<(), String> {
    let fault = |error: std::io::Error| format!("failed to write {}: {error}", path.display());
    stream(path, |writer| table.write_csv(writer, &mut |_, _| {})).map_err(fault)?;
    table.release();
    Ok(())
}

fn stream(
    path: &Path,
    fill: impl FnOnce(&mut std::io::BufWriter<std::fs::File>) -> std::io::Result<()>,
) -> std::io::Result<()> {
    let file = grana_file::Atomic::file(path)?;
    {
        let mut writer = std::io::BufWriter::new(std::fs::File::create(file.staging())?);
        fill(&mut writer)?;
        std::io::Write::flush(&mut writer)?;
    }
    file.commit()
}

pub(super) struct Writer {
    task: Progress<Result<PathBuf, String>>,
    label: String,
}

impl Writer {
    pub(super) fn start(
        label: String,
        path: PathBuf,
        work: impl FnOnce(&AtomicBool, &mut dyn FnMut(usize, usize), &Path) -> Result<(), String>
        + Send
        + 'static,
    ) -> Self {
        let returned = path.clone();
        let task = Progress::spawn(grana_exec::Role::Write, move |cancel, progress| {
            work(cancel, progress, &path)?;
            Ok(returned)
        });
        Self { task, label }
    }

    pub(super) fn label(&self) -> &str {
        &self.label
    }

    pub(super) fn progress(&self) -> (usize, usize) {
        self.task.progress()
    }

    pub(super) fn stepped(&mut self) -> bool {
        self.task.stepped()
    }

    pub(super) fn poll(&mut self) -> Poll<Result<PathBuf, String>> {
        let outcome = self.task.poll();
        match &outcome {
            Poll::Event(Ok(path)) => {
                grana_log::info!(target: "grana::session", export = %self.label, path = %path.display(), "the export finished");
            }
            Poll::Event(Err(detail)) => {
                grana_log::error!(target: "grana::session", export = %self.label, %detail, "the export failed");
            }
            Poll::Empty | Poll::Closed => {}
        }
        outcome
    }
}

impl Session {
    pub(super) fn export_provenance(&mut self) {
        if self.tasks.writing() {
            self.message = Some(Message::error("an export is already running"));
            return;
        }
        if self.scene.is_none() {
            self.message = Some(Message::info(
                "open a dataset before exporting its provenance",
            ));
            return;
        }
        self.ask(
            Inquiry::folder("Export the provenance crate into a folder"),
            |session, path| {
                if let Some(root) = path {
                    session.write_provenance(root);
                }
            },
        );
    }

    fn write_provenance(&mut self, root: PathBuf) {
        if self.protocol.is_none() {
            self.capture_protocol();
        }
        let Some(protocol) = self.protocol.clone() else {
            self.message = Some(Message::error("the analysis carries no protocol"));
            return;
        };
        let Some(scene) = self.scene.as_ref() else {
            return;
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
            sets.push(provenance::Set {
                key: crate::slug(set.key().as_str()),
                label: set.name().to_string(),
                trace: Trace::of(set, self.fingerprint),
                table,
            });
        }
        let name = scene.dataset.name().to_string();
        let source = self.source.as_ref().and_then(|loader| {
            let source = loader.source();
            source.path().map(Path::to_path_buf)
        });
        self.message = None;
        self.tasks.provenance = Some(provenance::Export::start(provenance::Request {
            root,
            name,
            fingerprint: self.fingerprint,
            source,
            protocol,
            sets,
        }));
    }

    pub(super) fn finish_provenance(&mut self, outcome: Result<PathBuf, String>) {
        self.message = Some(match outcome {
            Ok(path) => Message::info(format!("the provenance crate is at {}", path.display())),
            Err(detail) => Message::error(detail),
        });
    }

    pub(super) fn export_coloc(&mut self) {
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let Some(result) = scene.coloc.result() else {
            return;
        };
        let pair = result.pair();
        let file_name = format!(
            "{}-{}-{}-colocalization.csv",
            crate::slug(scene.dataset.name()),
            crate::slug(scene.dataset.channels()[pair.a()].name()),
            crate::slug(scene.dataset.channels()[pair.b()].name())
        );
        self.ask(
            Inquiry::save("Export colocalization statistics", file_name).filter("CSV", &["csv"]),
            |session, path| {
                if let Some(path) = path {
                    session.write_coloc(path);
                }
            },
        );
    }

    fn write_coloc(&mut self, path: PathBuf) {
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let Some(result) = scene.coloc.result() else {
            return;
        };
        let csv = result.series().csv(result.thresholds());
        match grana_file::write(&path, csv.as_bytes()) {
            Ok(()) => self.message = None,
            Err(error) => {
                self.message = Some(Message::error(format!(
                    "failed to export colocalization to {}: {error}",
                    path.display()
                )));
            }
        }
    }

    pub(super) fn export_measure(&mut self) {
        if self.measure.view() == grana_measure::MeasureView::Ripley {
            self.export_ripley();
            return;
        }
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let Some(set) = self.measure.set() else {
            self.message = Some(Message::info(
                "choose an object set before exporting statistics",
            ));
            return;
        };
        let Some(object) = scene.objects.get(set) else {
            return;
        };
        let channel = scene.dataset.channels()[object.channel()].name();
        let name = object.name();
        let group = match self.measure.group() {
            Group::Object => "objects",
            Group::Track => "tracks",
            Group::Timepoint => "timepoints",
            Group::Molecule => "molecules",
        };
        let file_name = format!(
            "{}-{}-{}-{group}.csv",
            crate::slug(scene.dataset.name()),
            crate::slug(channel),
            crate::slug(name)
        );
        if self.tasks.pending_csv() {
            self.message = Some(Message::info(
                "the statistics are being measured before they are exported",
            ));
            return;
        }
        let title = format!("Export {name} statistics");
        self.ask(
            Inquiry::save(title, file_name).filter("CSV", &["csv"]),
            |session, path| session.write_measure(path),
        );
    }

    fn write_measure(&mut self, path: Option<PathBuf>) {
        let Some(path) = path else {
            return;
        };
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let Some(set) = self.measure.set() else {
            return;
        };
        let Some(object) = scene.objects.get(set) else {
            return;
        };
        let name = object.name().to_string();
        if self.tasks.pending_csv() {
            self.message = Some(Message::info(
                "the statistics are being measured before they are exported",
            ));
            return;
        }
        self.message = None;
        if self.measure.export(scene).is_some() {
            self.start_csv_export(path, name);
            return;
        }
        let Some(request) =
            TableRequest::new(scene, set, self.measure.group(), self.measure.aggregate())
        else {
            self.message = Some(Message::error(
                "the statistics of the chosen object set hold no table to export",
            ));
            return;
        };
        self.tasks.defer_csv(path, request.key(), name);
    }

    fn export_ripley(&mut self) {
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let Some(set) = self.measure.set() else {
            self.message = Some(Message::info(
                "choose an object set before exporting its Ripley curve",
            ));
            return;
        };
        let Some(object) = scene.objects.get(set) else {
            return;
        };
        let Some(grana_measure::Plot::Ripley(_)) = self.measure.plot() else {
            self.message = Some(Message::info(
                "the Ripley curve is still being computed before it is exported",
            ));
            return;
        };
        let file_name = format!(
            "{}-{}-ripley.csv",
            crate::slug(scene.dataset.name()),
            crate::slug(object.key().as_str())
        );
        self.ask(
            Inquiry::save("Export the Ripley curve", file_name).filter("CSV", &["csv"]),
            |session, path| {
                if let Some(path) = path {
                    session.write_ripley(path);
                }
            },
        );
    }

    fn write_ripley(&mut self, path: PathBuf) {
        let Some(grana_measure::Plot::Ripley(ripley)) = self.measure.plot() else {
            return;
        };
        let text = ripley.table();
        self.message = match grana_file::write(&path, text.as_bytes()) {
            Ok(()) => None,
            Err(error) => Some(Message::error(format!(
                "failed to export the Ripley curve to {}: {error}",
                path.display()
            ))),
        };
    }

    pub(super) fn finish_plot(
        &mut self,
        key: grana_scene::PlotKey,
        plot: Option<grana_measure::Plot>,
    ) {
        self.measure.install_plot(key, plot);
    }

    pub(super) fn finish_measure(&mut self, key: MeasureKey, table: Option<Table>) {
        self.measure.install(key, table.map(std::sync::Arc::new));
        let Some((path, deferred, name)) = self.tasks.take_csv() else {
            return;
        };
        if deferred != key {
            self.message = Some(Message::error(format!(
                "the statistics of {name} changed before they were measured, so {} was not written",
                path.display()
            )));
            return;
        }
        self.start_csv_export(path, name);
    }

    pub(super) fn finish_rows(&mut self, rows: grana_scene::Rows) {
        self.measure.install_rows(rows);
    }

    pub(super) fn finish_csv(&mut self, name: String, outcome: Result<PathBuf, String>) {
        self.message = Some(match outcome {
            Ok(path) => Message::info(format!("wrote {name} statistics to {}", path.display())),
            Err(detail) => Message::error(detail),
        });
    }

    fn start_csv_export(&mut self, path: PathBuf, name: String) {
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let Some((table, rows)) = self.measure.export(scene) else {
            self.message = Some(Message::error(format!(
                "the statistics of {name} were not measured, so {} was not written",
                path.display()
            )));
            return;
        };
        self.message = None;
        let label = format!("Exporting {name} statistics");
        self.tasks.csv = Some((
            Writer::start(label, path, move |_, progress, path| {
                write(path, &table, &rows, progress).map(|_| ())
            }),
            name,
        ));
    }

    pub(super) fn export_relation(&mut self, source: ObjectSetId, target: ObjectSetId, shift: i32) {
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let Some(set) = scene.objects.get(source) else {
            return;
        };
        let Some(related) = scene.objects.get(target) else {
            return;
        };
        let Some(relation) = set.relation(target, shift) else {
            return;
        };
        if relation.computed() == 0 {
            self.message = Some(Message::info(
                "measure the relation before exporting its pairs",
            ));
            return;
        }
        let file_name =
            grana_measure::PairExport::file_name(set.key().as_str(), related.key().as_str(), shift);
        let title = format!("Export {} pairs", set.name());
        self.ask(
            Inquiry::save(title, file_name).filter("CSV", &["csv"]),
            move |session, path| {
                if let Some(path) = path {
                    session.write_relation(source, target, shift, path);
                }
            },
        );
    }

    fn write_relation(
        &mut self,
        source: ObjectSetId,
        target: ObjectSetId,
        shift: i32,
        path: PathBuf,
    ) {
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let Some(set) = scene.objects.get(source) else {
            return;
        };
        let Some(related) = scene.objects.get(target) else {
            return;
        };
        let Some(relation) = set.relation(target, shift) else {
            return;
        };
        if relation.computed() == 0 {
            self.message = Some(Message::info(
                "measure the relation before exporting its pairs",
            ));
            return;
        }
        let mut export = grana_measure::PairExport::new(
            scene.dataset.frame_interval(),
            set.key().as_str(),
            related.key().as_str(),
        );
        for (time, frame) in relation.frames() {
            export.push(time, frame);
        }
        match grana_file::write(&path, export.text().as_bytes()) {
            Ok(()) => self.message = None,
            Err(error) => {
                self.message = Some(Message::error(format!(
                    "failed to export relation pairs to {}: {error}",
                    path.display()
                )));
            }
        }
    }

    pub(super) fn export_mesh(&mut self, set: ObjectSetId, format: MeshFormat) {
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let Some((target, title)) = mesh_target(scene, set, format) else {
            return;
        };
        match format {
            MeshFormat::Stl => self.ask(Inquiry::folder(title), move |session, path| {
                if let Some(folder) = path {
                    session.write_mesh(set, format, folder.join(target));
                }
            }),
            format => self.ask(
                Inquiry::save(title, target).filter(format.label(), &[format.extension()]),
                move |session, path| {
                    if let Some(path) = path {
                        session.write_mesh(set, format, path);
                    }
                },
            ),
        }
    }

    fn write_mesh(&mut self, set: ObjectSetId, format: MeshFormat, target: PathBuf) {
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let Some(object) = scene.objects.get(set) else {
            return;
        };
        let name = object.name().to_string();
        let time = scene.time();
        self.message = match grana_pipeline::mesh::write(scene, set, time, format, &target) {
            Ok(grana_pipeline::mesh::Export::Written) => Some(Message::info(format!(
                "wrote the {} mesh to {}",
                format.label(),
                target.display()
            ))),
            Ok(grana_pipeline::mesh::Export::Empty) => Some(Message::error(format!(
                "{name} holds no mesh at frame {time}"
            ))),
            Err(detail) => Some(Message::error(detail)),
        };
    }
}

fn mesh_target(scene: &Scene, set: ObjectSetId, format: MeshFormat) -> Option<(String, String)> {
    let object = scene.objects.get(set)?;
    let name = object.name().to_string();
    let key = object.key().as_str().to_string();
    let time = scene.time();
    let stem = format!(
        "{}-{}-t{time:04}",
        crate::slug(scene.dataset.name()),
        crate::slug(&key)
    );
    let title = format!("Export {name} mesh");
    Some(match format {
        MeshFormat::Stl => {
            let title = format!("{title} into a folder named {stem}");
            (stem, title)
        }
        format => (format!("{stem}.{}", format.extension()), title),
    })
}
