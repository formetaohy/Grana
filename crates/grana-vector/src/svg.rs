use std::fmt::Write as _;

use grana_text::{Contour, Curve};

use crate::base64;
use crate::document::{Cap, Page, Paint, Rect, Rgba};
use crate::number::{byte, text};

pub fn write(page: &Page) -> String {
    let width = text(page.width());
    let height = text(page.height());
    let mut svg = String::new();
    svg.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let _ = writeln!(
        svg,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" width=\"{width}pt\" height=\"{height}pt\" viewBox=\"0 0 {width} {height}\">"
    );
    let clips = clips(page);
    let gradients = gradients(page);
    if !clips.is_empty() || !gradients.is_empty() {
        svg.push_str("<defs>\n");
        for (index, clip) in clips.iter().enumerate() {
            let _ = writeln!(
                svg,
                "<clipPath id=\"clip{index}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/></clipPath>",
                text(clip.min[0]),
                text(clip.min[1]),
                text(clip.width()),
                text(clip.height())
            );
        }
        for (index, (top, bottom)) in gradients.iter().enumerate() {
            let _ = writeln!(
                svg,
                "<linearGradient id=\"grad{index}\" x1=\"0\" y1=\"0\" x2=\"0\" y2=\"1\"><stop offset=\"0\" stop-color=\"{}\" stop-opacity=\"{}\"/><stop offset=\"1\" stop-color=\"{}\" stop-opacity=\"{}\"/></linearGradient>",
                hex(*top),
                text(top.alpha),
                hex(*bottom),
                text(bottom.alpha)
            );
        }
        svg.push_str("</defs>\n");
    }
    if let Some(image) = page.background() {
        let [image_width, image_height] = image.size();
        let raster = grana_raster::Raster::new(image_width, image_height, image.pixels().to_vec());
        let png = grana_raster::encode(&raster).expect("the vector backdrop encodes as a PNG");
        let _ = writeln!(
            svg,
            "<image x=\"0\" y=\"0\" width=\"{width}\" height=\"{height}\" preserveAspectRatio=\"none\" xlink:href=\"data:image/png;base64,{}\"/>",
            base64::encode(&png)
        );
    }
    for shape in page.shapes() {
        let clip = shape.clip.map(|clip| {
            clips
                .iter()
                .position(|candidate| *candidate == clip)
                .expect("the clip is declared")
        });
        if let Some(index) = clip {
            let _ = writeln!(svg, "<g clip-path=\"url(#clip{index})\">");
        }
        let data = path(&shape.contours);
        match shape.paint {
            Paint::Fill(color) => {
                let _ = writeln!(
                    svg,
                    "<path d=\"{data}\" fill=\"{}\" fill-opacity=\"{}\"/>",
                    hex(color),
                    text(color.alpha)
                );
            }
            Paint::Gradient(top, bottom) => {
                let index = gradients
                    .iter()
                    .position(|candidate| *candidate == (top, bottom))
                    .expect("the gradient is declared");
                let _ = writeln!(svg, "<path d=\"{data}\" fill=\"url(#grad{index})\"/>");
            }
            Paint::Stroke { width, color, cap } => {
                let _ = writeln!(
                    svg,
                    "<path d=\"{data}\" fill=\"none\" stroke=\"{}\" stroke-opacity=\"{}\" stroke-width=\"{}\" stroke-linecap=\"{}\" stroke-linejoin=\"round\"/>",
                    hex(color),
                    text(color.alpha),
                    text(width),
                    match cap {
                        Cap::Butt => "butt",
                        Cap::Round => "round",
                    }
                );
            }
        }
        if clip.is_some() {
            svg.push_str("</g>\n");
        }
    }
    svg.push_str("</svg>\n");
    svg
}

fn clips(page: &Page) -> Vec<Rect> {
    let mut clips = Vec::new();
    for shape in page.shapes() {
        if let Some(clip) = shape.clip
            && !clips.contains(&clip)
        {
            clips.push(clip);
        }
    }
    clips
}

fn gradients(page: &Page) -> Vec<(Rgba, Rgba)> {
    let mut gradients = Vec::new();
    for shape in page.shapes() {
        if let Paint::Gradient(top, bottom) = shape.paint
            && !gradients.contains(&(top, bottom))
        {
            gradients.push((top, bottom));
        }
    }
    gradients
}

fn path(contours: &[Contour]) -> String {
    let mut data = String::new();
    for contour in contours {
        let _ = write!(
            data,
            "M {} {} ",
            text(contour.start()[0]),
            text(contour.start()[1])
        );
        for curve in contour.curves() {
            match *curve {
                Curve::Line(to) => {
                    let _ = write!(data, "L {} {} ", text(to[0]), text(to[1]));
                }
                Curve::Quad(control, to) => {
                    let _ = write!(
                        data,
                        "Q {} {} {} {} ",
                        text(control[0]),
                        text(control[1]),
                        text(to[0]),
                        text(to[1])
                    );
                }
                Curve::Cubic(first, second, to) => {
                    let _ = write!(
                        data,
                        "C {} {} {} {} {} {} ",
                        text(first[0]),
                        text(first[1]),
                        text(second[0]),
                        text(second[1]),
                        text(to[0]),
                        text(to[1])
                    );
                }
            }
        }
        data.push_str(if contour.closed() { "Z " } else { "" });
    }
    data.trim_end().to_string()
}

fn hex(color: Rgba) -> String {
    format!(
        "#{:02x}{:02x}{:02x}",
        byte(color.red),
        byte(color.green),
        byte(color.blue)
    )
}
