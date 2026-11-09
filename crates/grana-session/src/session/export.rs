use super::*;

use grana_mesh::{Format as MeshFormat, Material};

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
        let Some(object) = scene.objects.get(set) else {
            return;
        };
        let name = object.name().to_string();
        let key = object.key().as_str().to_string();
        let time = scene.time();
        let geometry = match object.frame(time).map(ObjectFrame::geometry) {
            Some(Ok(Some(geometry))) => geometry,
            Some(Ok(None)) => {
                self.message = Some(Message::error(format!(
                    "{name} holds no mesh at frame {time}"
                )));
                return;
            }
            Some(Err(error)) => {
                self.message = Some(Message::error(error.to_string()));
                return;
            }
            None => {
                self.message = Some(Message::error(format!("{name} holds no frame {time}")));
                return;
            }
        };
        let stem = format!(
            "{}-{}-t{time:04}",
            crate::slug(scene.dataset.name()),
            crate::slug(&key)
        );
        let title = format!("Export {name} mesh");
        match format {
            MeshFormat::Obj => {
                let Some(path) = rfd::FileDialog::new()
                    .set_title(&title)
                    .set_file_name(format!("{stem}.obj"))
                    .add_filter(format.label(), &[format.extension()])
                    .save_file()
                else {
                    return;
                };
                let written = std::fs::File::create(&path)
                    .map_err(grana_mesh::Error::from)
                    .and_then(|file| {
                        grana_mesh::write_obj(std::io::BufWriter::new(file), &geometry, &key)
                    });
                self.report_mesh(written, &path, &name);
            }
            MeshFormat::Glb => {
                let Some(path) = rfd::FileDialog::new()
                    .set_title(&title)
                    .set_file_name(format!("{stem}.glb"))
                    .add_filter(format.label(), &[format.extension()])
                    .save_file()
                else {
                    return;
                };
                let written = std::fs::File::create(&path)
                    .map_err(grana_mesh::Error::from)
                    .and_then(|file| {
                        let materials = materials(scene, object, geometry.object_count());
                        grana_mesh::write_glb(
                            std::io::BufWriter::new(file),
                            &geometry,
                            &key,
                            &materials,
                        )
                    });
                self.report_mesh(written, &path, &name);
            }
            MeshFormat::Stl => {
                let Some(directory) = rfd::FileDialog::new()
                    .set_title(format!("{title} into a folder"))
                    .pick_folder()
                else {
                    return;
                };
                let mut failure = None;
                for index in 0..geometry.object_count() {
                    let path = directory.join(format!("{stem}-{index:06}.stl"));
                    let written = std::fs::File::create(&path)
                        .map_err(grana_mesh::Error::from)
                        .and_then(|file| {
                            grana_mesh::write_stl(std::io::BufWriter::new(file), &geometry, index)
                        });
                    if let Err(error) = written {
                        failure = Some((path, error));
                        break;
                    }
                }
                match failure {
                    None => self.message = None,
                    Some((path, error)) => {
                        self.message = Some(Message::error(format!(
                            "failed to export {name} to {}: {error}",
                            path.display()
                        )));
                    }
                }
            }
        }
    }

    fn report_mesh(&mut self, written: Result<(), grana_mesh::Error>, path: &Path, name: &str) {
        match written {
            Ok(()) => self.message = None,
            Err(error) => {
                self.message = Some(Message::error(format!(
                    "failed to export {name} to {}: {error}",
                    path.display()
                )));
            }
        }
    }
}

fn materials(scene: &Scene, object: &grana_scene::ObjectSet, objects: usize) -> Vec<Material> {
    let colors = scene.object_colors(object);
    (0..objects)
        .map(|index| {
            let color = colors
                .as_ref()
                .and_then(|colors| colors.get(index).copied())
                .unwrap_or_else(|| object.color());
            Material::new(grana_display::srgb_to_linear(color), object.opacity())
        })
        .collect()
}
