use std::sync::Arc;

use grana_geometry::PackedMesh;

pub struct Surface {
    channel: usize,
    time: u32,
    mesh: Arc<PackedMesh>,
    colors: Arc<Vec<[f32; 3]>>,
    revision: u64,
}

impl Surface {
    pub fn new(
        channel: usize,
        time: u32,
        mesh: Arc<PackedMesh>,
        colors: Arc<Vec<[f32; 3]>>,
        revision: u64,
    ) -> Self {
        assert_eq!(
            colors.len(),
            mesh.object_count(),
            "a traced surface names one colour per label"
        );
        Self {
            channel,
            time,
            mesh,
            colors,
            revision,
        }
    }

    pub fn channel(&self) -> usize {
        self.channel
    }

    pub fn time(&self) -> u32 {
        self.time
    }

    pub fn mesh(&self) -> &Arc<PackedMesh> {
        &self.mesh
    }

    pub fn colors(&self) -> &[[f32; 3]] {
        &self.colors
    }

    pub fn shared_colors(&self) -> &Arc<Vec<[f32; 3]>> {
        &self.colors
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }
}

pub fn trace_channels(
    channels: &crate::channels::Channels,
    scene: &grana_scene::Scene,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<Vec<Surface>, String> {
    let mut surfaces = Vec::new();
    for (channel, held) in scene.channels.iter().enumerate() {
        if !held.enabled {
            continue;
        }
        if scene
            .dataset
            .channels()
            .get(channel)
            .is_none_or(|info| info.color_table().is_none())
        {
            continue;
        }
        let time = scene.time();
        let level = scene.dataset.pyramid().level(0);
        let request = crate::run::TraceRequest::new(channel, time, level, scene.drifts().clone());
        let mut traced = None;
        crate::run::trace(channels, request, cancel, &mut |event| {
            if let crate::run::SurfaceEvent::Surface { mesh, colors } = event {
                traced = Some((mesh, colors));
            }
        })?;
        let Some((mesh, colors)) = traced else {
            continue;
        };
        let revision = (mesh.vertex_count() as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
            ^ ((mesh.triangle_count() as u64) << 1)
            ^ (u64::from(time) << 48)
            ^ ((channel as u64) << 56);
        surfaces.push(Surface::new(channel, time, mesh, colors, revision));
    }
    Ok(surfaces)
}
