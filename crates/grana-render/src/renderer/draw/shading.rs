use super::super::*;
use super::Features;
use crate::table::SceneTable;
use crate::volume::CompositeFrame;

pub(super) struct Stage<'a> {
    pub(super) encoder: &'a mut CommandEncoder,
    pub(super) target: &'a mut FrameTarget,
    pub(super) table: &'a SceneTable,
    pub(super) shadow: Option<&'a BindGroup>,
    pub(super) volume_pass: &'a VolumePass,
    pub(super) meshes: Option<&'a MeshPasses>,
    pub(super) splats: Option<&'a SplatPasses>,
    pub(super) data: &'a FrameData,
    pub(super) features: Features,
    pub(super) dummy_ao: &'a BindGroup,
    pub(super) dummy_oit: &'a Oit,
}

pub(super) fn encode(stage: Stage<'_>) {
    let Stage {
        encoder,
        target,
        table,
        shadow,
        volume_pass,
        meshes,
        splats,
        data,
        features,
        dummy_ao,
        dummy_oit,
    } = stage;
    let region = features.region;
    if features.translucent {
        let epoch = target
            .oit
            .as_mut()
            .map(|oit| oit.begin(encoder))
            .expect("the fragment store is ready");
        assert_eq!(
            epoch, data.oit[2],
            "the fragment store opens the epoch the frame names"
        );
    }
    let ao = target.ao.as_ref().map_or(dummy_ao, crate::ao::Ao::group);
    let oit = target.oit.as_ref().filter(|_| features.translucent);
    let oit_group = oit.map_or(dummy_oit.read_group(), Oit::read_group);
    if features.surfaces
        && !features.slice
        && let Some(meshes) = meshes
    {
        meshes.draw_opaque(
            encoder,
            region,
            target.scene.color(),
            target.scene.depth(),
            shadow.expect("the shadow is ready"),
            ao,
        );
    }
    if let Some(oit) = oit {
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.oit.pass"),
            color_attachments: &[
                Some(cleared(oit.residual_color())),
                Some(cleared(oit.residual_moment())),
            ],
            depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                view: target.scene.depth(),
                depth_ops: Some(Operations {
                    load: LoadOp::Load,
                    store: StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        crate::target::viewport(&mut pass, region);
        if let Some(meshes) = meshes {
            meshes.draw_oit(
                &mut pass,
                shadow.expect("the shadow is ready"),
                oit.write_group(),
                ao,
            );
        }
        if let Some(splats) = splats {
            splats.draw_oit(&mut pass, table.group(), oit.write_group());
        }
    }
    if features.volume
        && !features.slice
        && let Some(staged) = target.volume.as_ref()
    {
        volume_pass.composite(
            encoder,
            CompositeFrame {
                target: target.scene.color(),
                region,
                scene: table.group(),
                volume: staged.group(),
                oit: oit_group,
            },
        );
    }
}

fn cleared(view: &TextureView) -> RenderPassColorAttachment<'_> {
    RenderPassColorAttachment {
        view,
        depth_slice: None,
        resolve_target: None,
        ops: Operations {
            load: LoadOp::Clear(wgpu::Color::TRANSPARENT),
            store: StoreOp::Store,
        },
    }
}
