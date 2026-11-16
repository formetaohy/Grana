use super::signature::ShadowKeys;
use super::view::View;
use super::*;
use crate::volume::{CompositeFrame, MarchFrame, SliceFrame};

const AMBIENT_REACH: f32 = 40.0;
const AMBIENT_STRENGTH: f32 = 0.9;

pub(super) struct Draw<'a> {
    pub(super) encoder: &'a mut CommandEncoder,
    pub(super) device: &'a Device,
    pub(super) queue: &'a Queue,
    pub(super) layouts: &'a Layouts,
    pub(super) pipelines: &'a Pipelines,
    pub(super) target: &'a mut FrameTarget,
    pub(super) scene: &'a SceneFrame<'a>,
    pub(super) table: &'a crate::table::SceneTable,
    pub(super) shadow_scene: &'a BindGroup,
    pub(super) level_group: &'a BindGroup,
    pub(super) atlas_group: &'a BindGroup,
    pub(super) occupancy: &'a BindGroup,
    pub(super) shadow: Option<&'a mut Shadow>,
    pub(super) shadow_keys: ShadowKeys,
    pub(super) volume: &'a VolumePass,
    pub(super) clear: &'a crate::clear::Clear,
    pub(super) surface: &'a crate::surface::Pipeline,
    pub(super) oit_resolve: &'a mut Option<OitResolve>,
    pub(super) meshes: Option<&'a mut MeshPasses>,
    pub(super) splats: Option<&'a mut SplatPasses>,
    pub(super) sections: &'a Sections,
    pub(super) overlay: &'a mut Overlays,
    pub(super) data: &'a FrameData,
    pub(super) view: View,
    pub(super) slice: bool,
    pub(super) translucent: bool,
    pub(super) volume_visible: bool,
    pub(super) surfaces: bool,
    pub(super) sample: u32,
    pub(super) dummy_ao: &'a BindGroup,
    pub(super) dummy_oit: &'a Oit,
    pub(super) dummy_normal: &'a TextureView,
    pub(super) dummy_volume: &'a crate::target::Unreached,
    pub(super) object_key: &'a mut Option<u64>,
}

