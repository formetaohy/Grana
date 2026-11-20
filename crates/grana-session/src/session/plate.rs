use std::path::PathBuf;

use super::*;

impl Session {
    pub(super) fn start_previews(&mut self) {
        if self.tasks.previews().is_some() {
            return;
        }
        let (Some(plate), Some(source)) = (
            self.plate.as_ref(),
            self.source.as_ref().map(Loader::source),
        ) else {
            return;
        };
        self.tasks
            .start_previews(preview::Previews::start(plate, &source));
    }

    pub(super) fn clear_plate(&mut self) {
        self.tasks.clear_previews();
        self.tasks.montage = None;
    }

    pub(super) fn finish_montage(&mut self, outcome: Result<PathBuf, String>) {
        self.message = Some(match outcome {
            Ok(path) => Message::info(format!("wrote the plate montage to {}", path.display())),
            Err(detail) => Message::error(detail),
        });
    }

    pub(super) fn export_plate(&mut self) {
        let Some(plate) = self.plate.as_ref() else {
            self.message = Some(Message::error("no plate is open"));
            return;
        };
        let name = format!("{}-plate.png", crate::slug(plate.name()));
        self.ask(
            Inquiry::save("Export the plate montage", name).filter("PNG", &["png"]),
            |session, path| {
                if let Some(path) = path {
                    session.export_plate_montage(path);
                }
            },
        );
    }

    pub fn export_plate_montage(&mut self, path: PathBuf) -> bool {
        let Some(plate) = self.plate.as_ref() else {
            self.message = Some(Message::error("no plate is open"));
            return false;
        };
        if self.tasks.montage.is_some() {
            self.message = Some(Message::error("a plate montage is already running"));
            return false;
        }
        let Some(source) = self.source.as_ref().map(Loader::source) else {
            self.message = Some(Message::error("the plate has no source to read"));
            return false;
        };
        self.message = None;
        self.tasks.montage = Some(montage::Montage::start(plate, &source, path));
        true
    }
}
