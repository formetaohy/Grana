use std::fmt;

use grana_measure::Table;

use crate::state::{HASH_OFFSET, mix};

const LABELS: [&str; 3] = ["Point", "Distance", "ROI"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AnnotationTag {
    Point,
    Distance,
    Roi,
}

impl AnnotationTag {
    pub fn index(self) -> usize {
        match self {
            Self::Point => 0,
            Self::Distance => 1,
            Self::Roi => 2,
        }
    }

    pub fn label(self) -> &'static str {
        LABELS[self.index()]
    }
}

#[derive(Clone, Copy, PartialEq)]
pub struct AnnotationKind {
    shape: Shape,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Shape {
    Point([f32; 3]),
    Distance([f32; 3], [f32; 3]),
    Roi([f32; 3], f32),
}

impl fmt::Debug for AnnotationKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.shape {
            Shape::Point(at) => formatter.debug_tuple("Point").field(&at).finish(),
            Shape::Distance(from, to) => formatter
                .debug_tuple("Distance")
                .field(&from)
                .field(&to)
                .finish(),
            Shape::Roi(center, radius) => formatter
                .debug_tuple("Roi")
                .field(&center)
                .field(&radius)
                .finish(),
        }
    }
}

impl AnnotationKind {
    pub fn point(at: [f32; 3]) -> Self {
        assert!(finite(at), "an annotation point lies in the volume");
        Self {
            shape: Shape::Point(at),
        }
    }

    pub fn distance(from: [f32; 3], to: [f32; 3]) -> Self {
        assert!(
            finite(from) && finite(to) && from != to,
            "an annotation distance spans two finite points"
        );
        Self {
            shape: Shape::Distance(from, to),
        }
    }

    pub fn roi(center: [f32; 3], radius: f32) -> Self {
        assert!(
            finite(center) && radius.is_finite() && radius > 0.0,
            "an annotation region grows by a positive radius"
        );
        Self {
            shape: Shape::Roi(center, radius),
        }
    }

    pub fn tag(self) -> AnnotationTag {
        match self.shape {
            Shape::Point(_) => AnnotationTag::Point,
            Shape::Distance(..) => AnnotationTag::Distance,
            Shape::Roi(..) => AnnotationTag::Roi,
        }
    }

    pub fn anchor(self) -> [f32; 3] {
        match self.shape {
            Shape::Point(at) | Shape::Roi(at, _) => at,
            Shape::Distance(from, _) => from,
        }
    }

    pub fn end(self) -> Option<[f32; 3]> {
        match self.shape {
            Shape::Distance(_, to) => Some(to),
            Shape::Point(_) | Shape::Roi(..) => None,
        }
    }

    pub fn radius(self) -> Option<f32> {
        match self.shape {
            Shape::Roi(_, radius) => Some(radius),
            Shape::Point(_) | Shape::Distance(..) => None,
        }
    }

    pub fn length(self) -> Option<f32> {
        match self.shape {
            Shape::Distance(from, to) => Some(
                (0..3)
                    .map(|axis| (to[axis] - from[axis]).powi(2))
                    .sum::<f32>()
                    .sqrt(),
            ),
            Shape::Point(_) | Shape::Roi(..) => None,
        }
    }

    pub fn as_point(self) -> Option<[f32; 3]> {
        match self.shape {
            Shape::Point(at) => Some(at),
            Shape::Distance(..) | Shape::Roi(..) => None,
        }
    }
}

fn finite(point: [f32; 3]) -> bool {
    point.iter().all(|value| value.is_finite())
}

#[derive(Clone, Debug, PartialEq)]
pub struct Annotation {
    name: String,
    visible: bool,
    kind: AnnotationKind,
}

