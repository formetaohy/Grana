use std::io::Write;
use std::path::Path;
use std::sync::atomic::AtomicBool;

use grana_update::{Offer, Source, StagedArtifact, Staging, Target};
use semver::Version;
use sha2::{Digest, Sha256};

use crate::support::{Server, Temp};

#[test]
fn updates_from_the_published_manifest() {
    let temp = Temp::new("grana-update");
    let files = temp.path().join("public");
    let release = files.join("releases").join("download").join("v9.9.9");
    std::fs::create_dir_all(&release).expect("the release folder is created");
    let archive = release.join("grana.zip");
    let payload = (0..64 * 1024)
        .map(|index| (index % 251) as u8)
        .collect::<Vec<u8>>();
    archive_of(&archive, "grana.exe", &payload);
    let setup = release.join("setup.exe");
    std::fs::write(&setup, b"installer").expect("the installer is written");
    let installer = std::fs::read(&setup).expect("the installer is readable");
    let bytes = std::fs::read(&archive).expect("the archive is readable");
    let server = Server::serve(&files);
    let origin = server.url().trim_end_matches('/').to_string();
    let manifest = format!(
        r#"version = "9.9.9"
notes = "{origin}/releases/tag/v9.9.9"

[portable]
url = "{origin}/releases/download/v9.9.9/grana.zip"
sha256 = "{portable_digest}"
size = {portable_size}

[installer]
url = "{origin}/releases/download/v9.9.9/setup.exe"
sha256 = "{installer_digest}"
size = {installer_size}
"#,
        portable_digest = digest(&bytes),
        portable_size = bytes.len(),
        installer_digest = digest(&installer),
        installer_size = installer.len(),
    );
    let latest = files.join("releases").join("latest").join("download");
    std::fs::create_dir_all(&latest).expect("the manifest folder is created");
    std::fs::write(latest.join("latest.toml"), &manifest).expect("the manifest is written");

    let source = Source::at(&origin).expect("the fixture names a source");
    let offer = grana_update::check(&source, &Version::new(0, 2, 2))
        .expect("the check reads the manifest")
        .expect("a newer release is offered");
    assert_eq!(offer.version, Version::new(9, 9, 9));
    assert_eq!(offer.portable.size, bytes.len() as u64);
    assert_eq!(offer.installer.size, installer.len() as u64);
    assert!(
        grana_update::check(&source, &offer.version)
            .expect("the check rereads the manifest")
            .is_none(),
        "a release is not offered to itself"
    );

    let staging = Staging::at(temp.path().join("staging"));
    let cancel = AtomicBool::new(false);
    let mut seen = (0, 0);
    let mut report = |done: u64, total: u64| seen = (done, total);
    let staged = staging
        .stage(&Target::Portable, &offer, &cancel, &mut report)
        .expect("the portable artifact is staged");
    let StagedArtifact::Portable(executable) = staged.artifact else {
        panic!("the portable target stages an executable");
    };
    assert_eq!(
        std::fs::read(&executable).expect("the executable is readable"),
        payload
    );
    assert_eq!(seen, (bytes.len() as u64, bytes.len() as u64));

    let foreign = manifest.replace(&origin, "https://elsewhere.example");
    assert!(
        Offer::read(&foreign, &source).is_err(),
        "a manifest that points at another origin is refused"
    );
    let clipped = manifest.replace(&digest(&bytes), &digest(&bytes)[..56]);
    assert!(
        Offer::read(&clipped, &source).is_err(),
        "a manifest with a clipped digest is refused"
    );
}

fn archive_of(path: &Path, name: &str, bytes: &[u8]) {
    let file = std::fs::File::create(path).expect("the archive is created");
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    zip.start_file(name, options).expect("the entry starts");
    zip.write_all(bytes).expect("the entry is written");
    zip.finish().expect("the archive is finished");
}

fn digest(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut text = String::with_capacity(digest.len() * 2);
    for byte in digest {
        text.push_str(&format!("{byte:02x}"));
    }
    text
}
