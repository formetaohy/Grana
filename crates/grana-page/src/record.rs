use std::path::Path;

use grana_chart::Page;
use grana_text::{Contour, Curve};
use grana_ui::{Color, Corners, Mark, Rect, Vec2};

pub struct Recorded {
    size: [u32; 2],
    marks: Vec<Mark>,
}

impl Recorded {
    pub fn of(page: &Page) -> Self {
        Self {
            size: page.size(),
            marks: page.marks(),
        }
    }

    pub fn size(&self) -> [u32; 2] {
        self.size
    }

    pub fn marks(&self) -> &[Mark] {
        &self.marks
    }

    pub fn encode(pages: &[Self]) -> Result<String, String> {
        let records = pages
            .iter()
            .map(|page| {
                Ok(PageRecord {
                    size: page.size,
                    marks: page
                        .marks
                        .iter()
                        .map(MarkRecord::of)
                        .collect::<Result<Vec<_>, String>>()?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        serde_json::to_string(&records)
            .map_err(|error| format!("a page cannot be recorded: {error}"))
    }

    pub fn decode(text: &str) -> Result<Vec<Self>, String> {
        let records = serde_json::from_str::<Vec<PageRecord>>(text)
            .map_err(|error| format!("a recorded page cannot be read: {error}"))?;
        records.into_iter().map(PageRecord::into_page).collect()
    }

    pub fn write(path: &Path, pages: &[Self]) -> Result<(), String> {
        grana_file::write(path, Self::encode(pages)?.as_bytes())
            .map_err(|error| format!("failed to write {}: {error}", path.display()))
    }

    pub fn read(path: &Path) -> Result<Vec<Self>, String> {
        let bytes = std::fs::read(path)
            .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
        let text = std::str::from_utf8(&bytes)
            .map_err(|error| format!("{} holds no recorded page: {error}", path.display()))?;
        Self::decode(text)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct PageRecord {
    size: [u32; 2],
    marks: Vec<MarkRecord>,
}

impl PageRecord {
    fn into_page(self) -> Result<Recorded, String> {
        Ok(Recorded {
            size: self.size,
            marks: self
                .marks
                .into_iter()
                .map(MarkRecord::into_mark)
                .collect::<Result<Vec<_>, String>>()?,
        })
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
enum MarkRecord {
    Rect {
        rect: [f32; 4],
        corners: [f32; 4],
        color: [f32; 4],
        clip: [f32; 4],
    },
    Gradient {
        rect: [f32; 4],
        corners: [f32; 4],
        top: [f32; 4],
        bottom: [f32; 4],
        clip: [f32; 4],
    },
    Border {
        rect: [f32; 4],
        corners: [f32; 4],
        width: f32,
        color: [f32; 4],
        clip: [f32; 4],
    },
    Triangle {
        corners: [[f32; 2]; 3],
        color: [f32; 4],
        clip: [f32; 4],
    },
    Segment {
        from: [f32; 2],
        to: [f32; 2],
        width: f32,
        start: [f32; 4],
        end: [f32; 4],
        clip: [f32; 4],
    },
    Arc {
        center: [f32; 2],
        radius: f32,
        width: f32,
        start: f32,
        sweep: f32,
        color: [f32; 4],
        clip: [f32; 4],
    },
    Outline {
        contours: Vec<ContourRecord>,
        color: [f32; 4],
        clip: [f32; 4],
    },
}

impl MarkRecord {
    fn of(mark: &Mark) -> Result<Self, String> {
        Ok(match *mark {
            Mark::Rect {
                rect,
                corners,
                color,
                clip,
            } => Self::Rect {
                rect: rect_of(rect),
                corners: corners_of(corners, rect),
                color: color_of(color),
                clip: rect_of(clip),
            },
            Mark::Gradient {
                rect,
                corners,
                top,
                bottom,
                clip,
            } => Self::Gradient {
                rect: rect_of(rect),
                corners: corners_of(corners, rect),
                top: color_of(top),
                bottom: color_of(bottom),
                clip: rect_of(clip),
            },
            Mark::Border {
                rect,
                corners,
                width,
                color,
                clip,
            } => Self::Border {
                rect: rect_of(rect),
                corners: corners_of(corners, rect),
                width,
                color: color_of(color),
                clip: rect_of(clip),
            },
            Mark::Triangle {
                corners,
                color,
                clip,
            } => Self::Triangle {
                corners,
                color: color_of(color),
                clip: rect_of(clip),
            },
            Mark::Segment {
                from,
                to,
                width,
                start,
                end,
                clip,
            } => Self::Segment {
                from,
                to,
                width,
                start: color_of(start),
                end: color_of(end),
                clip: rect_of(clip),
            },
            Mark::Arc {
                center,
                radius,
                width,
                start,
                sweep,
                color,
                clip,
            } => Self::Arc {
                center,
                radius,
                width,
                start,
                sweep,
                color: color_of(color),
                clip: rect_of(clip),
            },
            Mark::Outline {
                ref contours,
                color,
                clip,
            } => Self::Outline {
                contours: contours.iter().map(ContourRecord::of).collect(),
                color: color_of(color),
                clip: rect_of(clip),
            },
            Mark::Bitmap { .. } => {
                return Err("a raster is not part of a recorded vector page".to_string());
            }
        })
    }

    fn into_mark(self) -> Result<Mark, String> {
        Ok(match self {
            Self::Rect {
                rect,
                corners,
                color,
                clip,
            } => Mark::Rect {
                rect: into_rect(rect),
                corners: into_corners(corners),
                color: into_color(color),
                clip: into_rect(clip),
            },
            Self::Gradient {
                rect,
                corners,
                top,
                bottom,
                clip,
            } => Mark::Gradient {
                rect: into_rect(rect),
                corners: into_corners(corners),
                top: into_color(top),
                bottom: into_color(bottom),
                clip: into_rect(clip),
            },
            Self::Border {
                rect,
                corners,
                width,
                color,
                clip,
            } => Mark::Border {
                rect: into_rect(rect),
                corners: into_corners(corners),
                width,
                color: into_color(color),
                clip: into_rect(clip),
            },
            Self::Triangle {
                corners,
                color,
                clip,
            } => Mark::Triangle {
                corners,
                color: into_color(color),
                clip: into_rect(clip),
            },
            Self::Segment {
                from,
                to,
                width,
                start,
                end,
                clip,
            } => Mark::Segment {
                from,
                to,
                width,
                start: into_color(start),
                end: into_color(end),
                clip: into_rect(clip),
            },
            Self::Arc {
                center,
                radius,
                width,
                start,
                sweep,
                color,
                clip,
            } => Mark::Arc {
                center,
                radius,
                width,
                start,
                sweep,
                color: into_color(color),
                clip: into_rect(clip),
            },
            Self::Outline {
                contours,
                color,
                clip,
            } => Mark::Outline {
                contours: contours
                    .into_iter()
                    .map(ContourRecord::into_contour)
                    .collect(),
                color: into_color(color),
                clip: into_rect(clip),
            },
        })
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ContourRecord {
    start: [f32; 2],
    curves: Vec<CurveRecord>,
}

impl ContourRecord {
    fn of(contour: &Contour) -> Self {
        Self {
            start: contour.start(),
            curves: contour.curves().iter().map(CurveRecord::of).collect(),
        }
    }

    fn into_contour(self) -> Contour {
        let mut contour = Contour::new(self.start);
        for curve in self.curves {
            contour.push(curve.into_curve());
        }
        contour
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
enum CurveRecord {
    Line([f32; 2]),
    Quad([f32; 2], [f32; 2]),
    Cubic([f32; 2], [f32; 2], [f32; 2]),
}

impl CurveRecord {
    fn of(curve: &Curve) -> Self {
        match *curve {
            Curve::Line(point) => Self::Line(point),
            Curve::Quad(control, point) => Self::Quad(control, point),
            Curve::Cubic(first, second, point) => Self::Cubic(first, second, point),
        }
    }

    fn into_curve(self) -> Curve {
        match self {
            Self::Line(point) => Curve::Line(point),
            Self::Quad(control, point) => Curve::Quad(control, point),
            Self::Cubic(first, second, point) => Curve::Cubic(first, second, point),
        }
    }
}

fn rect_of(rect: Rect) -> [f32; 4] {
    [
        rect.min.x,
        rect.min.y,
        rect.max.x - rect.min.x,
        rect.max.y - rect.min.y,
    ]
}

fn into_rect(rect: [f32; 4]) -> Rect {
    Rect::from_min_size(Vec2::new(rect[0], rect[1]), Vec2::new(rect[2], rect[3]))
}

fn corners_of(corners: Corners, rect: Rect) -> [f32; 4] {
    let resolved = corners.resolved(rect.size());
    [
        resolved.top_left,
        resolved.top_right,
        resolved.bottom_right,
        resolved.bottom_left,
    ]
}

fn into_corners(corners: [f32; 4]) -> Corners {
    Corners {
        top_left: corners[0],
        top_right: corners[1],
        bottom_right: corners[2],
        bottom_left: corners[3],
    }
}

fn color_of(color: Color) -> [f32; 4] {
    [color.red, color.green, color.blue, color.alpha]
}

fn into_color(color: [f32; 4]) -> Color {
    Color::rgba(color[0], color[1], color[2], color[3])
}
