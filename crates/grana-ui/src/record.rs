use grana_raster::Bitmap;
use grana_text::Contour;

use crate::color::Color;
use crate::geometry::{Corners, Rect};

#[derive(Clone, Debug)]
pub enum Mark {
    Rect {
        rect: Rect,
        corners: Corners,
        color: Color,
        clip: Rect,
    },
    Gradient {
        rect: Rect,
        corners: Corners,
        top: Color,
        bottom: Color,
        clip: Rect,
    },
    Border {
        rect: Rect,
        corners: Corners,
        width: f32,
        color: Color,
        clip: Rect,
    },
    Triangle {
        corners: [[f32; 2]; 3],
        color: Color,
        clip: Rect,
    },
    Segment {
        from: [f32; 2],
        to: [f32; 2],
        width: f32,
        start: Color,
        end: Color,
        clip: Rect,
    },
    Arc {
        center: [f32; 2],
        radius: f32,
        width: f32,
        start: f32,
        sweep: f32,
        color: Color,
        clip: Rect,
    },
    Outline {
        contours: Vec<Contour>,
        color: Color,
        clip: Rect,
    },
    Bitmap {
        rect: Rect,
        bitmap: Bitmap,
        clip: Rect,
    },
}
