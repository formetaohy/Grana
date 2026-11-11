use glam::Vec2;
use grana_measure::Lasso;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrushKind {
    Rectangle,
    Lasso,
}

#[derive(Clone, Copy, Debug)]
pub struct Brush<'a> {
    pub kind: BrushKind,
    pub from: Vec2,
    pub to: Vec2,
    pub points: &'a [Vec2],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Highlight {
    Value(f32),
    Point(Vec2),
    Group { index: usize, value: f32 },
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Gates {
    pub x: Option<(f32, f32)>,
    pub y: Option<(f32, f32)>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Decor<'a> {
    pub gates: Gates,
    pub highlight: Option<Highlight>,
    pub pointer: Option<Vec2>,
    pub lasso: Option<&'a Lasso>,
    pub brush: Option<Brush<'a>>,
    pub sparse: bool,
}
