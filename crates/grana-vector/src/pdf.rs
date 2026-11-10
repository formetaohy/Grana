use std::fmt::Write as _;
use std::io::Write as _;

use flate2::Compression;
use flate2::write::ZlibEncoder;
use grana_text::{Contour, Curve};

use crate::document::{Cap, Document, Image, Paint, Rect, Rgba};
use crate::number::text;

const BAND: usize = 64;

pub fn write(document: &Document) -> Vec<u8> {
    let width = document.width();
    let height = document.height();
    let mut content = String::new();
    let mut alphas: Vec<f32> = Vec::new();
    let mut gradients: Vec<(Rgba, Rgba, Rect)> = Vec::new();
    if document.background().is_some() {
        let _ = writeln!(
            content,
            "q {} 0 0 {} 0 0 cm /Im0 Do Q",
            text(width),
            text(height)
        );
    }
    for shape in document.shapes() {
        content.push_str("q\n");
        if let Some(clip) = shape.clip {
            let _ = writeln!(
                content,
                "{} {} {} {} re W n",
                text(clip.min[0]),
                text(height - clip.max[1]),
                text(clip.width()),
                text(clip.height())
            );
        }
        match shape.paint {
            Paint::Fill(color) => {
                write_alpha(&mut content, &mut alphas, color.alpha);
                let _ = writeln!(
                    content,
                    "{} {} {} rg",
                    text(color.red),
                    text(color.green),
                    text(color.blue)
                );
                path(&mut content, &shape.contours, height);
                content.push_str("f\n");
            }
            Paint::Stroke { width, color, cap } => {
                write_alpha(&mut content, &mut alphas, color.alpha);
                let _ = writeln!(
                    content,
                    "{} {} {} RG",
                    text(color.red),
                    text(color.green),
                    text(color.blue)
                );
                let _ = writeln!(
                    content,
                    "{} w {} J 1 j",
                    text(width),
                    match cap {
                        Cap::Butt => 0,
                        Cap::Round => 1,
                    }
                );
                path(&mut content, &shape.contours, height);
                content.push_str("S\n");
            }
            Paint::Gradient(top, bottom) => {
                let bounds = bounds(&shape.contours).expect("a gradient shape spans an area");
                if (top.alpha - bottom.alpha).abs() < 1e-4 {
                    write_alpha(&mut content, &mut alphas, top.alpha);
                    let index = gradients.len();
                    gradients.push((top, bottom, bounds));
                    let _ = writeln!(content, "/Pattern cs /P{index} scn");
                    path(&mut content, &shape.contours, height);
                    content.push_str("f\n");
                } else {
                    path(&mut content, &shape.contours, height);
                    content.push_str("W n\n");
                    bands(&mut content, &mut alphas, top, bottom, bounds, height);
                }
            }
        }
        content.push_str("Q\n");
    }
    serialize(document, &content, &alphas, &gradients)
}

fn write_alpha(content: &mut String, alphas: &mut Vec<f32>, alpha: f32) {
    if alpha >= 1.0 {
        return;
    }
    let index = match alphas
        .iter()
        .position(|value| (*value - alpha).abs() < 1e-4)
    {
        Some(index) => index,
        None => {
            alphas.push(alpha);
            alphas.len() - 1
        }
    };
    let _ = writeln!(content, "/GS{index} gs");
}

fn bands(
    content: &mut String,
    alphas: &mut Vec<f32>,
    top: Rgba,
    bottom: Rgba,
    bounds: Rect,
    height: f32,
) {
    let step = bounds.height() / BAND as f32;
    for index in 0..BAND {
        let from = index as f32 / BAND as f32;
        let to = (index + 1) as f32 / BAND as f32;
        let start = mix(top, bottom, from);
        let end = mix(top, bottom, to);
        write_alpha(content, alphas, 0.5 * (start.alpha + end.alpha));
        let _ = writeln!(
            content,
            "{} {} {} rg",
            text(start.red),
            text(start.green),
            text(start.blue)
        );
        let _ = writeln!(
            content,
            "{} {} {} {} re f",
            text(bounds.min[0]),
            text(height - bounds.min[1] - to * bounds.height()),
            text(bounds.width()),
            text(step)
        );
    }
}

fn mix(top: Rgba, bottom: Rgba, t: f32) -> Rgba {
    Rgba::new(
        top.red + (bottom.red - top.red) * t,
        top.green + (bottom.green - top.green) * t,
        top.blue + (bottom.blue - top.blue) * t,
        top.alpha + (bottom.alpha - top.alpha) * t,
    )
}

