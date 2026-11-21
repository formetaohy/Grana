use super::*;
use crate::jitter;
use crate::schema::Overlay;
use crate::temporal;
use glam::Mat4;

pub(super) struct Stage<'a> {
    pub(super) device: &'a Device,
    pub(super) queue: &'a Queue,
    pub(super) encoder: &'a mut CommandEncoder,
    pub(super) layouts: &'a Layouts,
    pub(super) present: &'a Present,
    pub(super) temporal: &'a temporal::Resolve,
    pub(super) convergence: &'a mut temporal::Convergence,
    pub(super) scene: &'a BindGroup,
    pub(super) target: &'a mut FrameTarget,
    pub(super) overlay: &'a mut Overlays,
    pub(super) overlays: &'a [Overlay<'a>],
    pub(super) data: &'a FrameData,
    pub(super) surface: &'a TextureView,
    pub(super) fade: &'a mut Option<Fade>,
    pub(super) region: [u32; 4],
    pub(super) exposure: f32,
    pub(super) transparent: bool,
    pub(super) volume: bool,
    pub(super) phase: u32,
    pub(super) reset: bool,
    pub(super) clamp: bool,
    pub(super) previous: Option<Mat4>,
    pub(super) dummy_normal: &'a TextureView,
    pub(super) dummy_volume: &'a crate::target::Unreached,
}

pub(super) fn encode(stage: Stage<'_>) -> bool {
    let Stage {
        device,
        queue,
        encoder,
        layouts,
        present,
        temporal: temporal_pass,
        convergence,
        scene,
        target,
        overlay,
        overlays,
        data,
        surface,
        fade,
        region,
        exposure,
        transparent,
        volume,
        phase,
        reset,
        clamp,
        previous,
        dummy_normal,
        dummy_volume,
    } = stage;
    let current = Mat4::from_cols_array_2d(&data.view_projection);
    let offset = jitter::offset(phase, [region[2], region[3]]);
    let intact = target.temporal.intact() && !reset;
    if !intact {
        convergence.invalidate();
    }
    temporal_pass.write(
        queue,
        previous.unwrap_or(current),
        [offset, target.jitter],
        intact,
        clamp,
    );
    let geometry = target.geometry(device, layouts, dummy_normal, dummy_volume, volume);
    let group = temporal_pass.bind(device, target.scene.color(), &target.temporal);
    temporal_pass.encode(
        encoder,
        [scene, &geometry, &group],
        &target.temporal,
        region,
    );
    convergence.evaluate(device, queue, encoder, &target.temporal, region, exposure);
    target.temporal.advance();
    target.jitter = offset;
    overlay.prepare(device, queue, overlays, current);
    overlay.draw(encoder, target.resolved(), region);
    let blend = timing::fade(fade);
    present.write(
        queue,
        exposure,
        transparent,
        blend,
        coverage(region, target.size),
    );
    if blend.is_some() {
        let group = target.fade_group(device, present);
        present.present(encoder, surface, group, region);
        if fade.as_ref().is_some_and(|active| active.weight >= 1.0) {
            *fade = None;
        }
    } else {
        present.present(encoder, surface, &target.present_group, region);
    }
    intact
}

pub(super) fn coverage(region: [u32; 4], size: [u32; 2]) -> [f32; 4] {
    let width = size[0].max(1) as f32;
    let height = size[1].max(1) as f32;
    [
        region[0] as f32 / width,
        region[1] as f32 / height,
        region[2] as f32 / width,
        region[3] as f32 / height,
    ]
}
