use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Zarr,
    Tiff,
}

impl Format {
    pub fn of(path: &Path) -> Option<Self> {
        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
        match extension.as_str() {
            "zarr" => Some(Self::Zarr),
            "tif" | "tiff" => Some(Self::Tiff),
            _ => None,
        }
    }
}
