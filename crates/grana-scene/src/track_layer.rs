use grana_object::{Mesh, TrackSet};

pub struct TrackLayer {
    set: TrackSet,
    mesh: Mesh,
    visible: bool,
    thickness: f32,
    revision: u64,
}

impl TrackLayer {
    pub fn new(set: TrackSet, thickness: f32) -> Self {
        let mesh = set.mesh(thickness);
        Self {
            set,
            mesh,
            visible: true,
            thickness,
            revision: 1,
        }
    }

    pub fn set(&self) -> &TrackSet {
        &self.set
    }

    pub fn mesh(&self) -> &Mesh {
        &self.mesh
    }

    pub fn visible(&self) -> bool {
        self.visible
    }

    pub fn thickness(&self) -> f32 {
        self.thickness
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    pub fn set_thickness(&mut self, thickness: f32) {
        if (self.thickness - thickness).abs() <= f32::EPSILON {
            return;
        }
        self.thickness = thickness;
        self.mesh = self.set.mesh(thickness);
        self.revision += 1;
    }
}
