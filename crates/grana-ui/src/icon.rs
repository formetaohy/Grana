use std::f32::consts::{PI, TAU};

use glam::Vec2;

use crate::color::Color;
use crate::geometry::{Corners, Rect};
use crate::paint::Painter;
use crate::style::{Border, Fill, Shadow};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    ChevronDown,
    ChevronUp,
    ChevronLeft,
    ChevronRight,
    Plus,
    Minus,
    Close,
    Check,
    Ellipsis,
    Play,
    Pause,
    StepBack,
    StepForward,
    Record,
    Eye,
    EyeOff,
    Folder,
    Save,
    Copy,
    Camera,
    Globe,
    Film,
    Export,
    Trash,
    Cube,
    Section,
    Fit,
    Reset,
    Layers,
    Target,
    Search,
    Sliders,
    Table,
    Grid,
    Sparkles,
    Info,
    Warning,
    Clock,
    Lock,
    Channel,
    Filter,
    Columns,
    Sidebar,
    Undo,
    Redo,
    Pencil,
    Ruler,
    Sphere,
}

pub fn chevron(
    painter: &mut Painter<'_>,
    center: Vec2,
    size: f32,
    angle: f32,
    color: Color,
    weight: f32,
) {
    let (sine, cosine) = angle.sin_cos();
    let direction = Vec2::new(cosine, sine);
    let normal = Vec2::new(-sine, cosine);
    let tip = center + direction * size * 0.28;
    let back = center - direction * size * 0.28;
    let wing = size * 0.30;
    painter.segment(back + normal * wing, tip, weight, color);
    painter.segment(back - normal * wing, tip, weight, color);
}

