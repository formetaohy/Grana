use super::*;

use grana_interop::Crate;
use grana_measure::Aggregate;
use grana_mesh::Format as MeshFormat;
use grana_scene::Trace;

impl Session {
    pub(super) fn export_provenance(&mut self) {
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
        self.message = match self.write_crate(&protocol, &root) {
            Ok(path) => Some(Message::info(format!(
                "the provenance crate is at {}",
                path.display()
            ))),
            Err(detail) => Some(Message::error(detail)),
        };
    }

    fn write_crate(&self, protocol: &Protocol, root: &Path) -> Result<PathBuf, String> {
        let scene = self
            .scene
            .as_ref()
            .ok_or_else(|| "the crate needs an open dataset".to_string())?;
        let protocol_path = root.join("protocol.json");
        protocol
            .write(&protocol_path)
            .map_err(|error| error.to_string())?;
        let mut record = Crate::new(
            scene.dataset.name(),
            "The analysis of a microscopy dataset",
            "Grana",
            env!("CARGO_PKG_VERSION"),
        );
        if let Some(fingerprint) = self.fingerprint {
            record.fingerprint(&fingerprint.hex(), grana_image::FINGERPRINT_COVERAGE);
        }
        if let Some(path) = self.source.as_ref().and_then(|loader| {
            let source = loader.source();
            source.path().map(Path::to_path_buf)
        }) {
            record.source(&path);
        }
        record.part(
            "protocol.json",
            "protocol.json",
            "application/json",
            "the protocol that reproduces the analysis",
        );
        for set in scene.objects.iter() {
            if set.provenance().is_attached() {
                continue;
            }
            let Some(table) = scene.table(set.id(), Group::Object, Aggregate::MEAN) else {
                continue;
            };
            let rows = (0..table.rows()).collect::<Vec<_>>();
            let file = format!(
                "measurements/{}-objects.csv",
                crate::slug(set.key().as_str())
            );
            let path = root.join(&file);
            if let Some(folder) = path.parent() {
                std::fs::create_dir_all(folder)
                    .map_err(|error| format!("failed to create {}: {error}", folder.display()))?;
            }
            std::fs::write(&path, table.csv(&rows))
                .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
            let trace = Trace::of(set, self.fingerprint);
            record.part(
                &file,
                &format!("{}-objects.csv", crate::slug(set.key().as_str())),
                "text/csv",
                &format!("the objects of {} at every frame", set.name()),
            );
            record.action(
                &format!("#create-{}", crate::slug(set.key().as_str())),
                &format!("{} of {}", trace.algorithm().title(), set.name()),
                &format!(
                    "{} v{} over {} objects",
                    trace.algorithm().title(),
                    trace.version(),
                    trace.objects()
                ),
                &[file, "protocol.json".to_string()],
            );
        }
        record.write(root).map_err(|error| error.to_string())
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
            return;
        };
        let Some(object) = scene.objects.get(set) else {
            return;
        };
        let Some(csv) = self.measure.csv() else {
            return;
        };
        let channel = scene.dataset.channels()[object.channel()].name();
        let group = match self.measure.group() {
            Group::Object => "objects",
            Group::Track => "tracks",
            Group::Timepoint => "timepoints",
        };
        let file_name = format!(
            "{}-{}-{}-{group}.csv",
            crate::slug(scene.dataset.name()),
            crate::slug(channel),
            crate::slug(object.name())
        );
        let title = format!("Export {} statistics", object.name());
        let Some(path) = rfd::FileDialog::new()
            .set_title(title)
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
                    "failed to export statistics to {}: {error}",
                    path.display()
                )));
            }
        }
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
