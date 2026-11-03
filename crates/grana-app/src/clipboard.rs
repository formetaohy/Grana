use grana_ui::Clipboard;

pub struct SystemClipboard {
    clipboard: arboard::Clipboard,
}

impl SystemClipboard {
    pub fn new() -> Self {
        Self {
            clipboard: arboard::Clipboard::new().expect("the system clipboard is available"),
        }
    }
}

impl Clipboard for SystemClipboard {
    fn read(&mut self) -> Option<String> {
        self.clipboard.get_text().ok()
    }

    fn write(&mut self, text: &str) {
        if let Err(detail) = self.clipboard.set_text(text.to_string()) {
            eprintln!("grana: failed to copy text: {detail}");
        }
    }
}