impl Annotation {
    pub fn new(name: impl Into<String>, kind: AnnotationKind) -> Self {
        let name = name.into();
        assert!(
            !name.trim().is_empty(),
            "an annotation carries a name for its label"
        );
        Self {
            name,
            visible: true,
            kind,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn with_visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    pub fn visible(&self) -> bool {
        self.visible
    }

    pub fn kind(&self) -> AnnotationKind {
        self.kind
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Roi {
    center: [f32; 3],
    radius: f32,
}

impl Roi {
    pub fn new(center: [f32; 3], radius: f32) -> Self {
        assert!(
            finite(center) && radius.is_finite() && radius > 0.0,
            "a statistics region grows by a positive radius"
        );
        Self { center, radius }
    }

    pub fn center(self) -> [f32; 3] {
        self.center
    }

    pub fn radius(self) -> f32 {
        self.radius
    }

    pub fn holds(self, position: [f32; 3]) -> bool {
        let offset = [0, 1, 2].map(|axis| position[axis] - self.center[axis]);
        offset.iter().map(|value| value * value).sum::<f32>() <= self.radius * self.radius
    }

    pub fn holds_row(self, table: &Table, row: usize) -> bool {
        table
            .position(row)
            .is_none_or(|position| self.holds(position))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AnnotationState {
    pub entries: Vec<Annotation>,
    pub selected: Option<usize>,
    pub roi: Option<usize>,
    pub counts: [u32; 3],
}

#[derive(Clone, Debug, PartialEq)]
pub struct Annotations {
    entries: Vec<Annotation>,
    selected: Option<usize>,
    roi: Option<usize>,
    counts: [u32; 3],
}

impl Default for Annotations {
    fn default() -> Self {
        Self::new()
    }
}

impl Annotations {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            selected: None,
            roi: None,
            counts: [0; 3],
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Annotation> {
        self.entries.iter()
    }

    pub fn get(&self, index: usize) -> Option<&Annotation> {
        self.entries.get(index)
    }

    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    pub fn roi(&self) -> Option<usize> {
        self.roi
    }

    pub fn add_point(&mut self, at: [f32; 3]) -> usize {
        self.push(AnnotationKind::point(at))
    }

    pub fn add_distance(&mut self, from: [f32; 3], to: [f32; 3]) -> usize {
        self.push(AnnotationKind::distance(from, to))
    }

    pub fn add_roi(&mut self, center: [f32; 3], radius: f32) -> usize {
        self.push(AnnotationKind::roi(center, radius))
    }

    pub fn add(&mut self, name: &str, kind: AnnotationKind) -> usize {
        self.entries.push(Annotation::new(name, kind));
        self.entries.len() - 1
    }

    fn push(&mut self, kind: AnnotationKind) -> usize {
        let tag = kind.tag().index();
        let count = self.counts[tag];
        self.counts[tag] += 1;
        let name = format!("{} {}", kind.tag().label(), count + 1);
        self.add(&name, kind)
    }

    pub fn remove(&mut self, index: usize) -> bool {
        if index >= self.entries.len() {
            return false;
        }
        self.entries.remove(index);
        self.selected = shift(self.selected, index);
        self.roi = shift(self.roi, index);
        self.selected = self.selected.filter(|index| *index < self.entries.len());
        self.roi = self.roi.filter(|index| {
            self.entries
                .get(*index)
                .is_some_and(|entry| entry.kind.tag() == AnnotationTag::Roi)
        });
        true
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.selected = None;
        self.roi = None;
        self.counts = [0; 3];
    }

    pub fn select(&mut self, index: Option<usize>) -> bool {
        if index.is_some_and(|index| index >= self.entries.len()) {
            return false;
        }
        if self.selected == index {
            return false;
        }
        self.selected = index;
        true
    }

    pub fn set_visible(&mut self, index: usize, visible: bool) -> bool {
        let Some(entry) = self.entries.get_mut(index) else {
            return false;
        };
        if entry.visible == visible {
            return false;
        }
        entry.visible = visible;
        true
    }

    pub fn rename(&mut self, index: usize, name: &str) -> bool {
        if name.trim().is_empty() {
            return false;
        }
        let Some(entry) = self.entries.get_mut(index) else {
            return false;
        };
        if entry.name == name {
            return false;
        }
        entry.name = name.to_string();
        true
    }

    pub fn set_radius(&mut self, index: usize, radius: f32) -> bool {
        if !radius.is_finite() || radius <= 0.0 {
            return false;
        }
        let Some(entry) = self.entries.get_mut(index) else {
            return false;
        };
        let AnnotationKind { shape } = entry.kind;
        let Shape::Roi(center, current) = shape else {
            return false;
        };
        if current == radius {
            return false;
        }
        entry.kind = AnnotationKind::roi(center, radius);
        true
    }

    pub fn set_roi(&mut self, index: Option<usize>) -> bool {
        if let Some(index) = index
            && self
                .entries
                .get(index)
                .is_none_or(|entry| entry.kind.tag() != AnnotationTag::Roi)
        {
            return false;
        }
        if self.roi == index {
            return false;
        }
        self.roi = index;
        true
    }

    pub fn roi_geometry(&self) -> Option<Roi> {
        let kind = self.entries.get(self.roi?)?.kind;
        Some(Roi::new(kind.anchor(), kind.radius()?))
    }

    pub fn document(&self) -> u64 {
        let mut hash = HASH_OFFSET;
        for entry in &self.entries {
            for byte in entry.name.bytes() {
                hash = mix(hash, u64::from(byte));
            }
            hash = mix(hash, u64::from(u8::from(entry.visible)));
            hash = mix(hash, entry.kind.tag().index() as u64);
            for value in entry.kind.anchor() {
                hash = mix(hash, u64::from(value.to_bits()));
            }
            if let Some(end) = entry.kind.end() {
                for value in end {
                    hash = mix(hash, u64::from(value.to_bits()));
                }
            }
            if let Some(radius) = entry.kind.radius() {
                hash = mix(hash, u64::from(radius.to_bits()));
            }
        }
        hash = mix(hash, self.roi.map_or(u64::MAX, |index| index as u64));
        hash
    }

    pub fn state(&self) -> AnnotationState {
        AnnotationState {
            entries: self.entries.clone(),
            selected: self.selected,
            roi: self.roi,
            counts: self.counts,
        }
    }

    pub fn restore(&mut self, state: AnnotationState) {
        let AnnotationState {
            entries,
            selected,
            roi,
            counts,
        } = state;
        assert!(
            selected.is_none_or(|index| index < entries.len()),
            "the selected annotation belongs to the scene"
        );
        assert!(
            roi.is_none_or(|index| {
                entries
                    .get(index)
                    .is_some_and(|entry| entry.kind.tag() == AnnotationTag::Roi)
            }),
            "the statistics region names a region annotation"
        );
        self.entries = entries;
        self.selected = selected;
        self.roi = roi;
        self.counts = counts;
    }
}

fn shift(index: Option<usize>, removed: usize) -> Option<usize> {
    match index {
        Some(current) if current == removed => None,
        Some(current) if current > removed => Some(current - 1),
        other => other,
    }
}
