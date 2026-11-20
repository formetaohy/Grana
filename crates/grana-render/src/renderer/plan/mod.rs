use super::signature::{SceneSummary, ShadowKeys, ShadowVolume, digest};
use super::view::View;
use super::*;
use crate::brick::BrickTable;
use crate::schema::Versions;
use crate::table::SceneTable;

pub(super) struct Input<'a> {
    pub(super) pyramid: &'a Pyramid,
    pub(super) camera: &'a Camera,
    pub(super) region: Region,
    pub(super) view: View,
    pub(super) arena: &'a BrickArena,
    pub(super) slots: usize,
    pub(super) cap: Option<usize>,
    pub(super) current: Option<usize>,
    pub(super) measured: bool,
    pub(super) pose: u64,
    pub(super) rejected: Option<usize>,
}

#[derive(Clone, Copy)]
pub(super) struct Planned {
    pub(super) ladder: Ladder,
    pub(super) levels: Levels,
    pub(super) overflowed: Option<usize>,
}

pub(super) fn levels(
    input: Input<'_>,
    measure: &[Option<Measured>],
    cache: &mut Option<(u64, VolumeFit)>,
) -> Planned {
    let Input {
        pyramid,
        camera,
        region,
        view,
        arena,
        slots,
        cap,
        current,
        measured,
        pose,
        rejected,
    } = input;
    let level_count = pyramid.level_count();
    let first = arena.first();
    let ceiling = cap
        .unwrap_or(level_count - 1)
        .max(first)
        .min(level_count - 1);
    let limits = Limits { first, ceiling };
    let allowed = limits.count(pyramid);
    let share = slots.max(1);
    let fit_view = FitView {
        pyramid,
        camera,
        region,
        aspect: view.aspect,
        height: view.height,
        limits: Limits { first, ceiling },
        capacity: share,
        current,
    };
    let key = digest(&[
        view.motion,
        share as u64,
        u64::from(view.height),
        ceiling as u64,
        first as u64,
        u64::from(u32::from(measured)),
        level_key(current),
        level_key(rejected),
    ]);
    let (fit, overflowed) = if measured {
        fit_measured(fit_view, measure, pose, rejected)
    } else if let Some((held, value)) = *cache
        && held == key
    {
        (value, None)
    } else {
        let computed = fit_volume(fit_view);
        *cache = Some((key, computed));
        (computed, None)
    };
    let base = fit.ladder.base.min(allowed - 1);
    let count = BAND.min(allowed - base);
    let ladder = Ladder { base, count };
    let capacity = if base == fit.ladder.base && count == fit.ladder.count {
        fit.capacity
    } else {
        capacities(share, &fit.capacity, count)
    };
    Planned {
        ladder,
        levels: Levels { ladder, capacity },
        overflowed,
    }
}

fn level_key(level: Option<usize>) -> u64 {
    level.map_or(u64::MAX, |level| level as u64)
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
    let slot_grid = atlas.slot_grid();
    let mut prepared = [usize::MAX; BAND];
    let mut count = 0;
    for level in levels.ladder.levels() {
        if prepared[..count].contains(&level) {
            continue;
        }
        prepared[count] = level;
        count += 1;
        ensure_brick(bricks, pyramid.level(level), level, slot_grid, arena);
        let table = bricks[level].as_mut().expect("the brick table exists");
        if table.time() != Some(time) {
            table.rebase(upload, time, content, atlas, arena);
        }
    }
}

pub(super) fn tables(bricks: &[Option<BrickTable>], levels: Levels) -> Vec<&BrickTable> {
    levels
        .ladder
        .levels()
        .map(|level| bricks[level].as_ref().expect("the band table exists"))
        .collect()
}

pub(super) fn schedule(
    planner: &mut Planner,
    queue: &Queue,
    camera: &Camera,
    tables: &[&BrickTable],
    levels: Levels,
    time: u32,
) {
    let scale = bucket_scale(
        camera,
        tables[0].bounds.0.to_array(),
        tables[0].world.to_array(),
    );
    for (band, view) in planner.view.bands.iter_mut().enumerate() {
        let (capacity, dispatch, base) = match tables.get(band) {
            Some(table) => (
                levels.capacity.get(band).copied().unwrap_or(0) as u32,
                volume_dispatch(table),
                table.record_base(),
            ),
            None => (0, 0, 0),
        };
        view.prepare(
            queue,
            Schedule {
                time,
                band: band as u32,
                capacity,
                dispatch,
                bucket_scale: scale,
                base,
            },
        );
    }
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
    pub(super) ladder: Ladder,
    pub(super) tables: &'a [&'a BrickTable],
    pub(super) scene: &'a SceneFrame<'a>,
    pub(super) ray: crate::frame::Ray,
    pub(super) space: ShadowSpace,
    pub(super) translucent: bool,
    pub(super) stale: bool,
    pub(super) content: u32,
    pub(super) table: &'a SceneTable,
    pub(super) oit: Option<&'a Oit>,
}

pub(super) fn data(frame: Frame<'_>) -> FrameData {
    let Frame {
        camera,
        region,
        clip,
        view,
        time,
        ladder,
        tables,
        scene,
        ray,
        space,
        translucent,
        stale,
        content,
        table,
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
        ladder,
        tables,
        scene.shading,
        ray,
        space,
    );
    data.viewport = view.region.map(|value| value as f32);
    data.oit = [u32::from(translucent), 0, 0, 0];
    data.tag[1] = u32::from(stale);
    data.tag[3] = content;
    data.occupancy[0] = table.threshold_floor();
    data.packs = table.packs();
    if let Some(oit) = oit.filter(|_| translucent) {
        data.oit[1] = oit.layers();
        data.oit[2] = oit.next_epoch();
    }
    data
}

pub(super) struct ShadowFrame<'a> {
    pub(super) scene: &'a SceneFrame<'a>,
    pub(super) pyramid: &'a Pyramid,
    pub(super) ladder: Ladder,
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
        ladder,
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
    let level = ladder.base + ladder.count - 1;
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

pub(crate) use residence::{Adapt, Residence};
