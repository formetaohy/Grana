use super::super::*;
use super::Features;
use crate::table::SceneTable;
use crate::volume::SliceFrame;

pub(super) struct Stage<'a> {
    pub(super) device: &'a Device,
    pub(super) queue: &'a Queue,
    pub(super) encoder: &'a mut CommandEncoder,
    pub(super) target: &'a FrameTarget,
    pub(super) scene: &'a SceneFrame<'a>,
    pub(super) table: &'a SceneTable,
    pub(super) level: &'a BindGroup,
    pub(super) atlas: &'a BindGroup,
    pub(super) oit_resolve: &'a OitResolve,
    pub(super) splats: Option<&'a SplatPasses>,
    pub(super) sections: &'a Sections,
    pub(super) overlay: &'a mut Overlays,
    pub(super) volume_pass: &'a VolumePass,
    pub(super) data: &'a FrameData,
    pub(super) features: Features,
}

pub(super) fn encode(stage: Stage<'_>) {
    let Stage {
        device,
        queue,
        encoder,
        target,
        scene,
        table,
        level,
        atlas,
        oit_resolve,
        splats,
        sections,
        overlay,
        volume_pass,
        data,
        features,
    } = stage;
    let region = features.region;
    if features.slice {
        let targets = SliceTargets {
            color: target.scene.color(),
            depth: target.scene.depth(),
        };
        if features.volume {
            volume_pass.draw_slice(
                encoder,
                SliceFrame {
                    target: targets.color,
                    region,
                    scene: table.group(),
                    level,
                    atlas,
                },
            );
        }
        {
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("grana.section.pass"),
                color_attachments: &[Some(load(targets.color))],
                depth_stencil_attachment: None,
                ..Default::default()
            });
            crate::target::viewport(&mut pass, region);
            sections.draw(&mut pass, table.group());
        }
        if let Some(splats) = splats {
            splats.draw_slice(encoder, targets, region, table.group());
        }
    }
    if features.translucent
        && !features.volume
        && let Some(oit) = target.oit.as_ref()
    {
        oit_resolve.draw(
            encoder,
            target.scene.color(),
            region,
            table.group(),
            oit.read_group(),
        );
    }
    if !scene.overlays.is_empty() {
        overlay.prepare(
            device,
            queue,
            scene.overlays,
            glam::Mat4::from_cols_array_2d(&data.view_projection),
        );
        overlay.draw(encoder, target.scene.color(), region);
    }
}

fn load(view: &TextureView) -> RenderPassColorAttachment<'_> {
    RenderPassColorAttachment {
        view,
        depth_slice: None,
        resolve_target: None,
        ops: Operations {
            load: LoadOp::Load,
            store: StoreOp::Store,
        },
    }
}
