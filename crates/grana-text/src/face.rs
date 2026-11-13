use std::path::{Path, PathBuf};

use swash::FontRef;

const ROBOTO_FLEX: &[u8] = include_bytes!("../assets/roboto-flex.ttf");
const ROBOTO_MONO: &[u8] = include_bytes!("../assets/roboto-mono.ttf");

pub(crate) const FALLBACK_CANDIDATES: &[&str] = &[
    "C:/Windows/Fonts/seguisym.ttf",
    "C:/Windows/Fonts/msyh.ttc",
    "C:/Windows/Fonts/msjh.ttc",
    "C:/Windows/Fonts/simsun.ttc",
    "/System/Library/Fonts/PingFang.ttc",
    "/System/Library/Fonts/Hiragino Sans GB.ttc",
    "/System/Library/Fonts/STHeiti Light.ttc",
    "/System/Library/Fonts/Apple Symbols.ttf",
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
];

pub(crate) struct Face {
    pub(crate) font: FontRef<'static>,
}

impl Face {
    pub(crate) fn embedded(data: &'static [u8]) -> Self {
        let font =
            FontRef::from_index(data, 0).expect("embedded Material 3 Expressive font is valid");
        Self { font }
    }

    pub(crate) fn load(path: &Path) -> Option<Self> {
        let data = std::fs::read(path).ok()?;
        let leaked: &'static [u8] = Box::leak(data.into_boxed_slice());
        let font = FontRef::from_index(leaked, 0)?;
        Some(Self { font })
    }

    pub(crate) fn covers(&self, character: char) -> bool {
        self.font.charmap().map(character) != 0
    }
}

pub(crate) fn primary_faces() -> Vec<(Face, String)> {
    vec![
        (Face::embedded(ROBOTO_FLEX), "Roboto Flex".to_string()),
        (Face::embedded(ROBOTO_MONO), "Roboto Mono".to_string()),
    ]
}

pub(crate) fn fallback_paths() -> Vec<PathBuf> {
    FALLBACK_CANDIDATES.iter().map(PathBuf::from).collect()
}
