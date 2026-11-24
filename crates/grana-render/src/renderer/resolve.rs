use super::*;
use crate::schema::Overlay;
use crate::temporal;
use glam::{Mat4, Vec3};

pub(super) struct Accumulate<'a> {
    pub(super) device: &'a Device,
    pub(super) queue: &'a Queue,
    pub(super) encoder: &'a mut CommandEncoder,
    pub(super) layouts: &'a Layouts,
    pub(super) temporal: &'a temporal::Resolve,
    pub(super) convergence: &'a mut temporal::Convergence,
    pub(super) scene: &'a BindGroup,
    pub(super) target: &'a mut FrameTarget,
    pub(super) data: &'a FrameData,
    pub(super) region: [u32; 4],
    pub(super) exposure: f32,
    pub(super) volume: bool,
    pub(super) reset: bool,
    pub(super) clamp: bool,
    pub(super) reach: f32,
    pub(super) dummy_normal: &'a TextureView,
    pub(super) dummy_volume: &'a crate::target::Unreached,
}

pub(super) struct Present<'a> {
    pub(super) device: &'a Device,
    pub(super) queue: &'a Queue,
    pub(super) encoder: &'a mut CommandEncoder,
    pub(super) pass: &'a crate::present::Present,
    pub(super) target: &'a mut FrameTarget,
    pub(super) overlay: &'a mut Overlays,
    pub(super) overlays: &'a [Overlay<'a>],
    pub(super) surface: &'a TextureView,
    pub(super) fade: Option<f32>,
    pub(super) region: [u32; 4],
    pub(super) exposure: f32,
    pub(super) transparent: bool,
}

pub(super) fn accumulate(stage: Accumulate<'_>) -> bool {
    let Accumulate {
        device,
        queue,
        encoder,
        layouts,
        temporal: temporal_pass,
        convergence,
        scene,
        target,
        data,
        region,
        exposure,
        volume,
        reset,
        clamp,
        reach,
        dummy_normal,
        dummy_volume,
    } = stage;
    let current = Mat4::from_cols_array_2d(&data.view_projection);
    let eye = Vec3::new(data.eye[0], data.eye[1], data.eye[2]);
    let intact = target.temporal.intact() && !reset;
    if !intact {
        convergence.invalidate();
    }
    temporal_pass.write(
        queue,
        temporal::State {
            previous: target.temporal.camera().unwrap_or(current),
            eye: target.temporal.eye().unwrap_or(eye),
            intact,
            clamp,
            ceiling: temporal::SAMPLE_BUDGET as f32,
            exposure,
            reach,
            history: target.temporal.reach(),
        },
    );
    let geometry = target.geometry(device, layouts, dummy_normal, dummy_volume, volume);
    let group = temporal_pass.bind(device, target.scene.color(), &target.temporal);
    temporal_pass.encode(
        encoder,
        [scene, &geometry, &group],
        &target.temporal,
        region,
    );
    target.temporal.advance(current, eye, reach);
    intact
}

pub(super) fn present(stage: Present<'_>) {
    let Present {
        device,
        queue,
        encoder,
        pass,
        target,
        overlay,
        overlays,
        surface,
        fade,
        region,
        exposure,
        transparent,
    } = stage;
    let current = target
        .temporal
        .camera()
        .expect("the accumulation names the presented camera");
    overlay.prepare(device, queue, overlays, current);
    overlay.draw(encoder, target.resolved(), region);
    pass.write(
        queue,
        exposure,
        transparent,
        fade,
        coverage(region, target.size),
    );
    if fade.is_some() {
        let group = target.fade_group(device, pass);
        pass.present(encoder, surface, group, region);
    } else {
        pass.present(encoder, surface, &target.present_group, region);
    }
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
