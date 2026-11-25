#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrackRow {
    label: u32,
    begin: u32,
    end: u32,
    parent: u32,
}

impl TrackRow {
    pub fn new(label: u32, begin: u32, end: u32, parent: u32) -> Self {
        assert!(label > 0, "a track row carries a positive label");
        assert!(begin <= end, "a track row spans its frames");
        assert!(parent != label, "a track row is not its own parent");
        Self {
            label,
            begin,
            end,
            parent,
        }
    }

    pub fn label(self) -> u32 {
        self.label
    }

    pub fn begin(self) -> u32 {
        self.begin
    }

    pub fn end(self) -> u32 {
        self.end
    }

    pub fn parent(self) -> u32 {
        self.parent
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Marker {
    label: u32,
    time: u32,
    position: [f32; 3],
    radii: [f32; 3],
}

impl Marker {
    pub fn new(label: u32, time: u32, position: [f32; 3], radii: [f32; 3]) -> Self {
        assert!(label > 0, "a marker belongs to a labelled track");
        assert!(
            position.iter().all(|value| value.is_finite()),
            "a marker lies at a finite position"
        );
        assert!(
            radii.iter().all(|value| value.is_finite() && *value > 0.0),
            "a marker spans a positive radius on every axis"
        );
        Self {
            label,
            time,
            position,
            radii,
        }
    }

    pub fn label(self) -> u32 {
        self.label
    }

    pub fn time(self) -> u32 {
        self.time
    }

    pub fn position(self) -> [f32; 3] {
        self.position
    }

    pub fn radii(self) -> [f32; 3] {
        self.radii
    }
}
