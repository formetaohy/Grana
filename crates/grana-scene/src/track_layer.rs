use std::sync::Arc;

use grana_geometry::PackedMesh;
use grana_object::TrackSet;

pub struct TrackLayer {
    set: Arc<TrackSet>,
    mesh: Arc<PackedMesh>,
    visible: bool,
    thickness: f32,
    revision: u64,
}

impl TrackLayer {
    pub fn new(set: TrackSet, thickness: f32, revision: u64) -> Self {
        Self::shared(Arc::new(set), thickness, revision)
    }

    pub fn shared(set: Arc<TrackSet>, thickness: f32, revision: u64) -> Self {
        let mesh = PackedMesh::new(set.mesh(thickness));
        Self {
            set,
            mesh: Arc::new(mesh),
            visible: true,
            thickness,
            revision,
        }
    }

    pub fn set(&self) -> &TrackSet {
        &self.set
    }

    pub fn shared_set(&self) -> Arc<TrackSet> {
        Arc::clone(&self.set)
    }

    pub fn mesh(&self) -> Arc<PackedMesh> {
        Arc::clone(&self.mesh)
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

    pub fn set_thickness(&mut self, thickness: f32, revision: u64) {
        if (self.thickness - thickness).abs() <= f32::EPSILON {
            return;
        }
        self.thickness = thickness;
        self.mesh = Arc::new(PackedMesh::new(self.set.mesh(thickness)));
        self.revision = revision;
    }

    pub fn cut(&mut self, time: u32, object: usize, revision: u64) -> bool {
        let Some(set) = self.set.cut(time, object) else {
            return false;
        };
        self.install(set, revision);
        true
    }

    pub fn join(&mut self, source: (u32, usize), target: (u32, usize), revision: u64) -> bool {
        let Some(set) = self.set.join(source, target) else {
            return false;
        };
        self.install(set, revision);
        true
    }

    fn install(&mut self, set: TrackSet, revision: u64) {
        self.set = Arc::new(set);
        self.mesh = Arc::new(PackedMesh::new(self.set.mesh(self.thickness)));
        self.revision = revision;
    }
}
