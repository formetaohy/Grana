use std::f32::consts::{FRAC_PI_2, PI, TAU};

use grana_text::{Contour, Curve};

use crate::document::Rect;

const ARC_STEP: f32 = FRAC_PI_2;

pub fn rounded_rect(rect: Rect, radius: [f32; 4]) -> Vec<Contour> {
    if rect.is_empty() {
        return Vec::new();
    }
    let half = 0.5 * rect.width().min(rect.height());
    let [top_left, top_right, bottom_right, bottom_left] =
        radius.map(|value| value.max(0.0).min(half));
    let [left, top] = rect.min;
    let [right, bottom] = rect.max;
    let mut contour = Contour::new([left + top_left, top]);
    contour.push(Curve::Line([right - top_right, top]));
    corner(
        &mut contour,
        [right - top_right, top + top_right],
        top_right,
        -FRAC_PI_2,
        0.0,
    );
    contour.push(Curve::Line([right, bottom - bottom_right]));
    corner(
        &mut contour,
        [right - bottom_right, bottom - bottom_right],
        bottom_right,
        0.0,
        FRAC_PI_2,
    );
    contour.push(Curve::Line([left + bottom_left, bottom]));
    corner(
        &mut contour,
        [left + bottom_left, bottom - bottom_left],
        bottom_left,
        FRAC_PI_2,
        PI,
    );
    contour.push(Curve::Line([left, top + top_left]));
    corner(
        &mut contour,
        [left + top_left, top + top_left],
        top_left,
        PI,
        3.0 * FRAC_PI_2,
    );
    vec![contour]
}

fn corner(contour: &mut Contour, center: [f32; 2], radius: f32, from: f32, to: f32) {
    if radius <= 0.0 {
        contour.push(Curve::Line(center));
        return;
    }
    contour.push(Curve::Line(arc_point(center, radius, from)));
    arc_into(contour, center, radius, from, to);
}

pub fn segment(from: [f32; 2], to: [f32; 2]) -> Vec<Contour> {
    if from == to {
        return Vec::new();
    }
    let mut contour = Contour::new(from);
    contour.push(Curve::Line(to));
    vec![contour]
}

pub fn arc(center: [f32; 2], radius: f32, start: f32, sweep: f32) -> Vec<Contour> {
    assert!(
        radius.is_finite() && radius > 0.0,
        "a vector arc holds a positive radius"
    );
    if !sweep.is_finite() || sweep <= 0.0 {
        return Vec::new();
    }
    let sweep = sweep.min(TAU);
    let mut contour = Contour::new(arc_point(center, radius, start));
    arc_into(&mut contour, center, radius, start, start + sweep);
    vec![contour]
}

fn arc_into(contour: &mut Contour, center: [f32; 2], radius: f32, from: f32, to: f32) {
    let sweep = to - from;
    let steps = (sweep.abs() / ARC_STEP).ceil().max(1.0) as usize;
    let step = sweep / steps as f32;
    for index in 0..steps {
        let first = from + index as f32 * step;
        let last = first + step;
        let begin = arc_point(center, radius, first);
        let end = arc_point(center, radius, last);
        let control = radius * (step / 4.0).tan() * 4.0 / 3.0;
        let tangent = |angle: f32| [-angle.sin(), angle.cos()];
        let [tx, ty] = tangent(first);
        let [ux, uy] = tangent(last);
        contour.push(Curve::Cubic(
            [begin[0] + control * tx, begin[1] + control * ty],
            [end[0] - control * ux, end[1] - control * uy],
            end,
        ));
    }
}

fn arc_point(center: [f32; 2], radius: f32, angle: f32) -> [f32; 2] {
    [
        center[0] + radius * angle.cos(),
        center[1] + radius * angle.sin(),
    ]
}
