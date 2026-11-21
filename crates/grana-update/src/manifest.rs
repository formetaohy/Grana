use semver::Version;
use serde::Deserialize;

use crate::Error;
use crate::source::Source;

const HASH: usize = 64;
const HASH_BYTES: usize = HASH / 2;
const SIZE_LIMIT: u64 = 512 << 20;

#[derive(Clone, Debug)]
pub struct Offer {
    pub version: Version,
    pub notes: String,
    pub portable: Artifact,
    pub installer: Artifact,
}

#[derive(Clone, Debug)]
pub struct Artifact {
    pub url: String,
    pub sha256: [u8; HASH_BYTES],
    pub size: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Stored {
    version: String,
    notes: String,
    portable: Entry,
    installer: Entry,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    url: String,
    sha256: String,
    size: u64,
}

impl Offer {
    pub fn read(text: &str, source: &Source) -> Result<Self, Error> {
        let stored = toml::from_str::<Stored>(text).map_err(|error| Error::Offer {
            detail: error.to_string(),
        })?;
        let version = Version::parse(&stored.version).map_err(|error| Error::Offer {
            detail: format!("`{}` is not a version: {error}", stored.version),
        })?;
        if !source.admits(&stored.notes) {
            return Err(Error::Offer {
                detail: format!(
                    "`{}` is not published by `{}`",
                    stored.notes,
                    source.manifest()
                ),
            });
        }
        Ok(Self {
            version,
            notes: stored.notes,
            portable: Artifact::read(stored.portable, source)?,
            installer: Artifact::read(stored.installer, source)?,
        })
    }
}

impl Artifact {
    fn read(entry: Entry, source: &Source) -> Result<Self, Error> {
        if !source.admits(&entry.url) {
            return Err(Error::Offer {
                detail: format!(
                    "`{}` is not published by `{}`",
                    entry.url,
                    source.manifest()
                ),
            });
        }
        if entry.size == 0 || entry.size > SIZE_LIMIT {
            return Err(Error::Offer {
                detail: format!(
                    "the size {size} of `{url}` is out of range",
                    size = entry.size,
                    url = entry.url
                ),
            });
        }
        Ok(Self {
            url: entry.url,
            sha256: digest(&entry.sha256)?,
            size: entry.size,
        })
    }
}

fn digest(text: &str) -> Result<[u8; HASH_BYTES], Error> {
    if text.len() != HASH {
        return Err(Error::Offer {
            detail: format!("`{text}` is not a sha256 digest"),
        });
    }
    let mut digest = [0u8; HASH_BYTES];
    for (index, byte) in digest.iter_mut().enumerate() {
        let pair = &text[index * 2..index * 2 + 2];
        *byte = u8::from_str_radix(pair, 16).map_err(|_| Error::Offer {
            detail: format!("`{text}` is not a sha256 digest"),
        })?;
    }
    Ok(digest)
}
