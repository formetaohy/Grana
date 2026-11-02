use grana_geometry::PackedMesh;
use grana_object::TrackSet;

pub struct TrackLayer {
    set: TrackSet,
    mesh: PackedMesh,
    visible: bool,
    thickness: f32,
    revision: u64,
}

impl TrackLayer {
    pub fn new(set: TrackSet, thickness: f32) -> Self {
        let mesh = PackedMesh::new(set.mesh(thickness));
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

    pub fn mesh(&self) -> &PackedMesh {
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
        self.mesh = PackedMesh::new(self.set.mesh(thickness));
        self.revision += 1;
    }

    pub fn cut(&mut self, time: u32, object: usize) -> bool {
        let Some(set) = self.set.cut(time, object) else {
            return false;
        };
        self.install(set);
        true
    }

    pub fn join(&mut self, source: (u32, usize), target: (u32, usize)) -> bool {
        let Some(set) = self.set.join(source, target) else {
            return false;
        };
        self.install(set);
        true
    }

    fn install(&mut self, set: TrackSet) {
        self.set = set;
        self.mesh = PackedMesh::new(self.set.mesh(self.thickness));
        self.revision += 1;
    }
}
