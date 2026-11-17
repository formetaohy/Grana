use super::super::*;
use super::Features;
use crate::table::SceneTable;
use crate::volume::MarchFrame;

const AMBIENT_REACH: f32 = 40.0;
const AMBIENT_STRENGTH: f32 = 0.9;

pub(super) struct Stage<'a> {
    pub(super) device: &'a Device,
    pub(super) queue: &'a Queue,
    pub(super) layouts: &'a Layouts,
    pub(super) encoder: &'a mut CommandEncoder,
    pub(super) target: &'a mut FrameTarget,
    pub(super) table: &'a SceneTable,
    pub(super) level: &'a BindGroup,
    pub(super) atlas: &'a BindGroup,
    pub(super) occupancy: &'a BindGroup,
    pub(super) shadow: Option<&'a BindGroup>,
    pub(super) volume_pass: &'a VolumePass,
    pub(super) clear: &'a crate::clear::Clear,
    pub(super) surface: &'a crate::surface::Pipeline,
    pub(super) meshes: Option<&'a MeshPasses>,
    pub(super) data: &'a FrameData,
    pub(super) features: Features,
    pub(super) sample: u32,
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
        table,
        level,
        atlas,
        occupancy,
        shadow,
        volume_pass,
        clear,
        surface,
        meshes,
        data,
        features,
        sample,
        dummy_normal,
        dummy_volume,
    } = stage;
    let region = features.region;
    let surfacing = features.surfaces && !features.slice && target.normal.is_some();
    clear.scene(encoder, target.scene.color(), target.scene.depth(), region);
    if surfacing && let Some(meshes) = meshes {
        meshes.draw_normal(
            encoder,
            region,
            target.normal.as_ref().expect("the normal target is ready"),
            target.scene.depth(),
        );
    }
    if features.volume
        && !features.slice
        && let Some(staged) = target.volume.as_ref()
    {
        volume_pass.march(
            encoder,
            region,
            MarchFrame {
                radiance: staged.radiance(),
                surface: staged.surface(),
                normal: staged.normal(),
                scene: table.group(),
                level,
                atlas,
                object: target.scene.depth_group(),
                occupancy,
                shadow: shadow.expect("the shadow is ready"),
            },
        );
    }
    if surfacing && target.ao.is_some() && target.surface.is_some() {
        let resolution = target.size;
        let merged = if features.volume {
            Some(target.geometry(device, layouts, dummy_normal, dummy_volume, true))
        } else {
            None
        };
        let objects = if features.volume {
            None
        } else {
            Some(target.objects(device, layouts))
        };
        let source = match (&merged, &objects) {
            (Some(group), _) => crate::surface::Source::Merged(group),
            (None, Some(group)) => crate::surface::Source::Objects(group),
            (None, None) => unreachable!("a resolved surface names its source"),
        };
        let ao = target.ao.as_ref().expect("the ambient occlusion is ready");
        let resolved = target.surface.as_ref().expect("the scene surface is ready");
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
