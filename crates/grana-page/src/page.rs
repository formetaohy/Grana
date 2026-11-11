use std::path::{Path, PathBuf};

use grana_chart::Page;

use crate::Format;
use crate::document::write;

pub fn write_page(page: &Page, path: &Path) -> Result<PathBuf, String> {
    let format = Format::of(path).ok_or_else(|| Format::error(path, "a statistics page"))?;
    write(path, format, page.size(), &page.marks())
}
