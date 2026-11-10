use swash::zeno::{PathBuilder, PathData, Point};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Curve {
    Line([f32; 2]),
    Quad([f32; 2], [f32; 2]),
    Cubic([f32; 2], [f32; 2], [f32; 2]),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Contour {
    start: [f32; 2],
    curves: Vec<Curve>,
}

impl Contour {
    pub fn new(start: [f32; 2]) -> Self {
        assert!(
            start.iter().all(|value| value.is_finite()),
            "a contour starts at a finite point"
        );
        Self {
            start,
            curves: Vec::new(),
        }
    }

    pub fn start(&self) -> [f32; 2] {
        self.start
    }

    pub fn curves(&self) -> &[Curve] {
        &self.curves
    }

    pub fn end(&self) -> [f32; 2] {
        match self.curves.last() {
            None => self.start,
            Some(Curve::Line(to)) => *to,
            Some(Curve::Quad(_, to)) => *to,
            Some(Curve::Cubic(_, _, to)) => *to,
        }
    }

    pub fn closed(&self) -> bool {
        let end = self.end();
        (end[0] - self.start[0]).abs() < 1.0e-4 && (end[1] - self.start[1]).abs() < 1.0e-4
    }

    pub fn push(&mut self, curve: Curve) {
        self.curves.push(curve);
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Outline {
    contours: Vec<Contour>,
}

impl Outline {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn contours(&self) -> &[Contour] {
        &self.contours
    }

    pub fn is_empty(&self) -> bool {
        self.contours.is_empty()
    }

    pub fn absorb(&mut self, other: Outline) {
        self.contours.extend(other.contours);
    }

    pub fn into_contours(self) -> Vec<Contour> {
        self.contours
    }

    pub fn transform(&mut self, map: impl Fn([f32; 2]) -> [f32; 2]) -> &mut Self {
        for contour in &mut self.contours {
            contour.start = map(contour.start);
            for curve in &mut contour.curves {
                *curve = match *curve {
                    Curve::Line(to) => Curve::Line(map(to)),
                    Curve::Quad(control, to) => Curve::Quad(map(control), map(to)),
                    Curve::Cubic(first, second, to) => {
                        Curve::Cubic(map(first), map(second), map(to))
                    }
                };
            }
        }
        self
    }
}

#[derive(Default)]
struct Builder {
    outline: Outline,
}

impl PathBuilder for Builder {
    fn current_point(&self) -> Point {
        self.outline
            .contours
            .last()
            .map(|contour| match contour.curves.last() {
                None => contour.start,
                Some(Curve::Line(to)) => *to,
                Some(Curve::Quad(_, to)) => *to,
                Some(Curve::Cubic(_, _, to)) => *to,
            })
            .unwrap_or([0.0, 0.0])
            .into()
    }

    fn move_to(&mut self, to: impl Into<Point>) -> &mut Self {
        self.outline.contours.push(Contour::new(point(to)));
        self
    }

    fn line_to(&mut self, to: impl Into<Point>) -> &mut Self {
        if let Some(contour) = self.outline.contours.last_mut() {
            contour.push(Curve::Line(point(to)));
        }
        self
    }

    fn quad_to(&mut self, control: impl Into<Point>, to: impl Into<Point>) -> &mut Self {
        if let Some(contour) = self.outline.contours.last_mut() {
            contour.push(Curve::Quad(point(control), point(to)));
        }
        self
    }

    fn curve_to(
        &mut self,
        first: impl Into<Point>,
        second: impl Into<Point>,
        to: impl Into<Point>,
    ) -> &mut Self {
        if let Some(contour) = self.outline.contours.last_mut() {
            contour.push(Curve::Cubic(point(first), point(second), point(to)));
        }
        self
    }

    fn close(&mut self) -> &mut Self {
        self
    }
}

fn point(value: impl Into<Point>) -> [f32; 2] {
    value.into().into()
}

pub(crate) fn collect(path: impl PathData) -> Outline {
    let mut builder = Builder::default();
    path.copy_to(&mut builder);
    builder.outline
}
