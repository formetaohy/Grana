use super::*;

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

    pub(super) fn export_mesh(&mut self, set: ObjectSetId) {
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let Some(object) = scene.objects.get(set) else {
            return;
        };
        let time = scene.time();
        let geometry = match object.frame(time).map(ObjectFrame::geometry) {
            Some(Ok(Some(geometry))) => geometry,
            Some(Ok(None)) => {
                self.message = Some(Message::error(format!(
                    "{} holds no mesh at frame {time}",
                    object.name()
                )));
                return;
            }
            Some(Err(error)) => {
                self.message = Some(Message::error(error.to_string()));
                return;
            }
            None => {
                self.message = Some(Message::error(format!(
                    "{} holds no frame {time}",
                    object.name()
                )));
                return;
            }
        };
        let file_name = format!(
            "{}-{}-t{:04}.obj",
            crate::slug(scene.dataset.name()),
            object.key().as_str(),
            time
        );
        let title = format!("Export {} mesh", object.name());
        let Some(path) = rfd::FileDialog::new()
            .set_title(title)
            .set_file_name(file_name)
            .add_filter("Wavefront OBJ", &["obj"])
            .save_file()
        else {
            return;
        };
        let written = std::fs::File::create(&path)
            .map_err(grana_obj::Error::from)
            .and_then(|file| {
                grana_obj::write(
                    std::io::BufWriter::new(file),
                    &geometry,
                    object.key().as_str(),
                )
            });
        match written {
            Ok(()) => self.message = None,
            Err(error) => {
                self.message = Some(Message::error(format!(
                    "failed to export {} to {}: {error}",
                    object.name(),
                    path.display()
                )));
            }
        }
    }
}
