use std::path::{Path, PathBuf};

use grana_chart::Page;

use crate::Format;
use crate::document::{compose, write_document};

pub fn write_page(page: &Page, path: &Path) -> Result<PathBuf, String> {
    let format = Format::of(path).ok_or_else(|| Format::error(path, "a statistics page"))?;
    write_document(path, format, &compose(std::slice::from_ref(page))?)
}

pub fn write_pages(pages: &[Page], path: &Path) -> Result<PathBuf, String> {
    if pages.is_empty() {
        return Err(format!("{} carries no page to write", path.display()));
    }
    let format = Format::of(path).ok_or_else(|| Format::error(path, "a report"))?;
    write_document(path, format, &compose(pages)?)
}
