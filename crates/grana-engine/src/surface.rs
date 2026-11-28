use std::sync::Arc;

use grana_geometry::PackedMesh;

#[derive(Clone)]
pub struct Surface {
    channel: usize,
    mesh: Arc<PackedMesh>,
    colors: Arc<Vec<[f32; 3]>>,
    revision: u64,
}

impl Surface {
    pub fn new(
        channel: usize,
        mesh: Arc<PackedMesh>,
        colors: Arc<Vec<[f32; 3]>>,
        revision: u64,
    ) -> Self {
        assert_eq!(
            colors.len(),
            mesh.object_count(),
            "a channel surface names one colour per traced label"
        );
        Self {
            channel,
            mesh,
            colors,
            revision,
        }
    }

    pub fn view(&self) -> ChannelSurface<'_> {
        ChannelSurface::new(self.channel, &self.mesh, &self.colors, self.revision)
    }

    pub fn channel(&self) -> usize {
        self.channel
    }

    pub fn mesh(&self) -> &Arc<PackedMesh> {
        &self.mesh
    }

    pub fn colors(&self) -> &[[f32; 3]] {
        &self.colors
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }
}

pub struct ChannelSurface<'a> {
    channel: usize,
    mesh: &'a Arc<PackedMesh>,
    colors: &'a [[f32; 3]],
    revision: u64,
}

impl<'a> ChannelSurface<'a> {
    pub fn new(
        channel: usize,
        mesh: &'a Arc<PackedMesh>,
        colors: &'a [[f32; 3]],
        revision: u64,
    ) -> Self {
        assert_eq!(
            colors.len(),
            mesh.object_count(),
            "a channel surface names one colour per traced label"
        );
        Self {
            channel,
            mesh,
            colors,
            revision,
        }
    }

    pub fn channel(&self) -> usize {
        self.channel
    }

    pub fn mesh(&self) -> &Arc<PackedMesh> {
        self.mesh
    }

    pub fn colors(&self) -> &[[f32; 3]] {
        self.colors
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }
}
