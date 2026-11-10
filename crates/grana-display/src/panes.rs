use glam::Vec3;

use crate::{Camera, CameraState, SectionCamera, SectionPreset, SectionState};

pub const PANE_COUNT: usize = 4;
pub const SPLIT_MIN: f32 = 0.25;
pub const SPLIT_MAX: f32 = 0.8;
pub const PANE_GAP: f32 = 6.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PaneLayout {
    Single,
    #[default]
    Split,
    Quad,
}

impl PaneLayout {
    pub const ALL: [Self; 3] = [Self::Single, Self::Split, Self::Quad];

    pub fn name(self) -> &'static str {
        match self {
            Self::Single => "single",
            Self::Split => "split",
            Self::Quad => "quad",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Single => "One View",
            Self::Split => "Two Views",
            Self::Quad => "Four Views",
        }
    }

    pub fn index(self) -> usize {
        match self {
            Self::Single => 0,
            Self::Split => 1,
            Self::Quad => 2,
        }
    }

    pub fn from_index(index: usize) -> Option<Self> {
        Self::ALL.get(index).copied()
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|layout| layout.name() == name)
    }

    pub fn count(self) -> usize {
        match self {
            Self::Single => 1,
            Self::Split => 2,
            Self::Quad => PANE_COUNT,
        }
    }

    pub fn visible(self, index: usize) -> bool {
        index < self.count()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PaneKind {
    #[default]
    Volume,
    Section,
}

impl PaneKind {
    pub const ALL: [Self; 2] = [Self::Volume, Self::Section];

    pub fn name(self) -> &'static str {
        match self {
            Self::Volume => "volume",
            Self::Section => "section",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Volume => "3D",
            Self::Section => "Slice",
        }
    }

    pub fn index(self) -> usize {
        match self {
            Self::Volume => 0,
            Self::Section => 1,
        }
    }

    pub fn from_index(index: usize) -> Option<Self> {
        Self::ALL.get(index).copied()
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.name() == name)
    }

    pub fn volume(self) -> bool {
        self == Self::Volume
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaneView {
    pub camera: CameraState,
    pub section: SectionState,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaneState {
    pub kind: PaneKind,
    pub view: PaneView,
}

#[derive(Clone, Debug)]
pub struct Pane {
    kind: PaneKind,
    camera: Camera,
    section: SectionCamera,
}

impl Pane {
    pub fn new(kind: PaneKind) -> Self {
        Self {
            kind,
            camera: Camera::new(35.0),
            section: SectionCamera::new(),
        }
    }

    pub fn sliced(preset: SectionPreset) -> Self {
        let mut pane = Self::new(PaneKind::Section);
        preset.apply(&mut pane.section);
        pane
    }

    pub fn kind(&self) -> PaneKind {
        self.kind
    }

    pub fn set_kind(&mut self, kind: PaneKind) {
        self.kind = kind;
    }

    pub fn camera(&self) -> &Camera {
        &self.camera
    }

    pub fn camera_mut(&mut self) -> &mut Camera {
        &mut self.camera
    }

    pub fn section(&self) -> &SectionCamera {
        &self.section
    }

    pub fn section_mut(&mut self) -> &mut SectionCamera {
        &mut self.section
    }

    pub fn view(&self) -> PaneView {
        PaneView {
            camera: self.camera.state(),
            section: self.section.state(),
        }
    }

    pub fn state(&self) -> PaneState {
        PaneState {
            kind: self.kind,
            view: self.view(),
        }
    }

    pub fn restore(&mut self, state: PaneState) {
        self.kind = state.kind;
        self.restore_view(state.view);
    }

    pub fn restore_view(&mut self, view: PaneView) {
        self.camera.restore(view.camera);
        self.section.restore(view.section);
    }

    pub fn fit(&mut self, world: Vec3, voxel: Vec3, aspect: f32) {
        self.camera.fit(world, aspect);
        self.section.fit(world, voxel, aspect);
    }
}

#[derive(Clone, Debug)]
pub struct Panes {
    panes: [Pane; PANE_COUNT],
    layout: PaneLayout,
    split: f32,
    row: f32,
}

impl Default for Panes {
    fn default() -> Self {
        Self::new()
    }
}

impl Panes {
    pub fn new() -> Self {
        Self {
            panes: [
                Pane::new(PaneKind::Volume),
                Pane::sliced(SectionPreset::Xy),
                Pane::sliced(SectionPreset::Xz),
                Pane::sliced(SectionPreset::Yz),
            ],
            layout: PaneLayout::Split,
            split: 0.62,
            row: 0.5,
        }
    }

    pub fn pane(&self, index: usize) -> &Pane {
        assert!(index < PANE_COUNT, "a pane index names one of the panes");
        &self.panes[index]
    }

    pub fn pane_mut(&mut self, index: usize) -> &mut Pane {
        assert!(index < PANE_COUNT, "a pane index names one of the panes");
        &mut self.panes[index]
    }

    pub fn panes(&self) -> &[Pane; PANE_COUNT] {
        &self.panes
    }

    pub fn iter(&self) -> impl Iterator<Item = &Pane> {
        self.panes.iter()
    }

    pub fn layout(&self) -> PaneLayout {
        self.layout
    }

    pub fn set_layout(&mut self, layout: PaneLayout) {
        self.layout = layout;
    }

    pub fn count(&self) -> usize {
        self.layout.count()
    }

    pub fn visible(&self, index: usize) -> bool {
        self.layout.visible(index)
    }

    pub fn split(&self) -> f32 {
        self.split
    }

    pub fn set_split(&mut self, split: f32) {
        assert!(split.is_finite(), "a pane split is a finite fraction");
        self.split = split.clamp(SPLIT_MIN, SPLIT_MAX);
    }

    pub fn row(&self) -> f32 {
        self.row
    }

    pub fn set_row(&mut self, row: f32) {
        assert!(row.is_finite(), "a pane row is a finite fraction");
        self.row = row.clamp(SPLIT_MIN, SPLIT_MAX);
    }

    pub fn rects(&self, stage: [f32; 4], gap: f32) -> [[f32; 4]; PANE_COUNT] {
        assert!(
            stage[2] > 0.0 && stage[3] > 0.0,
            "a pane layout covers a non empty stage"
        );
        assert!(gap >= 0.0, "a pane layout keeps a non negative gap");
        let [x, y, width, height] = stage;
        let mut rects = [[0.0; 4]; PANE_COUNT];
        match self.layout {
            PaneLayout::Single => rects[0] = stage,
            PaneLayout::Split => {
                let columns = columns(x, width, gap, self.split);
                for (rect, (left, width)) in rects.iter_mut().zip(columns) {
                    *rect = [left, y, width, height];
                }
            }
            PaneLayout::Quad => {
                let columns = columns(x, width, gap, self.split);
                let rows = rows(y, height, gap, self.row);
                for (index, rect) in rects.iter_mut().enumerate() {
                    let (left, width) = columns[index % 2];
                    let (top, height) = rows[index / 2];
                    *rect = [left, top, width, height];
                }
            }
        }
        rects
    }

    pub fn states(&self) -> [PaneState; PANE_COUNT] {
        std::array::from_fn(|index| self.panes[index].state())
    }

    pub fn views(&self) -> [PaneView; PANE_COUNT] {
        std::array::from_fn(|index| self.panes[index].view())
    }

    pub fn restore(&mut self, states: [PaneState; PANE_COUNT]) {
        for (pane, state) in self.panes.iter_mut().zip(states) {
            pane.restore(state);
        }
    }

    pub fn restore_views(&mut self, views: [PaneView; PANE_COUNT]) {
        for (pane, view) in self.panes.iter_mut().zip(views) {
            pane.restore_view(view);
        }
    }

    pub fn fit(&mut self, world: Vec3, voxel: Vec3, aspects: [f32; PANE_COUNT]) {
        for (index, aspect) in aspects.into_iter().enumerate() {
            if self.visible(index) {
                self.panes[index].fit(world, voxel, aspect);
            }
        }
    }

    pub fn fit_pane(&mut self, index: usize, world: Vec3, voxel: Vec3, aspect: f32) {
        self.pane_mut(index).fit(world, voxel, aspect);
    }

    pub fn clamp_focus(&mut self, half: Vec3) {
        for pane in &mut self.panes {
            pane.camera_mut().clamp_focus(half);
        }
    }
}

fn columns(x: f32, width: f32, gap: f32, split: f32) -> [(f32, f32); 2] {
    let usable = (width - gap).max(2.0);
    let first = (usable * split).round().clamp(1.0, usable - 1.0);
    [(x, first), (x + first + gap, usable - first)]
}

fn rows(y: f32, height: f32, gap: f32, row: f32) -> [(f32, f32); 2] {
    let usable = (height - gap).max(2.0);
    let first = (usable * row).round().clamp(1.0, usable - 1.0);
    [(y, first), (y + first + gap, usable - first)]
}
