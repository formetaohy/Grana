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
        document.push(recorded_page(page)?);
    }
    if document.pages().is_empty() {
        return Err(format!("{} carries no page to write", path.display()));
    }
    write_document(path, format, &document)
}

fn recorded_page(recorded: &Recorded) -> Result<grana_vector::Page, String> {
    let size = recorded.size();
    let mut canvas = grana_vector::Page::new(size[0] as f32, size[1] as f32);
    if let Some(image) = recorded.image() {
        let raster = grana_raster::read(image).map_err(|error| {
            format!(
                "the recorded picture {} cannot be read: {error}",
                image.display()
            )
        })?;
        if raster.size() != size {
            return Err(format!(
                "the recorded picture {} spans {} by {}, not the page's {} by {}",
                image.display(),
                raster.width(),
                raster.height(),
                size[0],
                size[1]
            ));
        }
        canvas.set_background(grana_vector::Image::new(size, raster.pixels().to_vec()));
    }
    push(&mut canvas, recorded.marks())?;
    Ok(canvas)
}

fn canvas_of(page: &Page) -> Result<grana_vector::Page, String> {
    let size = page.size();
    let mut canvas = grana_vector::Page::new(size[0] as f32, size[1] as f32);
    push(&mut canvas, &page.marks())?;
    Ok(canvas)
}
