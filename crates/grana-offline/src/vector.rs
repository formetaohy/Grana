use std::path::{Path, PathBuf};

use grana_text::{Contour, Curve};
use grana_ui::{Color, Corners, Mark, Rect};
use grana_vector::{Document, Image, Paint, Rgba, Shape};

use crate::sink;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Format {
    Svg,
    Pdf,
}

pub(crate) fn format(path: &Path) -> Option<Format> {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("svg") => Some(Format::Svg),
        Some("pdf") => Some(Format::Pdf),
        _ => None,
    }
}

pub(crate) fn write(
    path: &Path,
    format: Format,
    size: [u32; 2],
    pixels: &[u16],
    marks: &[Mark],
    transparent: bool,
) -> Result<(), String> {
    let mut document = Document::new(size[0] as f32, size[1] as f32);
    let cleared = sink::rgba8(size, pixels, transparent);
    document.set_background(Image::new(size, cleared));
    push(&mut document, marks)?;
    write_document(path, format, &document)
}

pub(crate) fn write_marks(
    path: &Path,
    format: Format,
    size: [u32; 2],
    marks: &[Mark],
) -> Result<(), String> {
    if size[0] == 0 || size[1] == 0 {
        return Err("a vector page covers at least one point".to_string());
    }
    let mut document = Document::new(size[0] as f32, size[1] as f32);
    push(&mut document, marks)?;
    write_document(path, format, &document)
}

fn write_document(path: &Path, format: Format, document: &Document) -> Result<(), String> {
    let bytes = match format {
        Format::Svg => grana_vector::write_svg(document).into_bytes(),
        Format::Pdf => grana_vector::write_pdf(document),
    };
    grana_file::write(path, &bytes)
        .map_err(|error| format!("failed to write {}: {error}", PathBuf::from(path).display()))
}

fn push(document: &mut Document, marks: &[Mark]) -> Result<(), String> {
    for mark in marks {
        match *mark {
            Mark::Rect {
                rect,
                corners,
                color,
                clip,
            } => {
                let shape = Shape::new(
                    grana_vector::rounded_rect(to_rect(rect), to_radii(corners, rect)),
                    Paint::Fill(to_rgba(color)),
                )
                .clipped(to_clip(clip));
                document.push(shape);
            }
            Mark::Gradient {
                rect,
                corners,
                top,
                bottom,
                clip,
            } => {
                let shape = Shape::new(
                    grana_vector::rounded_rect(to_rect(rect), to_radii(corners, rect)),
                    Paint::Gradient(to_rgba(top), to_rgba(bottom)),
                )
                .clipped(to_clip(clip));
                document.push(shape);
            }
            Mark::Border {
                rect,
                corners,
                width,
                color,
                clip,
            } => {
                let inset = 0.5 * width;
                let inner = rect.shrink(inset);
                let mut radii = to_radii(corners, rect);
                for value in &mut radii {
                    *value = (*value - inset).max(0.0);
                }
                let shape = Shape::new(
                    grana_vector::rounded_rect(to_rect(inner), radii),
                    Paint::Stroke {
                        width,
                        color: to_rgba(color),
                        cap: grana_vector::Cap::Round,
                    },
                )
                .clipped(to_clip(clip));
                document.push(shape);
            }
            Mark::Triangle {
                corners,
                color,
                clip,
            } => {
                let shape = Shape::new(vec![triangle(&corners)], Paint::Fill(to_rgba(color)))
                    .clipped(to_clip(clip));
                document.push(shape);
            }
            Mark::Segment {
                from,
                to,
                width,
                start,
                end,
                clip,
            } => {
                let pieces = if start == end { 1 } else { 8 };
                for index in 0..pieces {
                    let begin = interpolate(from, to, index as f32 / pieces as f32);
                    let finish = interpolate(from, to, (index + 1) as f32 / pieces as f32);
                    let color = start.lerp(end, (index as f32 + 0.5) / pieces as f32);
                    let shape = Shape::new(
                        grana_vector::segment(begin, finish),
                        Paint::Stroke {
                            width,
                            color: to_rgba(color),
                            cap: grana_vector::Cap::Round,
                        },
                    )
                    .clipped(to_clip(clip));
                    document.push(shape);
                }
            }
            Mark::Arc {
                center,
                radius,
                width,
                start,
                sweep,
                color,
                clip,
            } => {
                let shape = Shape::new(
                    grana_vector::arc(center, radius, start, sweep),
                    Paint::Stroke {
                        width,
                        color: to_rgba(color),
                        cap: grana_vector::Cap::Butt,
                    },
                )
                .clipped(to_clip(clip));
                document.push(shape);
            }
            Mark::Outline {
                ref contours,
                color,
                clip,
            } => {
                let shape = Shape::new(contours.clone(), Paint::Fill(to_rgba(color)))
                    .clipped(to_clip(clip));
                document.push(shape);
            }
            Mark::Bitmap { .. } => {
                return Err(
                    "a vector figure holds outlines, lines and fields, not rasters".to_string(),
                );
            }
        }
    }
    Ok(())
}

fn to_rect(rect: Rect) -> grana_vector::Rect {
    grana_vector::Rect::new(rect.min.to_array(), rect.max.to_array())
}

fn to_clip(rect: Rect) -> Option<grana_vector::Rect> {
    (!rect.is_empty()).then(|| to_rect(rect))
}

fn to_radii(corners: Corners, rect: Rect) -> [f32; 4] {
    let resolved = corners.resolved(rect.size());
    [
        resolved.top_left,
        resolved.top_right,
        resolved.bottom_right,
        resolved.bottom_left,
    ]
}

fn to_rgba(color: Color) -> Rgba {
    Rgba::new(color.red, color.green, color.blue, color.alpha)
}

fn triangle(corners: &[[f32; 2]; 3]) -> Contour {
    let mut contour = Contour::new(corners[0]);
    contour.push(Curve::Line(corners[1]));
    contour.push(Curve::Line(corners[2]));
    contour
}

fn interpolate(from: [f32; 2], to: [f32; 2], t: f32) -> [f32; 2] {
    [
        from[0] + (to[0] - from[0]) * t,
        from[1] + (to[1] - from[1]) * t,
    ]
}
