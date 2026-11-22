use super::*;
use glam::Mat4;

const QUANTUM_BRICK: f32 = 0.5;

const QUANTUM_FLOOR: f32 = 0.5;

const QUANTUM_CEILING: f32 = 24.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Levels {
    pub(super) ladder: Ladder,
    pub(super) capacity: [usize; BAND],
}

#[derive(Clone, Copy)]
pub(super) struct Resolution {
    pub(super) length: f32,
    pub(super) angle: f32,
}

impl Resolution {
    pub(super) fn of(pyramid: &Pyramid, level: usize, camera: &Camera, height: u32) -> Self {
        let level = pyramid.level(level.min(pyramid.level_count() - 1));
        let voxel = level.voxel().into_iter().fold(0.0f32, f32::max);
        let brick = grana_volume::BRICK_SIDE as f32 * voxel;
        let point = camera.world_per_point(height.max(1) as f32, camera.distance());
        let length = (QUANTUM_BRICK * brick).clamp(QUANTUM_FLOOR * point, QUANTUM_CEILING * point);
        let reach = (camera.pivot() - camera.eye()).length().max(length);
        Self {
            length,
            angle: length / reach,
        }
    }

    pub(super) fn position(self, values: [f32; 3]) -> [i32; 3] {
        values.map(|value| quantise(value, self.length))
    }

    pub(super) fn direction(self, values: [f32; 3]) -> [i32; 3] {
        values.map(|value| quantise(value, self.angle))
    }
}

pub(super) fn quantise(value: f32, step: f32) -> i32 {
    assert!(
        step > 0.0 && step.is_finite(),
        "a view quantum is a positive length"
    );
    (value / step)
        .round()
        .clamp(i32::MIN as f32, i32::MAX as f32) as i32
}

#[derive(Clone, Copy, PartialEq)]
struct RegionKey {
    enabled: bool,
    center: [i32; 3],
    half: [i32; 3],
    axes: [[i32; 3]; 3],
}

