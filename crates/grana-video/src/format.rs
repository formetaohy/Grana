use std::ffi::OsStr;
use std::path::Path;

use crate::error::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    H264,
    Png,
}

impl Format {
    pub const fn of(transparent: bool) -> Self {
        if transparent { Self::Png } else { Self::H264 }
    }

    pub const fn extension(self) -> &'static str {
        match self {
            Self::H264 => "mp4",
            Self::Png => "mov",
        }
    }

    pub const fn transparent(self) -> bool {
        match self {
            Self::H264 => false,
            Self::Png => true,
        }
    }

    pub fn planned_bytes(self, size: [u32; 2], frames: usize) -> u64 {
        let pixels = u64::from(size[0]) * u64::from(size[1]);
        let sample = match self {
            Self::H264 => pixels * 3 / 2,
            Self::Png => pixels * 4,
        };
        sample.saturating_mul(frames as u64)
    }

    pub fn required(path: &Path, transparent: bool) -> Result<Self, Error> {
        let name = path
            .file_name()
            .and_then(OsStr::to_str)
            .unwrap_or("the export")
            .to_string();
        let extension = path.extension().and_then(OsStr::to_str);
        let named = [Self::H264, Self::Png].into_iter().find(|format| {
            extension.is_some_and(|extension| extension.eq_ignore_ascii_case(format.extension()))
        });
        let Some(named) = named else {
            return Err(Error::Extension { name });
        };
        if named.transparent() != transparent {
            return Err(if transparent {
                Error::Transparent { name }
            } else {
                Error::Opaque { name }
            });
        }
        Ok(named)
    }
}
