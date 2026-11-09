use super::*;

use grana_mesh::Format as MeshFormat;

impl Session {
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
