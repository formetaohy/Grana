use grana_editor::SettingsRequest;

use super::App;

impl App {
    pub(super) fn poll_storage(&mut self) {
        self.storage.poll();
        self.sync_storage();
        if let Some(detail) = self.storage.take_error() {
            grana_log::warn!(target: "grana::settings", %detail, "the derived cache could not be cleared");
            self.output = Some(format!("the derived cache could not be cleared: {detail}"));
        }
    }

    pub(super) fn sync_storage(&mut self) {
        let view = &mut self.workspace.storage;
        view.cache = self.storage.cache();
        view.freed = self.storage.freed();
        view.clearing = self.storage.clearing();
        view.measuring = self.storage.measuring();
    }

    pub(super) fn consume_settings(&mut self) {
        let Some(request) = self.workspace.settings.request.take() else {
            return;
        };
        match request {
            SettingsRequest::RevealData => {
                if let Err(detail) = self.storage.reveal_data() {
                    grana_log::warn!(target: "grana::settings", %detail, "the data folder could not be opened");
                    self.output = Some(detail);
                }
            }
            SettingsRequest::ClearCache => {
                self.storage.clear();
                self.sync_storage();
            }
            SettingsRequest::OpenRepository => {
                if let Err(error) = grana_update::open(env!("CARGO_PKG_REPOSITORY")) {
                    grana_log::warn!(target: "grana::settings", %error, "the project page could not be opened");
                    self.output = Some(format!("the project page could not be reached: {error}"));
                }
            }
        }
        self.request_redraw();
    }
}