impl RegionKey {
    fn of(region: Region, quantum: Resolution) -> Self {
        Self {
            enabled: region.enabled(),
            center: quantum.position(region.center().to_array()),
            half: quantum.position(region.half().to_array()),
            axes: [0, 1, 2].map(|axis| quantum.direction(region.axis(axis).to_array())),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Hash)]
pub(super) struct SliceKey {
    axis: usize,
    right: [i32; 3],
    up: [i32; 3],
    step: i32,
}

impl SliceKey {
    pub(super) fn of(slice: grana_display::Slice, quantum: Resolution) -> Self {
        Self {
            axis: slice.axis(),
            right: quantum.direction(slice.right().to_array()),
            up: quantum.direction(slice.up().to_array()),
            step: quantise(slice.step(), quantum.length),
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(super) struct ViewKey {
    pub(super) epoch: u64,
    pub(super) data: u64,
    pub(super) prefetch: bool,
    pub(super) assured: bool,
    pub(super) time: u32,
    pub(super) size: [u32; 2],
    pub(super) levels: Levels,
    region: RegionKey,
    eye: [i32; 3],
    focus: [i32; 3],
    field_of_view: i32,
    projection: grana_display::CameraProjection,
    slice: Option<SliceKey>,
}

#[derive(Clone, Copy, PartialEq)]
pub(super) struct Mode {
    pub(super) prefetch: bool,
    pub(super) assured: bool,
}

pub(super) struct ViewState<'a> {
    pub(super) camera: &'a Camera,
    pub(super) size: [u32; 2],
    pub(super) region: Region,
    pub(super) levels: Levels,
    pub(super) quantum: Resolution,
}

impl ViewKey {
    pub(super) fn of(epoch: u64, data: u64, mode: Mode, time: u32, view: ViewState<'_>) -> Self {
        let ViewState {
            camera,
            size,
            region,
            levels,
            quantum,
        } = view;
        Self {
            epoch,
            data,
            prefetch: mode.prefetch,
            assured: mode.assured,
            time,
            size,
            levels,
            region: RegionKey::of(region, quantum),
            eye: quantum.position(camera.eye().to_array()),
            focus: quantum.position(camera.pivot().to_array()),
            field_of_view: quantise(camera.fov_y(), quantum.angle),
            projection: camera.projection(),
            slice: camera.slice().map(|slice| SliceKey::of(slice, quantum)),
        }
    }

    pub(super) fn same_view(&self, other: &Self) -> bool {
        self.epoch == other.epoch
            && self.prefetch == other.prefetch
            && self.assured == other.assured
            && self.time == other.time
            && self.size == other.size
            && self.levels == other.levels
            && self.region == other.region
            && self.eye == other.eye
            && self.focus == other.focus
            && self.field_of_view == other.field_of_view
            && self.projection == other.projection
            && self.slice == other.slice
    }

    pub(super) fn prefetch_digest(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.epoch.hash(&mut hasher);
        self.prefetch.hash(&mut hasher);
        self.assured.hash(&mut hasher);
        self.size.hash(&mut hasher);
        self.levels.ladder.hash(&mut hasher);
        self.levels.capacity.hash(&mut hasher);
        self.region.enabled.hash(&mut hasher);
        self.region.center.hash(&mut hasher);
        self.region.half.hash(&mut hasher);
        self.region.axes.hash(&mut hasher);
        self.eye.hash(&mut hasher);
        self.focus.hash(&mut hasher);
        self.field_of_view.hash(&mut hasher);
        self.projection.index().hash(&mut hasher);
        self.slice
            .map(|slice| (slice.axis, slice.right, slice.up, slice.step))
            .hash(&mut hasher);
        hasher.finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct LevelKey {
    pub(super) base: usize,
    pub(super) count: usize,
}

impl LevelKey {
    pub(super) fn of(levels: Levels) -> Self {
        Self {
            base: levels.ladder.base,
            count: levels.ladder.count,
        }
    }
}

pub(super) struct TargetCache {
    slots: [Option<FrameTarget>; 2],
    current: usize,
}

impl TargetCache {
    pub(super) fn new() -> Self {
        Self {
            slots: [None, None],
            current: 0,
        }
    }

    pub(super) fn clear(&mut self) {
        self.slots = [None, None];
        self.current = 0;
    }

    pub(super) fn current(&self) -> Option<&FrameTarget> {
        self.slots[self.current].as_ref()
    }

    pub(super) fn iter_mut(&mut self) -> impl Iterator<Item = &mut FrameTarget> {
        self.slots.iter_mut().flatten()
    }

    pub(super) fn take(
        &mut self,
        device: &Device,
        layouts: &Layouts,
        present: &Present,
        size: [u32; 2],
    ) -> (FrameTarget, bool) {
        let held = (0..self.slots.len()).find(|index| {
            self.slots[*index]
                .as_ref()
                .is_some_and(|target| target.size == size)
        });
        let (slot, created) = match held {
            Some(index) => (index, false),
            None => {
                let index = (0..self.slots.len())
                    .find(|index| *index != self.current)
                    .unwrap_or(0);
                self.slots[index] = Some(FrameTarget::new(device, layouts, present, size));
                (index, true)
            }
        };
        self.current = slot;
        (
            self.slots[slot].take().expect("the frame target is ready"),
            created,
        )
    }

    pub(super) fn put(&mut self, target: FrameTarget) {
        self.slots[self.current] = Some(target);
    }
}

pub(super) struct FrameTarget {
    pub(super) scene: SceneTarget,
    pub(super) temporal: crate::temporal::History,
    pub(super) present_group: BindGroup,
    pub(super) fade: Option<FadeTarget>,
    pub(super) oit: Option<Oit>,
    pub(super) geometry: Option<(GeometryKey, BindGroup)>,
    pub(super) objects: Option<BindGroup>,
    pub(super) normal: Option<crate::target::NormalTarget>,
    pub(super) ao: Option<crate::ao::Ao>,
    pub(super) surface: Option<crate::surface::Target>,
    pub(super) volume: Option<crate::target::Volume>,
    pub(super) size: [u32; 2],
    pub(super) layers: Option<u64>,
    pub(super) content: Option<u64>,
    pub(super) classification: Option<u64>,
    pub(super) appearance: Option<u64>,
    pub(super) detail: Option<u64>,
    pub(super) lens: Option<u64>,
    pub(super) camera: Option<Mat4>,
    pub(super) levels: Option<LevelKey>,
    pub(super) age: u32,
    pub(super) phase: u32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct GeometryKey {
    pub(super) object: bool,
    pub(super) volume: bool,
}

pub(super) struct FadeTarget {
    texture: Texture,
    group: BindGroup,
}

impl FrameTarget {
    pub(super) fn new(
        device: &Device,
        layouts: &Layouts,
        present: &Present,
        size: [u32; 2],
    ) -> Self {
        let scene = SceneTarget::new(device, &layouts.depth, size);
        let temporal = crate::temporal::History::new(device, size);
        let present_group = present.bind(device, temporal.resolved(), temporal.resolved());
        Self {
            scene,
            temporal,
            present_group,
            fade: None,
            oit: None,
            geometry: None,
            objects: None,
            normal: None,
            ao: None,
            surface: None,
            volume: None,
            size,
            layers: None,
            content: None,
            classification: None,
            appearance: None,
            detail: None,
            lens: None,
            camera: None,
            levels: None,
            age: 0,
            phase: 0,
        }
    }

    pub(super) fn geometry(
        &mut self,
        device: &Device,
        layouts: &Layouts,
        dummy_normal: &TextureView,
        dummy_volume: &crate::target::Unreached,
        volume: bool,
    ) -> BindGroup {
        let key = GeometryKey {
            object: self.normal.is_some(),
            volume,
        };
        let stale = self.geometry.as_ref().is_none_or(|(held, _)| *held != key);
        if stale {
            let normal = self
                .normal
                .as_ref()
                .map_or(dummy_normal, crate::target::NormalTarget::view);
            let (surface, volume_normal) = match self.volume.as_ref().filter(|_| volume) {
                Some(volume) => (volume.surface(), volume.normal()),
                None => (dummy_volume.surface(), dummy_volume.normal()),
            };
            let group = device.create_bind_group(&BindGroupDescriptor {
                label: Some("grana.geometry.group"),
                layout: &layouts.geometry,
                entries: &[
                    BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(self.scene.depth()),
                    },
                    BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(normal),
                    },
                    BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(surface),
                    },
                    BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(volume_normal),
                    },
                ],
            });
            self.geometry = Some((key, group));
        }
        self.geometry
            .as_ref()
            .expect("the geometry group is ready")
            .1
            .clone()
    }

    pub(super) fn objects(&mut self, device: &Device, layouts: &Layouts) -> BindGroup {
        let stale = self.objects.is_none();
        if stale {
            let normal = self
                .normal
                .as_ref()
                .expect("the object surface reads the normal target")
                .view();
            let group = device.create_bind_group(&BindGroupDescriptor {
                label: Some("grana.object.surface.group"),
                layout: &layouts.object_surface,
                entries: &[
                    BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(self.scene.depth()),
                    },
                    BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(normal),
                    },
                ],
            });
            self.objects = Some(group);
        }
        self.objects
            .as_ref()
            .expect("the object surface group is ready")
            .clone()
    }