fn bounds(contours: &[Contour]) -> Option<Rect> {
    let mut min = [f32::INFINITY; 2];
    let mut max = [f32::NEG_INFINITY; 2];
    let mut visit = |point: [f32; 2]| {
        min = [min[0].min(point[0]), min[1].min(point[1])];
        max = [max[0].max(point[0]), max[1].max(point[1])];
    };
    for contour in contours {
        visit(contour.start());
        for curve in contour.curves() {
            match *curve {
                Curve::Line(to) => visit(to),
                Curve::Quad(control, to) => {
                    visit(control);
                    visit(to);
                }
                Curve::Cubic(first, second, to) => {
                    visit(first);
                    visit(second);
                    visit(to);
                }
            }
        }
    }
    (min[0] <= max[0] && min[1] <= max[1]).then(|| Rect::new(min, max))
}

fn path(content: &mut String, contours: &[Contour], height: f32) {
    for contour in contours {
        let start = contour.start();
        let _ = writeln!(content, "{} {} m", text(start[0]), text(height - start[1]));
        let mut current = start;
        for curve in contour.curves() {
            match *curve {
                Curve::Line(to) => {
                    let _ = writeln!(content, "{} {} l", text(to[0]), text(height - to[1]));
                    current = to;
                }
                Curve::Quad(control, to) => {
                    let first = [
                        current[0] + 2.0 / 3.0 * (control[0] - current[0]),
                        current[1] + 2.0 / 3.0 * (control[1] - current[1]),
                    ];
                    let second = [
                        to[0] + 2.0 / 3.0 * (control[0] - to[0]),
                        to[1] + 2.0 / 3.0 * (control[1] - to[1]),
                    ];
                    let _ = writeln!(
                        content,
                        "{} {} {} {} {} {} c",
                        text(first[0]),
                        text(height - first[1]),
                        text(second[0]),
                        text(height - second[1]),
                        text(to[0]),
                        text(height - to[1])
                    );
                    current = to;
                }
                Curve::Cubic(first, second, to) => {
                    let _ = writeln!(
                        content,
                        "{} {} {} {} {} {} c",
                        text(first[0]),
                        text(height - first[1]),
                        text(second[0]),
                        text(height - second[1]),
                        text(to[0]),
                        text(height - to[1])
                    );
                    current = to;
                }
            }
        }
        if contour.closed() {
            content.push_str("h\n");
        }
    }
}

