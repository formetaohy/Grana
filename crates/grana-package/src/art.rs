mod windows;

use std::io;
use std::path::Path;

pub fn write(root: &Path) -> io::Result<()> {
    let windows = root.join("windows").join("art");
    std::fs::create_dir_all(&windows)?;
    windows::write(&windows)
}

pub const SCALES: [u32; 3] = [1, 2, 3];

pub fn suffix(scale: u32) -> String {
    if scale == 1 {
        String::new()
    } else {
        format!("@{scale}x")
    }
}
