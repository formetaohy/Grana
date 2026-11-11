use std::path::{Path, PathBuf};

use grana_chart::Page;

use crate::Format;
use crate::document::{compose, push, write_document};
use crate::record::Recorded;

pub fn write_page(page: &Page, path: &Path) -> Result<PathBuf, String> {
    let format = Format::of(path).ok_or_else(|| Format::error(path, "a statistics page"))?;
    write_document(path, format, &compose(std::slice::from_ref(page))?)
}

pub fn write_report(pages: &[Page], recorded: &[Recorded], path: &Path) -> Result<PathBuf, String> {
    let format = Format::of(path).ok_or_else(|| Format::error(path, "a report"))?;
    let mut document = grana_vector::Document::new();
    for page in pages {
        document.push(canvas_of(page)?);
    }
    for page in recorded {
        let size = page.size();
        let mut canvas = grana_vector::Page::new(size[0] as f32, size[1] as f32);
        push(&mut canvas, page.marks())?;
        document.push(canvas);
    }
    if document.pages().is_empty() {
        return Err(format!("{} carries no page to write", path.display()));
    }
    write_document(path, format, &document)
}

fn canvas_of(page: &Page) -> Result<grana_vector::Page, String> {
    let size = page.size();
    let mut canvas = grana_vector::Page::new(size[0] as f32, size[1] as f32);
    push(&mut canvas, &page.marks())?;
    Ok(canvas)
}
