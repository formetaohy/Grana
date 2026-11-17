use super::signature::ShadowKeys;
use super::*;
use crate::table::SceneTable;

mod finish;
mod geometry;
mod shading;
mod shadow;

#[derive(Clone, Copy)]
pub(super) struct Features {
    pub(super) region: [u32; 4],
    pub(super) slice: bool,
    pub(super) translucent: bool,
    pub(super) volume: bool,
    pub(super) surfaces: bool,
    pub(super) ambient: bool,
}

pub(super) struct Groups<'a> {
    pub(super) level: &'a BindGroup,
    pub(super) atlas: &'a BindGroup,
    pub(super) occupancy: &'a BindGroup,
}

pub(super) struct Stage<'a> {
    pub(super) device: &'a Device,
    pub(super) queue: &'a Queue,
    pub(super) layouts: &'a Layouts,
    pub(super) encoder: &'a mut CommandEncoder,
    pub(super) target: &'a mut FrameTarget,
    pub(super) scene: &'a SceneFrame<'a>,
    pub(super) table: &'a SceneTable,
    pub(super) groups: Groups<'a>,
    pub(super) shadow: Option<&'a mut Shadow>,
    pub(super) keys: ShadowKeys,
    pub(super) volume_pass: &'a VolumePass,
    pub(super) clear: &'a crate::clear::Clear,
    pub(super) surface: &'a crate::surface::Pipeline,
    pub(super) oit_resolve: &'a OitResolve,
    pub(super) meshes: Option<&'a MeshPasses>,
    pub(super) splats: Option<&'a SplatPasses>,
    pub(super) sections: &'a Sections,
    pub(super) overlay: &'a mut Overlays,
    pub(super) data: &'a FrameData,
    pub(super) features: Features,
    pub(super) sample: u32,
    pub(super) dummy_ao: &'a BindGroup,
    pub(super) dummy_oit: &'a Oit,
    pub(super) dummy_normal: &'a TextureView,
    pub(super) dummy_volume: &'a crate::target::Unreached,
}

pub(super) fn encode(stage: Stage<'_>) {
    let Stage {
        device,
        queue,
        layouts,
        encoder,
        target,
        scene,
        table,
        groups,
        mut shadow,
        keys,
        volume_pass,
        clear,
        surface,
        oit_resolve,
        meshes,
        splats,
        sections,
        overlay,
        data,
        features,
        sample,
        dummy_ao,
        dummy_oit,
        dummy_normal,
        dummy_volume,
    } = stage;
    let Groups {
        level,
        atlas,
        occupancy,
    } = groups;
    shadow::encode(shadow::Stage {
        encoder,
        queue,
        scene,
        table,
        keys,
        shadow: shadow.as_deref_mut(),
        meshes,
        level,
        atlas,
        occupancy,
        data,
        features,
    });
    let shadow_group = shadow.as_deref().map(Shadow::group);
    geometry::encode(geometry::Stage {
        device,
        queue,
        layouts,
        encoder,
        target,
        table,
        level,
        atlas,
        occupancy,
        shadow: shadow_group,
        volume_pass,
        clear,
        surface,
        meshes,
        data,
        features,
        sample,
        dummy_normal,
        dummy_volume,
    });
    shading::encode(shading::Stage {
        encoder,
        target,
        table,
        shadow: shadow_group,
        volume_pass,
        meshes,
        splats,
        data,
        features,
        dummy_ao,
        dummy_oit,
    });
    finish::encode(finish::Stage {
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
    });
}
