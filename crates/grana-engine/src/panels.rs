use grana_display::{PANE_COUNT, PaneKind};
use grana_ui::Rect;

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

    pub fn screen(&self, scale: f32, size: [u32; 2]) -> Regions {
        self.regions(scale, [0.0, 0.0], size)
    }

    pub fn export(&self, width: u32, even: bool) -> Regions {
        assert!(width > 0, "an export names a positive image width");
        let stage = self.stage();
        assert!(
            stage.width() > 0.0 && stage.height() > 0.0,
            "an export covers at least one pane"
        );
        let width = if even { even_size(width) } else { width };
        let scale = width as f32 / self.stage().width().max(1.0);
        let height = (self.stage().height() * scale).round().max(1.0) as u32;
        let height = if even { even_size(height) } else { height };
        let stage = self.stage();
        self.regions(scale, [stage.min.x, stage.min.y], [width, height])
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

fn even_size(value: u32) -> u32 {
    (value.max(2) / 2) * 2
}
