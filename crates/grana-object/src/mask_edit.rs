use crate::ObjectAnchor;

const POINT_LIMIT: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaskTool {
    Brush,
    Wand,
}

impl MaskTool {
    pub fn index(self) -> usize {
        match self {
            Self::Brush => 0,
            Self::Wand => 1,
        }
    }

    pub fn parse(index: usize) -> Option<Self> {
        match index {
            0 => Some(Self::Brush),
            1 => Some(Self::Wand),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaskMode {
    Add,
    Remove,
}

impl MaskMode {
    pub fn index(self) -> usize {
        match self {
            Self::Add => 0,
            Self::Remove => 1,
        }
    }

    pub fn parse(index: usize) -> Option<Self> {
        match index {
            0 => Some(Self::Add),
            1 => Some(Self::Remove),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MaskEdit {
    frame: u32,
    anchor: ObjectAnchor,
    tool: MaskTool,
    mode: MaskMode,
    radius: f32,
    tolerance: f32,
    points: Vec<[f32; 3]>,
}

impl MaskEdit {
    pub fn brush(
        frame: u32,
        anchor: ObjectAnchor,
        mode: MaskMode,
        radius: f32,
        points: Vec<[f32; 3]>,
    ) -> Result<Self, String> {
        if !(radius.is_finite() && radius > 0.0) {
            return Err("a brush stroke carries a positive radius".to_string());
        }
        if points.is_empty() {
            return Err("a brush stroke paints at least one point".to_string());
        }
        Self::new(frame, anchor, MaskTool::Brush, mode, radius, 0.0, points)
    }

    pub fn wand(
        frame: u32,
        anchor: ObjectAnchor,
        mode: MaskMode,
        tolerance: f32,
        at: [f32; 3],
    ) -> Result<Self, String> {
        if !(tolerance.is_finite() && (0.0..=1.0).contains(&tolerance)) {
            return Err("a wand stroke carries a unit tolerance".to_string());
        }
        Self::new(
            frame,
            anchor,
            MaskTool::Wand,
            mode,
            0.0,
            tolerance,
            vec![at],
        )
    }

    fn new(
        frame: u32,
        anchor: ObjectAnchor,
        tool: MaskTool,
        mode: MaskMode,
        radius: f32,
        tolerance: f32,
        points: Vec<[f32; 3]>,
    ) -> Result<Self, String> {
        if points.len() > POINT_LIMIT {
            return Err(format!(
                "a mask stroke reaches {POINT_LIMIT} points and names {}",
                points.len()
            ));
        }
        if points.iter().flatten().any(|value| !value.is_finite()) {
            return Err("a mask stroke paints finite points".to_string());
        }
        Ok(Self {
            frame,
            anchor,
            tool,
            mode,
            radius,
            tolerance,
            points,
        })
    }

    pub fn frame(&self) -> u32 {
        self.frame
    }

    pub fn anchor(&self) -> ObjectAnchor {
        self.anchor
    }

    pub fn tool(&self) -> MaskTool {
        self.tool
    }

    pub fn mode(&self) -> MaskMode {
        self.mode
    }

    pub fn radius(&self) -> f32 {
        self.radius
    }

    pub fn tolerance(&self) -> f32 {
        self.tolerance
    }

    pub fn points(&self) -> &[[f32; 3]] {
        &self.points
    }
}
