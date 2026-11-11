use std::path::{Path, PathBuf};

use grana_chart::Page;

use crate::vector;

pub(crate) fn write(page: &Page, path: &Path) -> Result<PathBuf, String> {
    let kind = vector::format(path).ok_or_else(|| {
        format!(
            "a statistics chart is saved as SVG or PDF, not `{}`",
            path.display()
        )
    })?;
    let size = page.size();
    let marks = page.marks();
    vector::write_marks(path, kind, size, &marks)?;
    Ok(path.to_path_buf())
}
