use grana_ui::Rect;

#[derive(Clone, Copy, Debug)]
pub struct Panels {
    volume: [f32; 4],
    section: [f32; 4],
    stage: [f32; 4],
}

#[derive(Clone, Copy, Debug)]
pub struct Panes {
    pub size: [u32; 2],
    pub volume: [u32; 4],
    pub section: [u32; 4],
    pub pixels_per_point: f32,
    pub volume_points: [f32; 4],
    pub section_points: [f32; 4],
}

impl Panels {
    pub fn new(volume: Rect, section: Rect) -> Self {
        let stage = Rect::from_min_max(volume.min.min(section.min), volume.max.max(section.max));
        Self {
            volume: array(volume),
            section: array(section),
            stage: array(stage),
        }
    }

    pub fn native_width(&self, scale: f32) -> u32 {
        (self.stage[2] * scale).round().max(1.0) as u32
    }

    pub fn screen(&self, scale: f32, size: [u32; 2]) -> Panes {
        self.panes(scale, [0.0, 0.0], size)
    }

    pub fn export(&self, width: u32, even: bool) -> Panes {
        assert!(width > 0, "an export names a positive image width");
        let width = if even { even_size(width) } else { width };
        let scale = width as f32 / self.stage[2].max(1.0);
        let height = (self.stage[3] * scale).round().max(1.0) as u32;
        let height = if even { even_size(height) } else { height };
        self.panes(scale, [self.stage[0], self.stage[1]], [width, height])
    }

    fn panes(&self, scale: f32, origin: [f32; 2], size: [u32; 2]) -> Panes {
        Panes {
            size,
            volume: region(self.volume, scale, origin, size),
            section: region(self.section, scale, origin, size),
            pixels_per_point: scale,
            volume_points: points(self.volume, origin),
            section_points: points(self.section, origin),
        }
    }
}

fn array(rect: Rect) -> [f32; 4] {
    [rect.min.x, rect.min.y, rect.width(), rect.height()]
}

fn points(rect: [f32; 4], origin: [f32; 2]) -> [f32; 4] {
    [rect[0] - origin[0], rect[1] - origin[1], rect[2], rect[3]]
}

fn region(rect: [f32; 4], scale: f32, origin: [f32; 2], size: [u32; 2]) -> [u32; 4] {
    let x = (((rect[0] - origin[0]) * scale).round() as i64).clamp(0, size[0] as i64 - 1) as u32;
    let y = (((rect[1] - origin[1]) * scale).round() as i64).clamp(0, size[1] as i64 - 1) as u32;
    let width = ((rect[2] * scale).round() as u32).min(size[0] - x).max(1);
    let height = ((rect[3] * scale).round() as u32).min(size[1] - y).max(1);
    [x, y, width, height]
}

fn even_size(value: u32) -> u32 {
    (value.max(2) / 2) * 2
}
