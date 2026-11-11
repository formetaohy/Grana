use std::path::{Path, PathBuf};

use grana_chart::Page;
use grana_ui::Ui;
use wgpu::{Device, Queue, TextureFormat};

use crate::vector;

pub(crate) fn write(
    device: &Device,
    queue: &Queue,
    format: TextureFormat,
    page: &Page,
    path: &Path,
) -> Result<PathBuf, String> {
    let kind = vector::format(path).ok_or_else(|| {
        format!(
            "a statistics chart is saved as SVG or PDF, not `{}`",
            path.display()
        )
    })?;
    let size = page.size();
    let mut ui: Ui<()> = Ui::new(device, queue, format);
    ui.resize(size);
    let marks = page.marks(&mut ui);
    vector::write_marks(path, kind, size, &marks)?;
    Ok(path.to_path_buf())
}
