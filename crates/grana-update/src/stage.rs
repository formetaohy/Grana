use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use semver::Version;
use sha2::{Digest, Sha256};
use zip::ZipArchive;

use crate::Error;
use crate::manifest::{Artifact, Offer};
use crate::target::Target;

const ENTRY: &str = "grana.exe";
const BUFFER: usize = 1 << 20;

pub struct Staged {
    pub version: Version,
    pub artifact: StagedArtifact,
}

pub enum StagedArtifact {
    Portable(PathBuf),
    Installer(PathBuf),
}

impl StagedArtifact {
    pub fn path(&self) -> &Path {
        match self {
            Self::Portable(path) | Self::Installer(path) => path,
        }
    }
}

pub struct Staging {
    root: PathBuf,
}

impl Staging {
    pub fn system() -> Result<Self, Error> {
        let root = grana_dir::cache().ok_or_else(|| Error::Storage {
            path: "the cache".to_string(),
            detail: "the system assigns no cache directory".to_string(),
        })?;
        Ok(Self {
            root: root.join("update"),
        })
    }

    pub fn at(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn stage(
        &self,
        target: &Target,
        offer: &Offer,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(u64, u64),
    ) -> Result<Staged, Error> {
        let directory = self.directory(&offer.version)?;
        self.prune(&offer.version);
        let artifact = match target {
            Target::Portable => {
                let archive = directory.join("grana.zip");
                fetch(&offer.portable, &archive, cancel, progress)?;
                let executable = directory.join(ENTRY);
                extract(&archive, &executable)?;
                if let Err(error) = fs::remove_file(&archive) {
                    grana_log::warn!(target: "grana::update", %error, path = %archive.display(), "the archive was kept");
                }
                StagedArtifact::Portable(executable)
            }
            Target::Installed { .. } => {
                let setup =
                    directory.join(format!("Grana-{}-windows-x64-setup.exe", offer.version));
                fetch(&offer.installer, &setup, cancel, progress)?;
                StagedArtifact::Installer(setup)
            }
        };
        if let Ok(current) = std::env::current_exe() {
            crate::signature::admits(&current, artifact.path())?;
        }
        Ok(Staged {
            version: offer.version.clone(),
            artifact,
        })
    }

    fn directory(&self, version: &Version) -> Result<PathBuf, Error> {
        let directory = self.root.join(version.to_string());
        fs::create_dir_all(&directory).map_err(|error| Error::storage(&directory, error))?;
        Ok(directory)
    }

    fn prune(&self, keep: &Version) {
        let Ok(entries) = fs::read_dir(&self.root) else {
            return;
        };
        let keep = keep.to_string();
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy() == keep {
                continue;
            }
            let path = entry.path();
            let outcome = if path.is_dir() {
                fs::remove_dir_all(&path)
            } else {
                fs::remove_file(&path)
            };
            if let Err(error) = outcome {
                grana_log::warn!(target: "grana::update", %error, path = %path.display(), "the previous staging entry was kept");
            }
        }
    }
}

fn fetch(
    artifact: &Artifact,
    path: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<(), Error> {
    if path.exists() {
        fs::remove_file(path).map_err(|error| Error::storage(path, error))?;
    }
    let written = grana_net::Client::shared().download(&artifact.url, path, cancel, progress)?;
    if written != artifact.size || digest(path)? != artifact.sha256 {
        fs::remove_file(path).map_err(|error| Error::storage(path, error))?;
        return Err(Error::Corrupt {
            path: path.display().to_string(),
        });
    }
    Ok(())
}

fn digest(path: &Path) -> Result<[u8; 32], Error> {
    let mut file = File::open(path).map_err(|error| Error::storage(path, error))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; BUFFER];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| Error::storage(path, error))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().into())
}

fn extract(archive: &Path, executable: &Path) -> Result<(), Error> {
    let file = File::open(archive).map_err(|error| Error::storage(archive, error))?;
    let mut entries = ZipArchive::new(file).map_err(|error| Error::Corrupt {
        path: format!("`{}`: {error}", archive.display()),
    })?;
    let mut entry = entries.by_name(ENTRY).map_err(|error| Error::Corrupt {
        path: format!("`{}`: {error}", archive.display()),
    })?;
    let mut out = File::create(executable).map_err(|error| Error::storage(executable, error))?;
    io::copy(&mut entry, &mut out).map_err(|error| Error::storage(executable, error))?;
    out.flush()
        .map_err(|error| Error::storage(executable, error))?;
    Ok(())
}
