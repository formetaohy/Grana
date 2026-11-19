use super::App;

impl App {
    pub fn failed(&self) -> Option<&str> {
        self.fatal.as_deref()
    }

    pub(super) fn guard(&mut self) -> bool {
        if self.fatal.is_some() {
            return false;
        }
        match self.surface.as_ref().and_then(|surface| surface.lost()) {
            Some(detail) => {
                self.fail(detail);
                false
            }
            None => true,
        }
    }

    pub(super) fn fail(&mut self, detail: String) {
        if self.fatal.is_some() {
            return;
        }
        grana_log::error!(target: "grana::app", "{detail}");
        self.cancel_work();
        let kept = if self.session.access().author() {
            self.session.autosave_now(self.view_state())
        } else {
            Ok(())
        };
        let language = self.workspace.language;
        let note = match kept {
            Ok(()) => grana_i18n::app::device_lost_detail(language).to_string(),
            Err(error) => format!("{} {error}", grana_i18n::app::device_lost_detail(language)),
        };
        rfd::MessageDialog::new()
            .set_title(grana_i18n::app::device_lost(language))
            .set_description(format!("{detail}\n\n{note}"))
            .set_level(rfd::MessageLevel::Error)
            .set_buttons(rfd::MessageButtons::Ok)
            .show();
        eprintln!("{detail}");
        self.fatal = Some(detail);
        self.exiting = true;
    }

    fn cancel_work(&mut self) {
        self.workspace.playback.playing = false;
        self.workspace.playback.previewing = false;
        self.capture = None;
        self.pending_engine = None;
        self.export.cancel();
    }
}
