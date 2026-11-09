use std::path::PathBuf;

pub fn cache() -> Option<PathBuf> {
    base(Kind::Cache).map(|base| base.join("grana"))
}

pub fn data() -> Option<PathBuf> {
    base(Kind::Data).map(|base| base.join("grana"))
}

#[derive(Clone, Copy)]
enum Kind {
    Cache,
    Data,
}

#[cfg(target_os = "windows")]
fn base(kind: Kind) -> Option<PathBuf> {
    let local = variable("LOCALAPPDATA");
    match kind {
        Kind::Cache => local,
        Kind::Data => variable("APPDATA").or(local),
    }
}

#[cfg(target_os = "macos")]
fn base(kind: Kind) -> Option<PathBuf> {
    let home = variable("HOME")?;
    Some(match kind {
        Kind::Cache => home.join("Library").join("Caches"),
        Kind::Data => home.join("Library").join("Application Support"),
    })
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn base(kind: Kind) -> Option<PathBuf> {
    let (specific, fallback) = match kind {
        Kind::Cache => ("XDG_CACHE_HOME", ".cache"),
        Kind::Data => ("XDG_DATA_HOME", ".local/share"),
    };
    variable(specific).or_else(|| variable("HOME").map(|home| home.join(fallback)))
}

fn variable(name: &str) -> Option<PathBuf> {
    let value = std::env::var_os(name)?;
    let path = PathBuf::from(value);
    (!path.as_os_str().is_empty()).then_some(path)
}
