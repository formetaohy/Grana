use glam::{Mat4, Vec3, Vec4};
use grana_volume::{Axis, BRICK_SIDE, BrickCoord, BrickGrid, Pyramid};

use crate::BrickRequest;
use crate::atlas::Atlas;
use crate::level::LevelCache;

const SUPERCELL: u32 = 4;

pub(crate) fn select_volume_level(
    pyramid: &Pyramid,
    active: Option<usize>,
    camera: &crate::Camera,
    height: f32,
) -> usize {
    let pixel_world = 2.0 * camera.distance() * (camera.fov_y() * 0.5).tan() / height.max(1.0);
    let mut level = 0;
    for index in 0..pyramid.level_count() {
        let voxel = minimum_spacing(pyramid, index);
        if voxel <= pixel_world {
            level = index;
        } else {
            break;
        }
    }
    if let Some(active) = active {
        let voxel = minimum_spacing(pyramid, active);
        if level < active && pixel_world >= voxel * 0.7 {
            level = active;
        }
        if level > active && pixel_world <= voxel * 1.4 {
            level = active;
        }
    }
    level
}

pub(crate) fn select_section_level(
    pyramid: &Pyramid,
    active: Option<usize>,
    section: &crate::SectionCamera,
    height: f32,
) -> usize {
    let pixel_world = section.span() / height.max(1.0);
    let mut level = 0;
    for index in 0..pyramid.level_count() {
        if section_spacing(pyramid, index, section.axis()) <= pixel_world {
            level = index;
        } else {
            break;
        }
    }
    if let Some(active) = active {
        let spacing = section_spacing(pyramid, active, section.axis());
        if level < active && pixel_world >= spacing * 0.7 {
            level = active;
        }
        if level > active && pixel_world <= spacing * 1.4 {
            level = active;
        }
    }
    level
}

fn minimum_spacing(pyramid: &Pyramid, level: usize) -> f32 {
    let spacing = pyramid.level(level).spacing();
    spacing.x().min(spacing.y()).min(spacing.z())
}

fn section_spacing(pyramid: &Pyramid, level: usize, axis: Axis) -> f32 {
    let spacing = pyramid.level(level).spacing();
    let (u, v) = axis.in_plane();
    u.spacing(spacing).max(v.spacing(spacing))
}

pub(crate) struct Residency<'a> {
    pub pyramid: &'a Pyramid,
    pub levels: &'a mut [Option<LevelCache>],
    pub atlas: &'a mut Atlas,
    pub capacity: usize,
    pub frame: u64,
}

impl Residency<'_> {
    pub fn stabilize(
        &mut self,
        active: &mut Option<usize>,
        target: usize,
        requests: &mut Vec<BrickRequest>,
        visible: impl Fn(usize, Option<usize>) -> Option<Vec<(BrickCoord, f32)>>,
    ) -> (usize, usize, usize) {
        let mut target_level = target;
        let target_visible = loop {
            match visible(target_level, Some(self.capacity)) {
                Some(list) => break list,
                None if target_level + 1 < self.pyramid.level_count() => target_level += 1,
                None => {
                    let mut list =
                        visible(target_level, None).expect("an unbounded query completes");
                    list.truncate(self.capacity);
                    break list;
                }
            }
        };
        let mut render_level = active.unwrap_or(target_level);
        if active.is_none() {
            *active = Some(render_level);
        }
        let mut switch = render_level == target_level;
        let mut active_visible = Vec::new();
        if render_level != target_level {
            match visible(render_level, Some(self.capacity)) {
                Some(list) => {
                    let fits = list.len() + target_visible.len() <= self.capacity;
                    let ready = !target_visible.is_empty()
                        && count_missing(self.levels, target_level, &target_visible) == 0;
                    if !fits || ready {
                        switch = true;
                    } else {
                        active_visible = list;
                    }
                }
                None => switch = true,
            }
        }
        if switch {
            *active = Some(target_level);
            render_level = target_level;
            let (visible, resident) = collect(
                self.levels,
                self.atlas,
                render_level,
                &target_visible,
                self.frame,
                requests,
            );
            (render_level, visible, resident)
        } else {
            collect(
                self.levels,
                self.atlas,
                target_level,
                &target_visible,
                self.frame,
                requests,
            );
            let (visible, resident) = collect(
                self.levels,
                self.atlas,
                render_level,
                &active_visible,
                self.frame,
                requests,
            );
            (render_level, visible, resident)
        }
    }
}

