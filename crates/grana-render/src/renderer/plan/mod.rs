use super::signature::{SceneSummary, ShadowKeys, ShadowVolume, digest};
use super::view::View;
use super::*;
use crate::brick::BrickTable;
use crate::hollow::Cursor;
use crate::planner::{volume_count, volume_dispatch};
use crate::schema::Versions;
use crate::table::SceneTable;

pub(super) struct Input<'a> {
    pub(super) pyramid: &'a Pyramid,
    pub(super) camera: &'a Camera,
    pub(super) region: Region,
    pub(super) view: View,
    pub(super) target: usize,
    pub(super) hollow: Option<Cursor<'a>>,
    pub(super) denial: &'a [usize],
    pub(super) budget: usize,
}

#[derive(Clone, Copy)]
pub(super) struct Planned {
    pub(super) levels: Levels,
    pub(super) truncated: bool,
}

pub(super) fn levels(input: Input<'_>) -> Planned {
    let Input {
        pyramid,
        camera,
        region,
        view,
        target,
        hollow,
        denial,
        budget,
    } = input;
    let level_count = pyramid.level_count();
    let budget = budget.max(1);
    let mut level = level_count - 1;
    let mut truncated = true;
    for candidate in target..level_count {
        let denied = denial.get(candidate).copied().unwrap_or(0);
        let needed = volume_count(pyramid, camera, region, view.aspect, candidate, hollow) + denied;
        if needed <= budget {
            level = candidate;
            truncated = false;
            break;
        }
    }
    Planned {
        levels: Levels {
            level,
            capacity: budget,
        },
        truncated,
    }
}

pub(super) struct Tables<'a> {
    pub(super) bricks: &'a mut [Option<BrickTable>],
    pub(super) pyramid: &'a Pyramid,
    pub(super) levels: Levels,
    pub(super) atlas: &'a Atlas,
    pub(super) arena: &'a BrickArena,
    pub(super) upload: &'a mut crate::upload::Upload,
    pub(super) time: u32,
    pub(super) content: u32,
}

pub(super) fn prepare(tables: Tables<'_>) {
    let Tables {
        bricks,
        pyramid,
        levels,
        atlas,
        arena,
        upload,
        time,
        content,
    } = tables;
    let level = levels.level;
    ensure_brick(
        bricks,
        pyramid.level(level),
        level,
        atlas.slot_grid(),
        arena,
    );
    let table = bricks[level].as_mut().expect("the brick table exists");
    if table.time() != Some(time) {
        table.rebase(upload, time, content, atlas, arena);
    }
}

pub(super) fn table(bricks: &[Option<BrickTable>], levels: Levels) -> &BrickTable {
    bricks[levels.level]
        .as_ref()
        .expect("the brick table exists")
}

pub(super) fn schedule(
    planner: &mut Planner,
    queue: &Queue,
    camera: &Camera,
    table: &BrickTable,
    levels: Levels,
    time: u32,
) {
    let scale = bucket_scale(camera, table.bounds.0.to_array(), table.world.to_array());
    planner.plan.prepare(
        queue,
        Schedule {
            time,
            capacity: levels.capacity as u32,
            dispatch: volume_dispatch(table),
            bucket_scale: scale,
            base: table.record_base(),
        },
    );
}

pub(super) fn ray(
    camera: &Camera,
    table: &BrickTable,
    scale: f32,
    height: u32,
) -> Result<(f32, [u32; 4]), String> {
    let step = crate::frame::step_length(camera, table, scale);
    let bounds = crate::frame::sample_bounds(camera, table, step, scale, height)?;
    Ok((step, bounds))
}

pub(super) struct Frame<'a> {
    pub(super) camera: &'a Camera,
    pub(super) region: Region,
    pub(super) clip: Clip,
    pub(super) view: View,
    pub(super) time: u32,
    pub(super) table: &'a BrickTable,
    pub(super) scene: &'a SceneFrame<'a>,
    pub(super) ray: crate::frame::Ray,
    pub(super) space: ShadowSpace,
    pub(super) translucent: bool,
    pub(super) stale: bool,
    pub(super) content: u32,
    pub(super) channels: &'a SceneTable,
    pub(super) oit: Option<&'a Oit>,
}

pub(super) fn data(frame: Frame<'_>) -> FrameData {
    let Frame {
        camera,
        region,
        clip,
        view,
        time,
        table,
        scene,
        ray,
        space,
        translucent,
        stale,
        content,
        channels,
        oit,
    } = frame;
    let mut data = FrameData::new(
        VolumeView {
            camera,
            region,
            clip,
            aspect: view.aspect,
            height: view.height,
            time,
        },
        table,
        scene.shading,
        ray,
        space,
    );
    data.viewport = view.region.map(|value| value as f32);
    data.oit = [u32::from(translucent), 0, 0, 0];
    data.tag[1] = u32::from(stale);
    data.tag[3] = content;
    data.occupancy[0] = channels.threshold_floor();
    data.packs = channels.packs();
    if let Some(oit) = oit.filter(|_| translucent) {
        data.oit[1] = oit.layers();
        data.oit[2] = oit.next_epoch();
    }
    data
}

pub(super) struct ShadowFrame<'a> {
    pub(super) scene: &'a SceneFrame<'a>,
    pub(super) pyramid: &'a Pyramid,
    pub(super) level: usize,
    pub(super) versions: Versions,
    pub(super) summary: &'a mut Option<SceneSummary>,
    pub(super) channel: u64,
    pub(super) time: u32,
    pub(super) content: u32,
    pub(super) stale: bool,
}

#[derive(Clone, Copy)]
pub(super) struct ShadowPlan {
    pub(super) space: ShadowSpace,
    pub(super) keys: ShadowKeys,
    pub(super) level: usize,
}

impl ShadowPlan {
    pub(super) fn volume(self) -> u64 {
        self.keys.volume()
    }

    pub(super) fn objects(self) -> u64 {
        self.keys.objects()
    }
}

pub(super) fn shadow(frame: ShadowFrame<'_>) -> ShadowPlan {
    let ShadowFrame {
        scene,
        pyramid,
        level,
        versions,
        summary,
        channel,
        time,
        content,
        stale,
    } = frame;
    let finest = pyramid.level(0);
    let world = Vec3::from(finest.world_extent());
    let spacing = finest.spacing();
    let held = match *summary {
        Some(held) if held.holds(versions) => held,
        _ => {
            let held = SceneSummary::of(scene, world);
            *summary = Some(held);
            held
        }
    };
    let (low, high) = held.bounds();
    let space = ShadowSpace::new(
        scene.shading.light(),
        low,
        high,
        Vec3::new(spacing.x(), spacing.y(), spacing.z()),
    );
    let layers = digest(&[
        versions.topology(),
        versions.classification(),
        held.visibility(),
    ]);
    let keys = ShadowKeys::of(
        layers,
        ShadowVolume {
            time,
            content,
            stale,
            style: channel,
            level,
        },
        &space,
    );
    ShadowPlan { space, keys, level }
}

mod residence;

pub(crate) use residence::{Adapt, Poll, Residence};
