use grana_display::{OutputSize, PANE_COUNT, PANE_GAP, PaneKind, Panes};
use grana_ui::{Rect, Vec2};

const STAGE_WIDTH: f32 = 1000.0;

#[derive(Clone, Copy, Debug)]
pub struct Panels {
    panes: [Option<(Rect, PaneKind)>; PANE_COUNT],
}

#[derive(Clone, Copy, Debug)]
pub struct PaneRegion {
    pub kind: PaneKind,
    pub region: [u32; 4],
    pub points: [f32; 4],
}

#[derive(Clone, Copy, Debug)]
pub struct Regions {
    pub size: [u32; 2],
    pub pixels_per_point: f32,
    pub panes: [Option<PaneRegion>; PANE_COUNT],
}

impl Panels {
    pub fn new(panes: [Option<(Rect, PaneKind)>; PANE_COUNT]) -> Self {
        Self {
            panes: panes
                .map(|pane| pane.filter(|(rect, _)| rect.width() > 0.0 && rect.height() > 0.0)),
        }
    }

    pub fn of(panes: &Panes, size: [u32; 2]) -> Self {
        assert!(
            size[0] > 0 && size[1] > 0,
            "a stage covers at least one pixel"
        );
        let scale = size[0] as f32 / STAGE_WIDTH;
        let stage = [0.0, 0.0, STAGE_WIDTH, size[1] as f32 / scale];
        let rects = panes.rects(stage, PANE_GAP);
        Self::new(std::array::from_fn(|index| {
            panes.visible(index).then(|| {
                (
                    Rect::from_min_size(
                        Vec2::new(rects[index][0], rects[index][1]),
                        Vec2::new(rects[index][2], rects[index][3]),
                    ),
                    panes.pane(index).kind(),
                )
            })
        }))
    }

    pub fn pane(&self, index: usize) -> Option<(Rect, PaneKind)> {
        self.panes[index]
    }

    pub fn panes(&self) -> impl Iterator<Item = (usize, Rect, PaneKind)> {
        self.panes
            .iter()
            .enumerate()
            .filter_map(|(index, pane)| pane.map(|(rect, kind)| (index, rect, kind)))
    }

    pub fn stage(&self) -> Rect {
        self.panes
            .iter()
            .flatten()
            .fold(Rect::ZERO, |stage, (rect, _)| {
                if stage.width() <= 0.0 && stage.height() <= 0.0 {
                    *rect
                } else {
                    Rect::from_min_max(stage.min.min(rect.min), stage.max.max(rect.max))
                }
            })
    }

    pub fn native_width(&self, scale: f32) -> u32 {
        (self.stage().width() * scale).round().max(1.0) as u32
    }

    pub fn output(&self, width: u32, even: bool) -> OutputSize {
        assert!(width > 0, "an export names a positive image width");
        let stage = self.stage();
        assert!(
            stage.width() > 0.0 && stage.height() > 0.0,
            "an export covers at least one pane"
        );
        let height = (f64::from(width) * f64::from(stage.height()) / f64::from(stage.width()))
            .round()
            .max(1.0) as u32;
        let size = OutputSize::of(width, height);
        if even { size.even() } else { size }
    }

    pub fn screen(&self, scale: f32, size: [u32; 2]) -> Regions {
        self.regions(scale, [0.0, 0.0], size)
    }

    pub fn export(&self, size: OutputSize) -> Regions {
        let stage = self.stage();
        assert!(
            stage.width() > 0.0 && stage.height() > 0.0,
            "an export covers at least one pane"
        );
        let scale = size.width() as f32 / stage.width();
        self.regions(scale, [stage.min.x, stage.min.y], size.to_array())
    }

    fn regions(&self, scale: f32, origin: [f32; 2], size: [u32; 2]) -> Regions {
        Regions {
            size,
            pixels_per_point: scale,
            panes: self.panes.map(|pane| {
                pane.map(|(rect, kind)| PaneRegion {
                    kind,
                    region: region(rect, scale, origin, size),
                    points: points(rect, origin),
                })
            }),
        }
    }
}

fn points(rect: Rect, origin: [f32; 2]) -> [f32; 4] {
    [
        rect.min.x - origin[0],
        rect.min.y - origin[1],
        rect.width(),
        rect.height(),
    ]
}

fn region(rect: Rect, scale: f32, origin: [f32; 2], size: [u32; 2]) -> [u32; 4] {
    let x = (((rect.min.x - origin[0]) * scale).round() as i64).clamp(0, size[0] as i64 - 1) as u32;
    let y = (((rect.min.y - origin[1]) * scale).round() as i64).clamp(0, size[1] as i64 - 1) as u32;
    let width = ((rect.width() * scale).round() as u32)
        .min(size[0] - x)
        .max(1);
    let height = ((rect.height() * scale).round() as u32)
        .min(size[1] - y)
        .max(1);
    [x, y, width, height]
}
