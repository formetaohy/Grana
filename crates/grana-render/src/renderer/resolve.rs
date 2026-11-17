use super::*;
use crate::temporal::{History, Inputs, Reuse, TemporalPass};

pub(super) struct Capture<'a> {
    pub(super) device: &'a Device,
    pub(super) history: &'a mut History,
    pub(super) previous: FrameView,
    pub(super) scene: &'a BindGroup,
    pub(super) geometry: &'a BindGroup,
    pub(super) accumulated: &'a TextureView,
    pub(super) counts: &'a TextureView,
    pub(super) fresh: &'a TextureView,
}

pub(super) fn capture(pass: &TemporalPass, encoder: &mut CommandEncoder, capture: Capture<'_>) {
    let Capture {
        device,
        history,
        previous,
        scene,
        geometry,
        accumulated,
        counts,
        fresh,
    } = capture;
    TemporalPass::capture(
        pass,
        device,
        encoder,
        history,
        &previous,
        scene,
        &Inputs {
            accumulated,
            counts,
            fresh,
            geometry,
        },
    );
}

pub(super) struct Seed<'a> {
    pub(super) device: &'a Device,
    pub(super) queue: &'a Queue,
    pub(super) history: &'a History,
    pub(super) view: &'a FrameView,
    pub(super) length: f32,
    pub(super) tolerance: f32,
    pub(super) scene: &'a BindGroup,
    pub(super) geometry: &'a BindGroup,
    pub(super) accumulated: &'a TextureView,
    pub(super) counts: &'a TextureView,
    pub(super) fresh: &'a TextureView,
}

pub(super) fn seed(pass: &TemporalPass, encoder: &mut CommandEncoder, seed: Seed<'_>) {
    let Seed {
        device,
        queue,
        history,
        view,
        length,
        tolerance,
        scene,
        geometry,
        accumulated,
        counts,
        fresh,
    } = seed;
    TemporalPass::seed(
        pass,
        device,
        queue,
        encoder,
        scene,
        Reuse {
            history,
            view,
            length,
            tolerance,
            inputs: &Inputs {
                accumulated,
                counts,
                fresh,
                geometry,
            },
        },
    );
}

pub(super) struct Measure<'a> {
    pub(super) device: &'a Device,
    pub(super) queue: &'a Queue,
    pub(super) pipelines: &'a Pipelines,
    pub(super) convergence: &'a mut Option<Convergence>,
    pub(super) sampled: crate::converge::Sampled<'a>,
    pub(super) samples: u32,
}

pub(super) fn measure(encoder: &mut CommandEncoder, measure: Measure<'_>) {
    let Measure {
        device,
        queue,
        pipelines,
        convergence,
        sampled,
        samples,
    } = measure;
    let convergence = convergence.get_or_insert_with(|| Convergence::new(device, pipelines));
    Convergence::encode(convergence, device, queue, encoder, sampled, samples);
}

pub(super) struct Accumulate<'a> {
    pub(super) clear: &'a crate::clear::Clear,
    pub(super) present: &'a Present,
    pub(super) target: &'a mut FrameTarget,
    pub(super) region: [u32; 4],
    pub(super) reset: bool,
}

pub(super) fn accumulate(encoder: &mut CommandEncoder, accumulate: Accumulate<'_>) {
    let Accumulate {
        clear,
        present,
        target,
        region,
        reset,
    } = accumulate;
    if reset {
        crate::clear::Clear::accumulation(
            clear,
            encoder,
            &target.accumulation_view,
            &target.count_view,
            region,
        );
    }
    Present::accumulate(
        present,
        encoder,
        &target.accumulation_view,
        &target.count_view,
        &target.accumulate_group,
        region,
    );
}

pub(super) struct Presenting<'a> {
    pub(super) device: &'a Device,
    pub(super) queue: &'a Queue,
    pub(super) present: &'a Present,
    pub(super) target: &'a mut FrameTarget,
    pub(super) surface: &'a TextureView,
    pub(super) viewport: [u32; 4],
    pub(super) region: [u32; 4],
    pub(super) exposure: f32,
    pub(super) transparent: bool,
    pub(super) fade: &'a mut Option<Fade>,
}

pub(super) fn present(encoder: &mut CommandEncoder, presenting: Presenting<'_>) {
    let Presenting {
        device,
        queue,
        present,
        target,
        surface,
        viewport,
        region,
        exposure,
        transparent,
        fade,
    } = presenting;
    let blend = timing::fade(fade);
    Present::write(
        present,
        queue,
        exposure,
        transparent,
        blend,
        coverage(region, target.size),
    );
    if fade.is_some() {
        let group = target.fade_group(device, present);
        Present::present(present, encoder, surface, group, viewport);
        if fade.as_ref().is_some_and(|active| active.weight >= 1.0) {
            *fade = None;
        }
    } else {
        Present::present(present, encoder, surface, &target.present_group, viewport);
    }
}

fn coverage(region: [u32; 4], size: [u32; 2]) -> [f32; 4] {
    let width = size[0].max(1) as f32;
    let height = size[1].max(1) as f32;
    [
        region[0] as f32 / width,
        region[1] as f32 / height,
        region[2] as f32 / width,
        region[3] as f32 / height,
    ]
}