pub(super) fn encode(draw: Draw<'_>) {
    let Draw {
        encoder,
        device,
        queue,
        layouts,
        pipelines,
        target,
        scene,
        table,
        shadow_scene,
        level_group,
        atlas_group,
        occupancy,
        shadow,
        shadow_keys,
        volume,
        clear,
        surface,
        oit_resolve,
        meshes,
        splats,
        sections,
        overlay,
        data,
        view,
        slice,
        translucent,
        volume_visible,
        surfaces,
        sample,
        dummy_ao,
        dummy_oit,
        dummy_normal,
        dummy_volume,
        object_key,
    } = draw;
    let region = view.region;
    let deferred = surfaces && !slice && target.gbuffer.is_some();
    let mut shadow = shadow;
    if let Some(shadow) = shadow.as_deref_mut()
        && !slice
        && scene.shading.shadows > 0.0
        && volume_visible
    {
        let groups = ShadowGroups {
            scene: shadow_scene,
            level: level_group,
            atlas: atlas_group,
            occupancy,
        };
        let object = shadow_keys.objects();
        if *object_key != Some(object) {
            *object_key = Some(object);
            shadow.clear_objects(encoder);
            if let Some(meshes) = meshes.as_deref() {
                meshes.draw_shadow(encoder, shadow.objects());
            }
        }
        let key = shadow_keys.volume();
        if shadow.stale(key) {
            table.write_shadow_frame(queue, *data);
        }
        shadow.encode_tau(encoder, groups, key);
    }
    if deferred {
        clear.depth(encoder, target.scene.depth(), region);
        let gbuffer = target
            .gbuffer
            .as_ref()
            .expect("the geometry buffer is ready");
        if let Some(meshes) = meshes.as_deref() {
            meshes.draw_gbuffer(
                encoder,
                region,
                gbuffer,
                target.scene.depth(),
                table.group(),
            );
        }
    } else {
        clear.scene(encoder, target.scene.color(), target.scene.depth(), region);
    }
    if volume_visible
        && !slice
        && let Some(staged) = target.volume.as_ref()
    {
        volume.march(
            encoder,
            region,
            MarchFrame {
                radiance: staged.radiance(),
                surface: staged.surface(),
                normal: staged.normal(),
                scene: table.group(),
                level: level_group,
                atlas: atlas_group,
                object: target.scene.depth_group(),
                occupancy,
                shadow: shadow
                    .as_deref()
                    .map(Shadow::group)
                    .expect("the shadow is ready"),
            },
        );
    }
    if deferred {
        let resolution = target.size;
        let merged = if volume_visible {
            Some(target.geometry(device, layouts, dummy_normal, dummy_volume, true))
        } else {
            None
        };
        let objects = if volume_visible {
            None
        } else {
            Some(target.objects(device, layouts))
        };
        if let (Some(ao), Some(resolved)) = (target.ao.as_ref(), target.surface.as_ref()) {
            let source = match (&merged, &objects) {
                (Some(group), _) => crate::surface::Source::Merged(group),
                (None, Some(group)) => crate::surface::Source::Objects(group),
                (None, None) => unreachable!("a resolved surface names its source"),
            };
            surface.resolve(encoder, resolved, region, source);
            ao.evaluate(
                queue,
                encoder,
                data,
                resolution,
                crate::ao::Ambient::new(AMBIENT_REACH, AMBIENT_STRENGTH, sample),
                resolved.read(),
            );
        }
    }
    if slice {
        let targets = SliceTargets {
            color: target.scene.color(),
            depth: target.scene.depth(),
        };
        if volume_visible {
            volume.draw_slice(
                encoder,
                SliceFrame {
                    target: targets.color,
                    region,
                    scene: table.group(),
                    level: level_group,
                    atlas: atlas_group,
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
        if let Some(splats) = splats.as_deref() {
            splats.draw_slice(encoder, targets, region, table.group());
        }
    }
    let ao_group = target.ao.as_ref().map_or(dummy_ao, crate::ao::Ao::group);
    let epoch = if translucent {
        target.oit.as_mut().map(|oit| oit.begin(encoder))
    } else {
        None
    };
    if let Some(epoch) = epoch {
        assert_eq!(
            epoch, data.oit[2],
            "the fragment store opens the epoch the frame names"
        );
    }
    let oit = target.oit.as_ref().filter(|_| translucent);
    let oit_group: &BindGroup = oit.map_or(dummy_oit.read_group(), Oit::read_group);
    if let Some(oit) = oit {
        clear.residual(encoder, oit.residual_color(), oit.residual_moment(), region);
        {
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("grana.oit.pass"),
                color_attachments: &[
                    Some(load(oit.residual_color())),
                    Some(load(oit.residual_moment())),
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
            if let Some(meshes) = meshes.as_deref() {
                meshes.draw_oit(
                    &mut pass,
                    shadow
                        .as_deref()
                        .map(Shadow::group)
                        .expect("the shadow is ready"),
                    oit.write_group(),
                    ao_group,
                );
            }
            if let Some(splats) = splats.as_deref() {
                splats.draw_oit(&mut pass, table.group(), oit.write_group());
            }
        }
    }
    if deferred
        && let Some(gbuffer) = target.gbuffer.as_ref()
        && let Some(meshes) = meshes.as_deref()
    {
        meshes.draw_deferred(
            encoder,
            region,
            target.scene.color(),
            gbuffer.group(),
            shadow
                .as_deref()
                .map(Shadow::group)
                .expect("the shadow is ready"),
            ao_group,
        );
    }
    if volume_visible
        && !slice
        && let Some(staged) = target.volume.as_ref()
    {
        volume.composite(
            encoder,
            CompositeFrame {
                target: target.scene.color(),
                region,
                scene: table.group(),
                volume: staged.group(),
                object: target.scene.depth_group(),
                oit: oit_group,
            },
        );
    }
    if let Some(oit) = oit
        && !volume_visible
    {
        oit_resolve
            .get_or_insert_with(|| OitResolve::new(device, layouts, pipelines))
            .draw(
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