fn collect(
    levels: &mut [Option<LevelCache>],
    atlas: &mut Atlas,
    level: usize,
    visible: &[(BrickCoord, f32)],
    frame: u64,
    requests: &mut Vec<BrickRequest>,
) -> (usize, usize) {
    let mut resident = 0;
    for (coord, _) in visible {
        let slot = levels[level]
            .as_ref()
            .map_or(0, |cache| cache.slot(cache.grid.index(*coord)));
        if slot == 0 {
            requests.push(BrickRequest {
                level,
                coord: *coord,
            });
        } else {
            resident += 1;
            atlas.mark(slot - 1, frame);
        }
    }
    (visible.len(), resident)
}

fn count_missing(
    levels: &[Option<LevelCache>],
    level: usize,
    visible: &[(BrickCoord, f32)],
) -> usize {
    let Some(cache) = &levels[level] else {
        return visible.len();
    };
    visible
        .iter()
        .filter(|(coord, _)| cache.slot(cache.grid.index(*coord)) == 0)
        .count()
}

pub(crate) fn collect_visible(
    pyramid: &Pyramid,
    level_index: usize,
    camera: &crate::Camera,
    aspect: f32,
    limit: Option<usize>,
) -> Option<Vec<(BrickCoord, f32)>> {
    let level = pyramid.level(level_index);
    let extent = level.extent();
    let grid = BrickGrid::new(extent);
    let world = Vec3::from(level.world_extent());
    let voxel = world / Vec3::new(extent.x() as f32, extent.y() as f32, extent.z() as f32);
    let box_min = -0.5 * world;
    let planes = frustum_planes(camera.view_projection(aspect, 0.5 * world.length()));
    let eye = camera.eye();
    let counts = grid.counts();
    let supercells = counts.map(|count| count.div_ceil(SUPERCELL));
    let mut visible = Vec::new();
    for z in 0..supercells[2] {
        for y in 0..supercells[1] {
            for x in 0..supercells[0] {
                let origin = [
                    x * SUPERCELL * BRICK_SIDE,
                    y * SUPERCELL * BRICK_SIDE,
                    z * SUPERCELL * BRICK_SIDE,
                ];
                let size = [
                    (extent.x() - origin[0]).min(SUPERCELL * BRICK_SIDE),
                    (extent.y() - origin[1]).min(SUPERCELL * BRICK_SIDE),
                    (extent.z() - origin[2]).min(SUPERCELL * BRICK_SIDE),
                ];
                let minimum = box_min + Vec3::from(origin.map(|value| value as f32)) * voxel;
                let maximum = minimum + Vec3::from(size.map(|value| value as f32)) * voxel;
                if outside(&planes, minimum, maximum) {
                    continue;
                }
                let bricks = [
                    size[0].div_ceil(BRICK_SIDE),
                    size[1].div_ceil(BRICK_SIDE),
                    size[2].div_ceil(BRICK_SIDE),
                ];
                for brick_z in 0..bricks[2] {
                    for brick_y in 0..bricks[1] {
                        for brick_x in 0..bricks[0] {
                            let coord = BrickCoord::new(
                                x * SUPERCELL + brick_x,
                                y * SUPERCELL + brick_y,
                                z * SUPERCELL + brick_z,
                            );
                            let brick_origin = grid.origin(coord);
                            let brick_size = grid.extent_of(coord);
                            let minimum = box_min
                                + Vec3::from(brick_origin.map(|value| value as f32)) * voxel;
                            let maximum = minimum
                                + Vec3::new(
                                    brick_size.x() as f32,
                                    brick_size.y() as f32,
                                    brick_size.z() as f32,
                                ) * voxel;
                            if outside(&planes, minimum, maximum) {
                                continue;
                            }
                            let center = 0.5 * (minimum + maximum);
                            visible.push((coord, (center - eye).length_squared()));
                            if let Some(limit) = limit
                                && visible.len() > limit
                            {
                                return None;
                            }
                        }
                    }
                }
            }
        }
    }
    visible.sort_by(|left, right| left.1.total_cmp(&right.1));
    Some(visible)
}

