use std::fmt;

use grana_profile::Profile;

use crate::state::{HASH_OFFSET, mix};

const LABELS: [&str; 2] = ["Point", "Distance"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AnnotationTag {
    Point,
    Distance,
}

impl AnnotationTag {
    pub fn index(self) -> usize {
        match self {
            Self::Point => 0,
            Self::Distance => 1,
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

    pub fn tag(self) -> AnnotationTag {
        match self.shape {
            Shape::Point(_) => AnnotationTag::Point,
            Shape::Distance(..) => AnnotationTag::Distance,
        }
    }

    pub fn anchor(self) -> [f32; 3] {
        match self.shape {
            Shape::Point(at) | Shape::Distance(at, _) => at,
        }
    }

    pub fn end(self) -> Option<[f32; 3]> {
        match self.shape {
            Shape::Distance(_, to) => Some(to),
            Shape::Point(_) => None,
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
            Shape::Point(_) => None,
        }
    }

    pub fn as_point(self) -> Option<[f32; 3]> {
        match self.shape {
            Shape::Point(at) => Some(at),
            Shape::Distance(..) => None,
        }
    }

    pub fn translated(self, offset: [f32; 3]) -> Self {
        match self.shape {
            Shape::Point(at) => Self::point(add(at, offset)),
            Shape::Distance(from, to) => Self::distance(add(from, offset), add(to, offset)),
        }
    }

    pub fn with_anchor(self, anchor: [f32; 3]) -> Self {
        match self.shape {
            Shape::Point(_) => Self::point(anchor),
            Shape::Distance(_, to) => Self::distance(anchor, to),
        }
    }

    pub fn with_end(self, end: [f32; 3]) -> Self {
        match self.shape {
            Shape::Point(_) => Self::point(end),
            Shape::Distance(from, _) => Self::distance(from, end),
        }
    }
}

fn add(point: [f32; 3], offset: [f32; 3]) -> [f32; 3] {
    [0, 1, 2].map(|axis| point[axis] + offset[axis])
}

fn finite(point: [f32; 3]) -> bool {
    point.iter().all(|value| value.is_finite())
}

#[derive(Clone, Debug, PartialEq)]
pub struct Annotation {
    name: String,
    visible: bool,
    frame: Option<u32>,
    kind: AnnotationKind,
    profile: Option<Profile>,
}

impl Annotation {
    pub fn new(name: impl Into<String>, frame: Option<u32>, kind: AnnotationKind) -> Self {
        let name = name.into();
        assert!(
            !name.trim().is_empty(),
            "an annotation carries a name for its label"
        );
        Self {
            name,
            visible: true,
            frame,
            kind,
            profile: None,
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

    pub fn frame(&self) -> Option<u32> {
        self.frame
    }

    pub fn kind(&self) -> AnnotationKind {
        self.kind
    }

    pub fn profile(&self) -> Option<&Profile> {
        self.profile.as_ref()
    }

    pub fn set_profile(&mut self, profile: Option<Profile>) -> bool {
        if let Some(profile) = &profile {
            assert_eq!(
                self.kind.tag(),
                AnnotationTag::Distance,
                "a profile follows a distance annotation"
            );
            assert!(
                self.kind.anchor() == profile.line().from()
                    && self.kind.end() == Some(profile.line().to()),
                "a profile follows the line of its annotation"
            );
        }
        if self.profile == profile {
            return false;
        }
        self.profile = profile;
        true
    }

    pub fn shown_at(&self, time: u32) -> bool {
        self.visible && self.frame.is_none_or(|frame| frame == time)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AnnotationState {
    pub entries: Vec<Annotation>,
    pub selected: Option<usize>,
    pub counts: [u32; 2],
}

#[derive(Clone, Debug, PartialEq)]
pub struct Annotations {
    entries: Vec<Annotation>,
    selected: Option<usize>,
    counts: [u32; 2],
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
            counts: [0; 2],
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

    pub fn add_point(&mut self, at: [f32; 3], frame: Option<u32>) -> usize {
        self.push(AnnotationKind::point(at), frame)
    }

    pub fn add_distance(&mut self, from: [f32; 3], to: [f32; 3], frame: Option<u32>) -> usize {
        self.push(AnnotationKind::distance(from, to), frame)
    }

    pub fn add(&mut self, name: &str, frame: Option<u32>, kind: AnnotationKind) -> usize {
        self.entries.push(Annotation::new(name, frame, kind));
        self.entries.len() - 1
    }

    fn push(&mut self, kind: AnnotationKind, frame: Option<u32>) -> usize {
        let tag = kind.tag().index();
        let count = self.counts[tag];
        self.counts[tag] += 1;
        let name = format!("{} {}", kind.tag().label(), count + 1);
        self.add(&name, frame, kind)
    }

    pub fn remove(&mut self, index: usize) -> bool {
        if index >= self.entries.len() {
            return false;
        }
        self.entries.remove(index);
        self.selected = shift(self.selected, index);
        self.selected = self.selected.filter(|index| *index < self.entries.len());
        true
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.selected = None;
        self.counts = [0; 2];
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

    pub fn set_frame(&mut self, index: usize, frame: Option<u32>) -> bool {
        let Some(entry) = self.entries.get_mut(index) else {
            return false;
        };
        if entry.frame == frame {
            return false;
        }
        entry.frame = frame;
        true
    }

    pub fn set_profile(&mut self, index: usize, profile: Option<Profile>) -> bool {
        let Some(entry) = self.entries.get_mut(index) else {
            return false;
        };
        entry.set_profile(profile)
    }

    pub fn set_kind(&mut self, index: usize, kind: AnnotationKind) -> bool {
        let Some(entry) = self.entries.get_mut(index) else {
            return false;
        };
        assert_eq!(
            entry.kind.tag(),
            kind.tag(),
            "an annotation keeps the shape it was created with"
        );
        if entry.kind == kind {
            return false;
        }
        entry.kind = kind;
        entry.profile = None;
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

    pub fn document(&self) -> u64 {
        let mut hash = HASH_OFFSET;
        for entry in &self.entries {
            for byte in entry.name.bytes() {
                hash = mix(hash, u64::from(byte));
            }
            hash = mix(hash, u64::from(u8::from(entry.visible)));
            hash = mix(hash, entry.frame.map_or(u64::MAX, u64::from));
            hash = mix(hash, entry.kind.tag().index() as u64);
            for value in entry.kind.anchor() {
                hash = mix(hash, u64::from(value.to_bits()));
            }
            if let Some(end) = entry.kind.end() {
                for value in end {
                    hash = mix(hash, u64::from(value.to_bits()));
                }
            }
            hash = mix(hash, u64::from(entry.profile.is_some()));
            if let Some(profile) = &entry.profile {
                hash = mix(hash, u64::from(profile.spacing().to_bits()));
                for name in profile.names() {
                    for byte in name.bytes() {
                        hash = mix(hash, u64::from(byte));
                    }
                }
                hash = mix(hash, profile.samples().len() as u64);
                for sample in profile.samples() {
                    hash = mix(hash, u64::from(sample.offset().to_bits()));
                    for value in sample.values() {
                        hash = mix(hash, u64::from(value.to_bits()));
                    }
                }
            }
        }
        hash
    }

    pub fn state(&self) -> AnnotationState {
        AnnotationState {
            entries: self.entries.clone(),
            selected: self.selected,
            counts: self.counts,
        }
    }

    pub fn restore(&mut self, state: AnnotationState) {
        let AnnotationState {
            entries,
            selected,
            counts,
        } = state;
        assert!(
            selected.is_none_or(|index| index < entries.len()),
            "the selected annotation belongs to the scene"
        );
        self.entries = entries;
        self.selected = selected;
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
