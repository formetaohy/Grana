use crate::ObjectAnchor;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceEditKind {
    Merge,
    Split,
}

impl SurfaceEditKind {
    pub fn index(self) -> usize {
        match self {
            Self::Merge => 0,
            Self::Split => 1,
        }
    }

    pub fn verb(self) -> &'static str {
        match self {
            Self::Merge => "merge",
            Self::Split => "split",
        }
    }

    pub fn parse(index: usize) -> Option<Self> {
        match index {
            0 => Some(Self::Merge),
            1 => Some(Self::Split),
            _ => None,
        }
    }

    fn admits(self, anchors: usize) -> bool {
        match self {
            Self::Merge => anchors >= 2,
            Self::Split => anchors == 1,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SurfaceEdit {
    frame: u32,
    kind: SurfaceEditKind,
    anchors: Vec<ObjectAnchor>,
}

impl SurfaceEdit {
    pub fn merge(frame: u32, anchors: Vec<ObjectAnchor>) -> Self {
        Self::new(frame, SurfaceEditKind::Merge, anchors).expect("a merge joins two objects")
    }

    pub fn split(frame: u32, anchor: ObjectAnchor) -> Self {
        Self::new(frame, SurfaceEditKind::Split, vec![anchor]).expect("a split divides one object")
    }

    pub fn restored(
        frame: u32,
        kind: SurfaceEditKind,
        anchors: Vec<ObjectAnchor>,
    ) -> Result<Self, String> {
        Self::new(frame, kind, anchors)
    }

    fn new(frame: u32, kind: SurfaceEditKind, anchors: Vec<ObjectAnchor>) -> Result<Self, String> {
        if !kind.admits(anchors.len()) {
            return Err(match kind {
                SurfaceEditKind::Merge => format!(
                    "a merge of frame {frame} joins at least two objects and names {}",
                    anchors.len()
                ),
                SurfaceEditKind::Split => format!(
                    "a split of frame {frame} divides one object and names {}",
                    anchors.len()
                ),
            });
        }
        Ok(Self {
            frame,
            kind,
            anchors,
        })
    }

    pub fn frame(&self) -> u32 {
        self.frame
    }

    pub fn kind(&self) -> SurfaceEditKind {
        self.kind
    }

    pub fn anchors(&self) -> &[ObjectAnchor] {
        &self.anchors
    }
}