pub(crate) fn visible_section(
    pyramid: &Pyramid,
    level_index: usize,
    camera: &crate::Camera,
    section: &crate::SectionCamera,
    aspect: f32,
    limit: Option<usize>,
) -> Option<Vec<(BrickCoord, f32)>> {
    let level = pyramid.level(level_index);
    let extent = level.extent();
    let grid = BrickGrid::new(extent);
    let spacing = level.spacing();
    let world = level.world_extent();
    let dims = [extent.x() as f32, extent.y() as f32, extent.z() as f32];
    let voxel = [spacing.x(), spacing.y(), spacing.z()];
    let axis = section.axis().index();
    let (u_axis, v_axis) = section.axis().in_plane();
    let (u, v) = (u_axis.index(), v_axis.index());
    let focus = camera.focus();
    let layer = ((focus[axis] / world[axis] + 0.5) * dims[axis])
        .floor()
        .clamp(0.0, dims[axis] - 1.0);
    let center_u = focus[u] / world[u] + 0.5;
    let center_v = focus[v] / world[v] + 0.5;
    let half_u = section.span() * aspect / (2.0 * world[u]);
    let half_v = section.span() / (2.0 * world[v]);
    let umin = (center_u - half_u).clamp(0.0, 1.0);
    let umax = (center_u + half_u).clamp(0.0, 1.0);
    let vmin = (center_v - half_v).clamp(0.0, 1.0);
    let vmax = (center_v + half_v).clamp(0.0, 1.0);
    if umin >= umax || vmin >= vmax {
        return Some(Vec::new());
    }
    let u0 = ((umin * dims[u]).floor() as u32).min(u_axis.extent(extent) - 1);
    let u1 = ((umax * dims[u]).ceil() as u32)
        .saturating_sub(1)
        .min(u_axis.extent(extent) - 1);
    let v0 = ((vmin * dims[v]).floor() as u32).min(v_axis.extent(extent) - 1);
    let v1 = ((vmax * dims[v]).ceil() as u32)
        .saturating_sub(1)
        .min(v_axis.extent(extent) - 1);
    let layer_brick = (layer as u32) / BRICK_SIDE;
    let mut visible = Vec::new();
    for brick_v in v0 / BRICK_SIDE..=v1 / BRICK_SIDE {
        for brick_u in u0 / BRICK_SIDE..=u1 / BRICK_SIDE {
            let mut coord = [0u32; 3];
            coord[axis] = layer_brick;
            coord[u] = brick_u;
            coord[v] = brick_v;
            let coord = BrickCoord::new(coord[0], coord[1], coord[2]);
            let origin = grid.origin(coord);
            let size = grid.extent_of(coord);
            let mut center = [0.0f32; 3];
            for component in 0..3 {
                let low = origin[component] as f32 * voxel[component] - 0.5 * world[component];
                let high = low + Axis::ALL[component].extent(size) as f32 * voxel[component];
                center[component] = 0.5 * (low + high);
            }
            let distance = Vec3::from(center) - focus;
            visible.push((coord, distance.length_squared()));
            if let Some(limit) = limit
                && visible.len() > limit
            {
                return None;
            }
        }
    }
    visible.sort_by(|left, right| left.1.total_cmp(&right.1));
    Some(visible)
}

fn frustum_planes(view_projection: Mat4) -> [Vec4; 6] {
    let row0 = view_projection.row(0);
    let row1 = view_projection.row(1);
    let row2 = view_projection.row(2);
    let row3 = view_projection.row(3);
    [
        row3 + row0,
        row3 - row0,
        row3 + row1,
        row3 - row1,
        row2,
        row3 - row2,
    ]
    .map(|plane| plane / plane.truncate().length())
}

fn outside(planes: &[Vec4; 6], minimum: Vec3, maximum: Vec3) -> bool {
    planes.iter().any(|plane| {
        let positive = Vec3::new(
            if plane.x >= 0.0 { maximum.x } else { minimum.x },
            if plane.y >= 0.0 { maximum.y } else { minimum.y },
            if plane.z >= 0.0 { maximum.z } else { minimum.z },
        );
        plane.x * positive.x + plane.y * positive.y + plane.z * positive.z + plane.w < 0.0
    })
}