fn serialize(
    document: &Document,
    content: &str,
    alphas: &[f32],
    gradients: &[(Rgba, Rgba, Rect)],
) -> Vec<u8> {
    let width = document.width();
    let height = document.height();
    let image = document.background();
    let mask = image.and_then(alpha_mask);
    let mut next = 5u32;
    let image_object = image.map(|_| {
        let object = next;
        next += 1;
        object
    });
    let mask_object = mask.as_ref().map(|_| {
        let object = next;
        next += 1;
        object
    });
    let state_base = next;
    next += alphas.len() as u32;
    let pattern_base = next;
    next += gradients.len() as u32;
    let shading_base = next;
    next += gradients.len() as u32;
    let count = next;

    let mut resources = String::from("/ProcSet [/PDF /ImageC /ImageB]");
    if let Some(object) = image_object {
        let _ = write!(resources, " /XObject << /Im0 {object} 0 R >>");
    }
    if !alphas.is_empty() {
        let _ = write!(resources, " /ExtGState <<");
        for index in 0..alphas.len() {
            let _ = write!(resources, " /GS{index} {} 0 R", state_base + index as u32);
        }
        resources.push_str(" >>");
    }
    if !gradients.is_empty() {
        let _ = write!(resources, " /Pattern <<");
        for index in 0..gradients.len() {
            let _ = write!(resources, " /P{index} {} 0 R", pattern_base + index as u32);
        }
        resources.push_str(" >>");
    }

    let mut bodies: Vec<Vec<u8>> = vec![Vec::new(); count as usize];
    bodies[1] = b"<< /Type /Catalog /Pages 2 0 R >>".to_vec();
    bodies[2] = b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec();
    bodies[3] = format!(
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Resources << {resources} >> /Contents 4 0 R >>",
        text(width),
        text(height)
    )
    .into_bytes();
    let stream = content.as_bytes();
    let mut contents = format!("<< /Length {} >>\nstream\n", stream.len()).into_bytes();
    contents.extend_from_slice(stream);
    contents.extend_from_slice(b"endstream");
    bodies[4] = contents;
    if let (Some(object), Some(image)) = (image_object, image) {
        let [image_width, image_height] = image.size();
        let rgb = flate(&color_bytes(image));
        let mut body = format!(
            "<< /Type /XObject /Subtype /Image /Width {image_width} /Height {image_height} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode"
        );
        if let Some(mask) = mask_object {
            let _ = write!(body, " /SMask {mask} 0 R");
        }
        let _ = write!(body, " /Length {} >>\nstream\n", rgb.len());
        let mut bytes = body.into_bytes();
        bytes.extend_from_slice(&rgb);
        bytes.extend_from_slice(b"\nendstream");
        bodies[object as usize] = bytes;
    }
    if let (Some(object), Some(mask)) = (mask_object, mask.as_ref()) {
        let gray = flate(mask);
        let [image_width, image_height] = document
            .background()
            .expect("the mask follows the backdrop")
            .size();
        let mut bytes = format!(
            "<< /Type /XObject /Subtype /Image /Width {image_width} /Height {image_height} /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode /Length {} >>\nstream\n",
            gray.len()
        )
        .into_bytes();
        bytes.extend_from_slice(&gray);
        bytes.extend_from_slice(b"\nendstream");
        bodies[object as usize] = bytes;
    }
    for (index, alpha) in alphas.iter().enumerate() {
        bodies[(state_base + index as u32) as usize] = format!(
            "<< /Type /ExtGState /ca {} /CA {} >>",
            text(*alpha),
            text(*alpha)
        )
        .into_bytes();
    }
    for (index, (top, bottom, bounds)) in gradients.iter().enumerate() {
        let x = 0.5 * (bounds.min[0] + bounds.max[0]);
        let shading = format!(
            "<< /ShadingType 2 /ColorSpace /DeviceRGB /Coords [{} {} {} {}] /Function << /FunctionType 2 /Domain [0 1] /C0 [{} {} {}] /C1 [{} {} {}] /N 1 >> /Extend [true true] >>",
            text(x),
            text(height - bounds.min[1]),
            text(x),
            text(height - bounds.max[1]),
            text(top.red),
            text(top.green),
            text(top.blue),
            text(bottom.red),
            text(bottom.green),
            text(bottom.blue)
        );
        bodies[(shading_base + index as u32) as usize] = shading.into_bytes();
        bodies[(pattern_base + index as u32) as usize] = format!(
            "<< /Type /Pattern /PatternType 2 /Shading {} 0 R /Matrix [1 0 0 1 0 0] >>",
            shading_base + index as u32
        )
        .into_bytes();
    }

    let mut file = Vec::new();
    file.extend_from_slice(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n");
    let mut offsets = vec![0usize; count as usize];
    for (index, body) in bodies.iter().enumerate().skip(1) {
        offsets[index] = file.len();
        let _ = writeln!(file, "{index} 0 obj");
        file.extend_from_slice(body);
        file.extend_from_slice(b"\nendobj\n");
    }
    let start = file.len();
    let _ = writeln!(file, "xref\n0 {count}");
    file.extend_from_slice(b"0000000000 65535 f \n");
    for offset in offsets.iter().skip(1) {
        let _ = writeln!(file, "{offset:010} 00000 n ");
    }
    let _ = write!(
        file,
        "trailer\n<< /Size {count} /Root 1 0 R >>\nstartxref\n{start}\n%%EOF\n"
    );
    file
}

fn color_bytes(image: &Image) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(image.pixels().len() / 4 * 3);
    for pixel in image.pixels().as_chunks::<4>().0 {
        bytes.extend_from_slice(&pixel[..3]);
    }
    bytes
}

fn alpha_mask(image: &Image) -> Option<Vec<u8>> {
    let visible = image
        .pixels()
        .as_chunks::<4>()
        .0
        .iter()
        .any(|pixel| pixel[3] != 255);
    visible.then(|| {
        image
            .pixels()
            .as_chunks::<4>()
            .0
            .iter()
            .map(|pixel| pixel[3])
            .collect()
    })
}

fn flate(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::new(6));
    encoder
        .write_all(bytes)
        .expect("the flate stream accepts its bytes");
    encoder.finish().expect("the flate stream finishes")
}
