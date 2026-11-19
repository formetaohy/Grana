use std::ffi::OsString;
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub fn write(root: &Path) -> io::Result<PathBuf> {
    let name = folder_name()?;
    let executable = build(root)?;
    let folder = root.join("dist").join(name);
    if folder.exists() {
        std::fs::remove_dir_all(&folder)?;
    }
    std::fs::create_dir_all(&folder)?;
    std::fs::copy(&executable, folder.join(executable_name()))?;
    std::fs::copy(root.join("LICENSE"), folder.join("LICENSE"))?;
    Ok(folder)
}

fn build(root: &Path) -> io::Result<PathBuf> {
    let mut child = Command::new(cargo())
        .current_dir(root)
        .args([
            "build",
            "--package",
            "grana",
            "--bin",
            "grana",
            "--release",
            "--all-features",
            "--locked",
            "--message-format=json-render-diagnostics",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;
    let stream = child
        .stdout
        .take()
        .expect("the release build output is piped");
    let mut executable = None;
    for line in BufReader::new(stream).lines() {
        let message: serde_json::Value = serde_json::from_str(&line?).map_err(|error| {
            io::Error::other(format!(
                "the release build emitted malformed output: {error}"
            ))
        })?;
        match message["reason"].as_str() {
            Some("compiler-message") => {
                if let Some(rendered) = message["message"]["rendered"].as_str() {
                    eprint!("{rendered}");
                }
            }
            Some("compiler-artifact") if application_artifact(&message) => {
                if let Some(path) = message["executable"].as_str() {
                    executable = Some(PathBuf::from(path));
                }
            }
            _ => {}
        }
    }
    let status = child.wait()?;
    if !status.success() {
        return Err(io::Error::other(format!(
            "the release build failed: {status}"
        )));
    }
    executable.ok_or_else(|| io::Error::other("the release build produced no executable"))
}

fn application_artifact(message: &serde_json::Value) -> bool {
    message["target"]["name"].as_str() == Some("grana")
        && message["target"]["kind"]
            .as_array()
            .is_some_and(|kinds| kinds.iter().any(|kind| kind.as_str() == Some("bin")))
}

fn folder_name() -> io::Result<String> {
    if std::env::var_os("CARGO_BUILD_TARGET").is_some() {
        return Err(io::Error::other(
            "cargo dist builds the host target; unset CARGO_BUILD_TARGET",
        ));
    }
    let platform = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => "windows-x64",
        ("macos", "aarch64") => "macos-arm64",
        (os, arch) => {
            return Err(io::Error::other(format!(
                "unsupported distribution platform: {os}-{arch}"
            )));
        }
    };
    Ok(format!("Grana-{}-{platform}", env!("CARGO_PKG_VERSION")))
}

fn cargo() -> OsString {
    std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"))
}

const fn executable_name() -> &'static str {
    if cfg!(windows) { "grana.exe" } else { "grana" }
}
