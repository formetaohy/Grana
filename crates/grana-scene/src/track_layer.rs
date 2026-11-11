use std::sync::Arc;

use grana_geometry::PackedMesh;
use grana_object::TrackSet;

use crate::revision::Revision;

pub struct TrackLayer {
    set: Arc<TrackSet>,
    mesh: Arc<PackedMesh>,
    revision: Revision,
}

impl TrackLayer {
    pub fn new(set: TrackSet, thickness: f32, revision: Revision) -> Self {
        Self::shared(Arc::new(set), thickness, revision)
    }

    pub fn shared(set: Arc<TrackSet>, thickness: f32, revision: Revision) -> Self {
        let mesh = PackedMesh::new(set.mesh(thickness));
        Self {
            set,
            mesh: Arc::new(mesh),
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

    pub fn revision(&self) -> Revision {
        self.revision
    }

    pub fn set_thickness(&mut self, thickness: f32, revision: Revision) {
        self.mesh = Arc::new(PackedMesh::new(self.set.mesh(thickness)));
        self.revision = revision;
    }

    pub fn cut(&mut self, time: u32, object: usize, thickness: f32, revision: Revision) -> bool {
        let Some(set) = self.set.cut(time, object) else {
            return false;
        };
        self.install(set, thickness, revision);
        true
    }

    pub fn join(
        &mut self,
        source: (u32, usize),
        target: (u32, usize),
        thickness: f32,
        revision: Revision,
    ) -> bool {
        let Some(set) = self.set.join(source, target) else {
            return false;
        };
        self.install(set, thickness, revision);
        true
    }

    pub fn divide(
        &mut self,
        parent: (u32, usize),
        first: (u32, usize),
        second: (u32, usize),
        thickness: f32,
        revision: Revision,
    ) -> bool {
        let Some(set) = self.set.divide(parent, first, second) else {
            return false;
        };
        self.install(set, thickness, revision);
        true
    }

    pub fn merge(
        &mut self,
        first: (u32, usize),
        second: (u32, usize),
        child: (u32, usize),
        thickness: f32,
        revision: Revision,
    ) -> bool {
        let Some(set) = self.set.merge(first, second, child) else {
            return false;
        };
        self.install(set, thickness, revision);
        true
    }

    pub fn reparent(
        &mut self,
        child: (u32, usize),
        parent: (u32, usize),
        thickness: f32,
        revision: Revision,
    ) -> bool {
        let Some(set) = self.set.reparent(child, parent) else {
            return false;
        };
        self.install(set, thickness, revision);
        true
    }

    pub fn detach(&mut self, child: (u32, usize), thickness: f32, revision: Revision) -> bool {
        let Some(set) = self.set.detach(child) else {
            return false;
        };
        self.install(set, thickness, revision);
        true
    }

    fn install(&mut self, set: TrackSet, thickness: f32, revision: Revision) {
        self.set = Arc::new(set);
        self.mesh = Arc::new(PackedMesh::new(self.set.mesh(thickness)));
        self.revision = revision;
    }
}
