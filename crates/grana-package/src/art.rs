mod macos;
mod windows;

use std::io;
use std::path::Path;

pub fn write(root: &Path) -> io::Result<()> {
    let windows = root.join("windows").join("art");
    let macos = root.join("macos").join("art");
    std::fs::create_dir_all(&windows)?;
    std::fs::create_dir_all(&macos)?;
    windows::write(&windows)?;
    macos::write(&macos)
}

pub const SCALES: [u32; 3] = [1, 2, 3];

pub fn suffix(scale: u32) -> String {
    if scale == 1 {
        String::new()
    } else {
        format!("@{scale}x")
    }
}
