#[derive(Clone, Debug, PartialEq)]
pub struct LabelExport {
    path: String,
    frames: u32,
    labels: u32,
}

impl LabelExport {
    pub fn new(path: impl Into<String>, frames: u32, labels: u32) -> Self {
        let path = path.into();
        assert!(
            !path.trim().is_empty(),
            "a label export names the store it wrote"
        );
        assert!(
            frames > 0 && labels > 0,
            "a label export writes at least one label over one frame"
        );
        Self {
            path,
            frames,
            labels,
        }
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn frames(&self) -> u32 {
        self.frames
    }

    pub fn labels(&self) -> u32 {
        self.labels
    }
}
