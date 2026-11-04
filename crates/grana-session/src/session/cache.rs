use super::*;

impl Session {
    pub(super) fn clear_cache(&mut self) {
        match grana_image::clear_cache() {
            Ok(bytes) => {
                self.message = Some(Message::info(format!(
                    "cleared {:.1} MiB of derived pyramid tiles",
                    bytes as f64 / (1024.0 * 1024.0)
                )));
            }
            Err(error) => {
                self.message = Some(Message::error(format!(
                    "failed to clear the derived pyramid cache: {error}"
                )));
            }
        }
    }
}
