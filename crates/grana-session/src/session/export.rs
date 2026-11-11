use std::path::PathBuf;
use std::sync::Arc;

use grana_measure::{Aggregate, Group, Table};
use grana_mesh::Format as MeshFormat;
use grana_scene::{MeasureKey, TableRequest, Trace};

use super::*;

use crate::task::Progress;

pub(super) struct Csv {
    task: Progress<Result<PathBuf, String>>,
    label: String,
}

impl Csv {
    fn start(table: Arc<Table>, rows: Vec<usize>, path: PathBuf, name: &str) -> Self {
        let label = format!("Exporting {name} statistics");
        let task = Progress::spawn(grana_exec::Role::Write, move |_, _| {
            std::fs::write(&path, table.csv(&rows)).map_err(|error| {
                format!("failed to export statistics to {}: {error}", path.display())
            })?;
            Ok(path)
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
        self.task.poll()
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
        if self.protocol.is_none() {
            self.capture_protocol();
        }
        let Some(protocol) = self.protocol.clone() else {
            self.message = Some(Message::error("the analysis carries no protocol"));
            return;
        };
        let Some(root) = rfd::FileDialog::new()
            .set_title("Export the provenance crate into a folder")
            .pick_folder()
        else {
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
        let csv = result.series().csv(result.thresholds());
        let file_name = format!(
            "{}-{}-{}-colocalization.csv",
            crate::slug(scene.dataset.name()),
            crate::slug(scene.dataset.channels()[pair.a()].name()),
            crate::slug(scene.dataset.channels()[pair.b()].name())
        );
        let Some(path) = rfd::FileDialog::new()
            .set_title("Export colocalization statistics")
            .set_file_name(file_name)
            .add_filter("CSV", &["csv"])
            .save_file()
        else {
            return;
        };
        match std::fs::write(&path, csv) {
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
        let Some(path) = rfd::FileDialog::new()
            .set_title(title)
            .set_file_name(file_name)
            .add_filter("CSV", &["csv"])
            .save_file()
        else {
            return;
        };
        self.message = None;
        if self.measure.export(scene).is_some() {
            self.start_csv_export(path);
            return;
        }
        if TableRequest::new(scene, set, self.measure.group(), self.measure.aggregate()).is_none() {
            self.message = Some(Message::error(
                "the statistics of the chosen object set hold no table to export",
            ));
            return;
        }
        self.tasks.defer_csv(path);
    }

    pub(super) fn finish_plot(
        &mut self,
        key: grana_scene::PlotKey,
        plot: Option<grana_measure::Plot>,
    ) {
        self.measure.install_plot(key, plot);
    }

    pub(super) fn finish_measure(&mut self, key: MeasureKey, table: Option<Table>) {
        self.measure.install(key, table);
        let Some(path) = self.tasks.take_csv() else {
            return;
        };
        self.start_csv_export(path);
    }

    pub(super) fn finish_csv(&mut self, outcome: Result<PathBuf, String>) {
        self.message = Some(match outcome {
            Ok(path) => Message::info(format!(
                "wrote {} statistics to {}",
                self.csv_name(),
                path.display()
            )),
            Err(detail) => Message::error(detail),
        });
    }

    fn csv_name(&self) -> String {
        self.scene
            .as_ref()
            .and_then(|scene| self.measure.set().and_then(|set| scene.objects.get(set)))
            .map_or_else(
                || "the object set".to_string(),
                |object| object.name().to_string(),
            )
    }

    fn start_csv_export(&mut self, path: PathBuf) {
        let name = self.csv_name();
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let Some((table, rows)) = self.measure.export(scene) else {
            self.message = Some(Message::error(
                "the statistics of the chosen object set hold no table to export",
            ));
            return;
        };
        self.message = None;
        self.tasks.csv = Some(Csv::start(table, rows, path, &name));
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
        let mut export = grana_measure::PairExport::new(
            scene.dataset.frame_interval(),
            set.key().as_str(),
            related.key().as_str(),
        );
        for (time, frame) in relation.frames() {
            export.push(time, frame);
        }
        let file_name =
            grana_measure::PairExport::file_name(set.key().as_str(), related.key().as_str(), shift);
        let title = format!("Export {} pairs", set.name());
        let Some(path) = rfd::FileDialog::new()
            .set_title(title)
            .set_file_name(file_name)
            .add_filter("CSV", &["csv"])
            .save_file()
        else {
            return;
        };
        match std::fs::write(&path, export.text()) {
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
        let Some(written) = mesh(scene, set, format) else {
            return;
        };
        self.message = match written {
            Ok(()) => None,
            Err(detail) => Some(Message::error(detail)),
        };
    }
}

fn mesh(scene: &Scene, set: ObjectSetId, format: MeshFormat) -> Option<Result<(), String>> {
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
    let target = match format {
        MeshFormat::Stl => rfd::FileDialog::new()
            .set_title(format!("{title} into a folder"))
            .pick_folder(),
        format => rfd::FileDialog::new()
            .set_title(&title)
            .set_file_name(format!("{stem}.{}", format.extension()))
            .add_filter(format.label(), &[format.extension()])
            .save_file(),
    }?;
    Some(
        match grana_pipeline::mesh::write(scene, set, time, format, &target) {
            Ok(grana_pipeline::mesh::Export::Written) => Ok(()),
            Ok(grana_pipeline::mesh::Export::Empty) => {
                Err(format!("{name} holds no mesh at frame {time}"))
            }
            Err(detail) => Err(detail),
        },
    )
}