pub fn paint(
    icon: Icon,
    painter: &mut Painter<'_>,
    center: Vec2,
    size: f32,
    color: Color,
    weight: f32,
) {
    let scale = size / 20.0;
    let stroke = weight.max(1.0);
    let point = |x: f32, y: f32| center + (Vec2::new(x, y) - Vec2::splat(10.0)) * scale;
    let line = |painter: &mut Painter<'_>, a: (f32, f32), b: (f32, f32)| {
        painter.segment(point(a.0, a.1), point(b.0, b.1), stroke, color);
    };
    let path = |painter: &mut Painter<'_>, points: &[(f32, f32)]| {
        for pair in points.windows(2) {
            line(painter, pair[0], pair[1]);
        }
    };
    let rounded = |painter: &mut Painter<'_>, rect: Rect, radius: f32| {
        painter.outlined(
            rect,
            Corners::all(radius * scale),
            Fill::None,
            Border {
                width: stroke,
                color,
            },
            Shadow::NONE,
        );
    };
    let dot = |painter: &mut Painter<'_>, x: f32, y: f32, radius: f32| {
        painter.rect(
            Rect::from_center_size(point(x, y), Vec2::splat(radius * 2.0 * scale)),
            Corners::capsule(),
            color,
        );
    };
    let arc = |painter: &mut Painter<'_>, x: f32, y: f32, radius: f32, start: f32, sweep: f32| {
        painter.ring(point(x, y), radius * scale, stroke, start, sweep, color);
    };
    match icon {
        Icon::ChevronDown => path(painter, &[(5.6, 8.2), (10.0, 12.6), (14.4, 8.2)]),
        Icon::ChevronUp => path(painter, &[(5.6, 11.8), (10.0, 7.4), (14.4, 11.8)]),
        Icon::ChevronLeft => path(painter, &[(12.0, 5.6), (7.4, 10.0), (12.0, 14.4)]),
        Icon::ChevronRight => path(painter, &[(8.0, 5.6), (12.6, 10.0), (8.0, 14.4)]),
        Icon::Plus => {
            line(painter, (10.0, 4.6), (10.0, 15.4));
            line(painter, (4.6, 10.0), (15.4, 10.0));
        }
        Icon::Minus => line(painter, (4.6, 10.0), (15.4, 10.0)),
        Icon::Close => {
            line(painter, (5.8, 5.8), (14.2, 14.2));
            line(painter, (14.2, 5.8), (5.8, 14.2));
        }
        Icon::Check => path(painter, &[(4.6, 10.4), (8.4, 14.2), (15.4, 6.0)]),
        Icon::Ellipsis => {
            dot(painter, 5.4, 10.0, 1.25);
            dot(painter, 10.0, 10.0, 1.25);
            dot(painter, 14.6, 10.0, 1.25);
        }
        Icon::Play => {
            painter.triangle(point(7.0, 4.8), point(15.8, 10.0), point(7.0, 15.2), color);
        }
        Icon::Pause => {
            painter.rect(
                Rect::from_center_size(point(7.2, 10.0), Vec2::new(3.0, 10.4) * scale),
                Corners::all(1.2 * scale),
                color,
            );
            painter.rect(
                Rect::from_center_size(point(12.8, 10.0), Vec2::new(3.0, 10.4) * scale),
                Corners::all(1.2 * scale),
                color,
            );
        }
        Icon::StepBack => {
            painter.rect(
                Rect::from_center_size(point(5.4, 10.0), Vec2::new(1.8, 10.0) * scale),
                Corners::all(0.9 * scale),
                color,
            );
            painter.triangle(point(15.2, 5.2), point(15.2, 14.8), point(7.6, 10.0), color);
        }
        Icon::StepForward => {
            painter.rect(
                Rect::from_center_size(point(14.6, 10.0), Vec2::new(1.8, 10.0) * scale),
                Corners::all(0.9 * scale),
                color,
            );
            painter.triangle(point(4.8, 5.2), point(4.8, 14.8), point(12.4, 10.0), color);
        }
        Icon::Record => dot(painter, 10.0, 10.0, 4.2),
        Icon::Eye => {
            arc(painter, 10.0, 14.1, 8.1, PI * 1.166, PI * 0.668);
            arc(painter, 10.0, 5.9, 8.1, PI * 0.334, PI * 0.668);
            arc(painter, 10.0, 10.0, 2.7, 0.0, TAU);
        }
        Icon::EyeOff => {
            arc(painter, 10.0, 14.1, 8.1, PI * 1.166, PI * 0.668);
            arc(painter, 10.0, 5.9, 8.1, PI * 0.334, PI * 0.668);
            arc(painter, 10.0, 10.0, 2.7, 0.0, TAU);
            line(painter, (4.4, 4.4), (15.6, 15.6));
        }
        Icon::Folder => {
            path(
                painter,
                &[
                    (3.2, 15.8),
                    (3.2, 6.2),
                    (8.0, 6.2),
                    (9.4, 4.6),
                    (14.6, 4.6),
                    (16.0, 6.2),
                    (16.8, 6.2),
                    (16.8, 15.8),
                    (3.2, 15.8),
                ],
            );
        }
        Icon::Save => {
            line(painter, (10.0, 3.8), (10.0, 12.0));
            path(painter, &[(6.4, 8.6), (10.0, 12.2), (13.6, 8.6)]);
            path(
                painter,
                &[(4.2, 12.4), (4.2, 16.2), (15.8, 16.2), (15.8, 12.4)],
            );
        }
        Icon::Copy => {
            rounded(
                painter,
                Rect::from_min_max(point(6.8, 6.8), point(16.6, 16.6)),
                2.0,
            );
            path(
                painter,
                &[
                    (12.8, 6.8),
                    (12.8, 4.6),
                    (4.6, 4.6),
                    (4.6, 12.8),
                    (6.8, 12.8),
                ],
            );
        }
        Icon::Camera => {
            rounded(
                painter,
                Rect::from_min_max(point(2.6, 6.4), point(17.4, 16.2)),
                2.4,
            );
            path(painter, &[(6.6, 6.4), (7.6, 4.6), (12.4, 4.6), (13.4, 6.4)]);
            arc(painter, 10.0, 11.2, 3.2, 0.0, TAU);
            dot(painter, 14.6, 8.2, 0.8);
        }
        Icon::Film => {
            rounded(
                painter,
                Rect::from_min_max(point(2.8, 4.8), point(17.2, 15.2)),
                2.2,
            );
            line(painter, (6.8, 4.8), (6.8, 15.2));
            line(painter, (13.2, 4.8), (13.2, 15.2));
            for y in [7.0, 10.0, 13.0] {
                dot(painter, 15.0, y, 0.7);
            }
        }
        Icon::Export => {
            line(painter, (10.0, 12.8), (10.0, 3.6));
            path(painter, &[(6.4, 7.2), (10.0, 3.6), (13.6, 7.2)]);
            path(
                painter,
                &[(4.2, 12.0), (4.2, 16.2), (15.8, 16.2), (15.8, 12.0)],
            );
        }
        Icon::Trash => {
            line(painter, (3.8, 6.2), (16.2, 6.2));
            path(painter, &[(7.6, 6.2), (7.6, 4.2), (12.4, 4.2), (12.4, 6.2)]);
            path(
                painter,
                &[(5.2, 6.2), (6.0, 16.4), (14.0, 16.4), (14.8, 6.2)],
            );
            line(painter, (8.4, 9.0), (8.6, 13.6));
            line(painter, (11.6, 9.0), (11.4, 13.6));
        }
        Icon::Cube => {
            path(
                painter,
                &[
                    (10.0, 3.2),
                    (17.0, 7.0),
                    (10.0, 10.8),
                    (3.0, 7.0),
                    (10.0, 3.2),
                ],
            );
            path(
                painter,
                &[
                    (3.0, 7.0),
                    (3.0, 13.8),
                    (10.0, 17.0),
                    (17.0, 13.8),
                    (17.0, 7.0),
                ],
            );
            line(painter, (10.0, 10.8), (10.0, 17.0));
        }
        Icon::Section => {
            rounded(
                painter,
                Rect::from_min_max(point(3.2, 4.0), point(16.8, 16.0)),
                2.2,
            );
            path(painter, &[(5.2, 12.8), (14.8, 7.2)]);
            line(painter, (5.2, 12.8), (5.2, 14.6));
            line(painter, (14.8, 7.2), (14.8, 5.4));
            line(painter, (9.4, 10.6), (11.2, 9.6));
        }
        Icon::Fit => {
            path(painter, &[(3.4, 7.6), (3.4, 3.4), (7.6, 3.4)]);
            path(painter, &[(12.4, 3.4), (16.6, 3.4), (16.6, 7.6)]);
            path(painter, &[(16.6, 12.4), (16.6, 16.6), (12.4, 16.6)]);
            path(painter, &[(7.6, 16.6), (3.4, 16.6), (3.4, 12.4)]);
            dot(painter, 10.0, 10.0, 1.1);
        }
        Icon::Reset => {
            let start = PI * 0.28;
            arc(painter, 10.0, 10.0, 6.4, start, PI * 1.66);
            let end = start + PI * 1.66;
            let tip = point(10.0 + 6.4 * end.cos(), 10.0 + 6.4 * end.sin());
            let tangent = Vec2::new(-end.sin(), end.cos()) * 2.6 * scale;
            let normal = Vec2::new(end.cos(), end.sin()) * 2.6 * scale;
            painter.segment(tip, tip + tangent + normal, stroke, color);
            painter.segment(tip, tip + tangent - normal, stroke, color);
        }
        Icon::Layers => {
            path(
                painter,
                &[
                    (10.0, 3.4),
                    (17.0, 7.0),
                    (10.0, 10.6),
                    (3.0, 7.0),
                    (10.0, 3.4),
                ],
            );
            path(painter, &[(3.0, 11.0), (10.0, 14.6), (17.0, 11.0)]);
            path(painter, &[(3.0, 14.4), (10.0, 18.0), (17.0, 14.4)]);
        }
        Icon::Target => {
            arc(painter, 10.0, 10.0, 6.2, 0.0, TAU);
            arc(painter, 10.0, 10.0, 2.0, 0.0, TAU);
            line(painter, (10.0, 1.8), (10.0, 4.8));
            line(painter, (10.0, 15.2), (10.0, 18.2));
            line(painter, (1.8, 10.0), (4.8, 10.0));
            line(painter, (15.2, 10.0), (18.2, 10.0));
        }
        Icon::Search => {
            arc(painter, 8.8, 8.8, 5.2, 0.0, TAU);
            line(painter, (12.8, 12.8), (16.8, 16.8));
        }
        Icon::Globe => {
            arc(painter, 10.0, 10.0, 7.2, 0.0, TAU);
            line(painter, (2.8, 10.0), (17.2, 10.0));
            arc(painter, 10.0, 10.0, 3.6, -0.5 * PI, PI);
            arc(painter, 10.0, 10.0, 3.6, 0.5 * PI, PI);
        }
        Icon::Sliders => {
            for (y, knob) in [(5.8, 7.4), (10.0, 13.2), (14.2, 8.8)] {
                line(painter, (3.0, y), (17.0, y));
                painter.rect(
                    Rect::from_center_size(point(knob, y), Vec2::splat(4.2 * scale)),
                    Corners::capsule(),
                    color,
                );
            }
        }
        Icon::Table => {
            rounded(
                painter,
                Rect::from_min_max(point(3.0, 4.2), point(17.0, 15.8)),
                2.0,
            );
            line(painter, (3.0, 8.2), (17.0, 8.2));
            line(painter, (8.4, 8.2), (8.4, 15.8));
            line(painter, (12.6, 8.2), (12.6, 15.8));
        }
        Icon::Grid => {
            for (x, y) in [(3.0, 3.0), (11.0, 3.0), (3.0, 11.0), (11.0, 11.0)] {
                painter.rect(
                    Rect::from_min_max(point(x, y), point(x + 6.0, y + 6.0)),
                    Corners::all(1.6 * scale),
                    color,
                );
            }
        }
        Icon::Sparkles => {
            path(
                painter,
                &[
                    (9.0, 2.8),
                    (10.9, 8.1),
                    (16.2, 10.0),
                    (10.9, 11.9),
                    (9.0, 17.2),
                    (7.1, 11.9),
                    (1.8, 10.0),
                    (7.1, 8.1),
                    (9.0, 2.8),
                ],
            );
            dot(painter, 16.4, 4.4, 1.3);
        }
        Icon::Info => {
            arc(painter, 10.0, 10.0, 7.0, 0.0, TAU);
            dot(painter, 10.0, 6.4, 0.9);
            line(painter, (10.0, 9.2), (10.0, 14.0));
        }
        Icon::Warning => {
            path(
                painter,
                &[(10.0, 3.0), (17.8, 16.8), (2.2, 16.8), (10.0, 3.0)],
            );
            line(painter, (10.0, 7.4), (10.0, 12.0));
            dot(painter, 10.0, 14.4, 0.9);
        }
        Icon::Clock => {
            arc(painter, 10.0, 10.0, 7.0, 0.0, TAU);
            line(painter, (10.0, 6.2), (10.0, 10.2));
            line(painter, (10.0, 10.2), (13.0, 12.0));
        }
        Icon::Lock => {
            rounded(
                painter,
                Rect::from_min_max(point(4.8, 9.0), point(15.2, 17.0)),
                2.0,
            );
            arc(painter, 10.0, 8.6, 3.6, PI, PI);
            line(painter, (6.4, 8.6), (6.4, 9.6));
            line(painter, (13.6, 8.6), (13.6, 9.6));
        }
        Icon::Channel => {
            for (y, length) in [(5.4, 7.0), (10.0, 10.0), (14.6, 13.0)] {
                painter.segment(point(3.4, y), point(3.4 + length, y), 2.4, color);
            }
            line(painter, (3.4, 3.0), (3.4, 17.0));
        }
        Icon::Filter => {
            path(
                painter,
                &[
                    (3.2, 4.4),
                    (16.8, 4.4),
                    (11.6, 10.4),
                    (11.6, 16.0),
                    (8.4, 14.2),
                    (8.4, 10.4),
                    (3.2, 4.4),
                ],
            );
        }
        Icon::Columns => {
            rounded(
                painter,
                Rect::from_min_max(point(3.0, 4.0), point(17.0, 16.0)),
                2.0,
            );
            line(painter, (8.4, 4.0), (8.4, 16.0));
            line(painter, (12.6, 4.0), (12.6, 16.0));
        }
        Icon::Sidebar => {
            rounded(
                painter,
                Rect::from_min_max(point(3.0, 4.0), point(17.0, 16.0)),
                2.4,
            );
            line(painter, (8.2, 4.0), (8.2, 16.0));
            for y in [7.4, 10.0, 12.6] {
                line(painter, (5.0, y), (6.4, y));
            }
        }
        Icon::Undo => {
            painter.triangle(point(2.9, 8.0), point(7.2, 4.5), point(7.2, 11.5), color);
            line(painter, (7.2, 8.0), (12.6, 8.0));
            arc(painter, 12.6, 11.6, 3.6, PI * 1.5, PI * 0.5);
            line(painter, (16.2, 11.6), (16.2, 15.4));
        }
        Icon::Redo => {
            painter.triangle(point(17.1, 8.0), point(12.8, 4.5), point(12.8, 11.5), color);
            line(painter, (12.8, 8.0), (7.4, 8.0));
            arc(painter, 7.4, 11.6, 3.6, PI, PI * 0.5);
            line(painter, (3.8, 11.6), (3.8, 15.4));
        }
        Icon::Pencil => {
            path(
                painter,
                &[
                    (13.4, 3.6),
                    (16.4, 6.6),
                    (7.6, 15.4),
                    (4.0, 16.0),
                    (4.6, 12.4),
                    (13.4, 3.6),
                ],
            );
            line(painter, (11.6, 5.4), (14.6, 8.4));
        }
        Icon::Ruler => {
            line(painter, (3.2, 12.2), (16.8, 12.2));
            line(painter, (3.2, 10.0), (3.2, 14.4));
            line(painter, (16.8, 10.0), (16.8, 14.4));
            for x in [6.6, 10.0, 13.4] {
                line(painter, (x, 12.2), (x, 15.0));
            }
            line(painter, (10.0, 9.0), (10.0, 12.2));
        }
        Icon::Sphere => {
            arc(painter, 10.0, 10.0, 6.6, 0.0, TAU);
            line(painter, (3.4, 10.0), (16.6, 10.0));
            arc(painter, 10.0, 10.0, 3.1, 0.0, TAU);
        }
    }
}
