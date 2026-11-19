use super::super::plan::ShadowPlan;
use super::super::*;
use super::Features;
use crate::brick::BrickTable;
use crate::shadow::TauStage;
use crate::table::SceneTable;

pub(super) struct Stage<'a> {
    pub(super) encoder: &'a mut CommandEncoder,
    pub(super) queue: &'a Queue,
    pub(super) scene: &'a SceneFrame<'a>,
    pub(super) table: &'a SceneTable,
    pub(super) keys: ShadowPlan,
    pub(super) shadow: Option<&'a mut Shadow>,
    pub(super) meshes: Option<&'a MeshPasses>,
    pub(super) level: &'a BindGroup,
    pub(super) atlas: &'a BindGroup,
    pub(super) occupancy: &'a BindGroup,
    pub(super) bricks: &'a [Option<BrickTable>],
    pub(super) data: &'a FrameData,
    pub(super) features: Features,
}

pub(super) fn encode(stage: Stage<'_>) {
    let Stage {
        encoder,
        queue,
        scene,
        table,
        keys,
        shadow,
        meshes,
        level,
        atlas,
        occupancy,
        bricks,
        data,
        features,
    } = stage;
    let Some(shadow) = shadow.filter(|_| !features.slice && scene.shading.shadows > 0.0) else {
        return;
    };
    if meshes.is_none() && !features.volume {
        return;
    }
    let tau_table = bricks[keys.level]
        .as_ref()
        .expect("the shadow band carries a table");
    let groups = ShadowGroups {
        scene: table.shadow_group(),
        level,
        atlas,
        occupancy,
    };
    if let Some(meshes) = meshes {
        let object = keys.objects();
        if shadow.object_key() != Some(object) {
            shadow.set_object_key(object);
            shadow.clear_objects(encoder);
            meshes.draw_shadow(encoder, shadow.objects());
        }
    }
    if shadow.pending(keys.level, keys.volume()) {
        table.write_shadow_frame(queue, *data);
    }
    shadow.encode_tau(TauStage {
        encoder,
        queue,
        groups,
        key: keys.volume(),
        level: keys.level,
        table: tau_table,
        space: &keys.space,
    });
}
