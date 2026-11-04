use std::path::{Path, PathBuf};

use swash::FontRef;

pub(crate) const FACE_CANDIDATES: &[(&str, &[&str])] = &[
    (
        "windows",
        &[
            "C:/Windows/Fonts/SegUIVar.ttf",
            "C:/Windows/Fonts/segoeui.ttf",
        ],
    ),
    (
        "macos",
        &[
            "/System/Library/Fonts/SFNS.ttf",
            "/System/Library/Fonts/SFNSDisplay.ttf",
            "/System/Library/Fonts/Helvetica.ttc",
        ],
    ),
    (
        "linux",
        &[
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
            "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
        ],
    ),
];

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

pub(crate) fn primary_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(path) = std::env::var("GRANA_UI_FONT") {
        paths.push(PathBuf::from(path));
    }
    for (platform, candidates) in FACE_CANDIDATES {
        if std::env::consts::OS == *platform {
            paths.extend(candidates.iter().map(PathBuf::from));
        }
    }
    paths
}

pub(crate) fn fallback_paths() -> Vec<PathBuf> {
    FALLBACK_CANDIDATES.iter().map(PathBuf::from).collect()
}
