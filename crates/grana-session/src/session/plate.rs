use std::path::PathBuf;

use super::*;

use crate::task::Poll;

impl Session {
    pub(super) fn start_previews(&mut self) {
        if self.previews.is_some() {
            return;
        }
        let (Some(plate), Some(source)) = (
            self.plate.as_ref(),
            self.source.as_ref().map(Loader::source),
        ) else {
            return;
        };
        self.previews = Some(preview::Previews::start(plate, &source));
    }

    pub(super) fn clear_plate(&mut self) {
        self.previews = None;
        self.montage = None;
    }

    pub(super) fn poll_plate(&mut self) -> bool {
        let mut repaint = self.previews.as_mut().is_some_and(preview::Previews::poll);
        let Some(mut montage) = self.montage.take() else {
            return repaint;
        };
        loop {
            match montage.poll() {
                Poll::Event(montage::Event::Progress(_, _)) => repaint = true,
                Poll::Event(montage::Event::Done(outcome)) => {
                    self.message = Some(match outcome {
                        Ok(path) => {
                            Message::info(format!("wrote the plate montage to {}", path.display()))
                        }
                        Err(detail) => Message::error(detail),
                    });
                    repaint = true;
                    return repaint;
                }
                Poll::Empty => {
                    self.montage = Some(montage);
                    return repaint;
                }
                Poll::Closed => {
                    self.message = Some(Message::error(
                        "the plate montage stopped before the image was written",
                    ));
                    return true;
                }
            }
        }
    }

    pub(super) fn export_plate(&mut self) {
        let Some(plate) = self.plate.as_ref() else {
            self.message = Some(Message::error("no plate is open"));
            return;
        };
        let name = format!("{}-plate.png", crate::slug(plate.name()));
        let Some(path) = rfd::FileDialog::new()
            .set_title("Export the plate montage")
            .set_file_name(name)
            .add_filter("PNG", &["png"])
            .save_file()
        else {
            return;
        };
        self.export_plate_montage(path);
    }

    pub fn export_plate_montage(&mut self, path: PathBuf) -> bool {
        let Some(plate) = self.plate.as_ref() else {
            self.message = Some(Message::error("no plate is open"));
            return false;
        };
        if self.montage.is_some() {
            self.message = Some(Message::error("a plate montage is already running"));
            return false;
        }
        let Some(source) = self.source.as_ref().map(Loader::source) else {
            self.message = Some(Message::error("the plate has no source to read"));
            return false;
        };
        self.message = None;
        self.montage = Some(montage::Montage::start(plate, &source, path));
        true
    }
}