    pub(super) fn fade_group(&mut self, device: &Device, present: &Present) -> &BindGroup {
        let Self {
            temporal,
            fade,
            size,
            ..
        } = self;
        let size = *size;
        let held = fade.get_or_insert_with(|| {
            let texture = crate::target::image(
                device,
                "grana.fade",
                size,
                crate::target::SCENE_FORMAT,
                TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            );
            let view = crate::target::view(&texture);
            let group = present.bind(device, temporal.resolved(), &view);
            FadeTarget { texture, group }
        });
        &held.group
    }

    pub(super) fn snapshot(&mut self, encoder: &mut CommandEncoder, region: [u32; 4]) {
        let Some(fade) = self.fade.as_ref() else {
            return;
        };
        let extent = Extent3d {
            width: region[2].max(1),
            height: region[3].max(1),
            depth_or_array_layers: 1,
        };
        encoder.copy_texture_to_texture(
            TexelCopyTextureInfo {
                texture: self.temporal.resolved_texture(),
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: region[0],
                    y: region[1],
                    z: 0,
                },
                aspect: TextureAspect::All,
            },
            TexelCopyTextureInfo {
                texture: &fade.texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: region[0],
                    y: region[1],
                    z: 0,
                },
                aspect: TextureAspect::All,
            },
            extent,
        );
    }

    pub(super) fn resolved(&self) -> &TextureView {
        self.temporal.resolved()
    }
}
