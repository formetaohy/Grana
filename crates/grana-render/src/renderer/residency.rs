use super::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Levels {
    pub(super) ladder: Ladder,
    pub(super) capacity: [usize; BAND],
}

#[derive(Clone, Copy, PartialEq)]
pub(super) struct Residency {
    pub(super) epoch: u64,
    pub(super) content: u64,
    pub(super) style: u64,
    pub(super) prefetch: bool,
    pub(super) assured: bool,
    pub(super) time: u32,
    pub(super) region: Region,
    pub(super) eye: [f32; 3],
    pub(super) focus: [f32; 3],
    pub(super) field_of_view: f32,
    pub(super) size: [u32; 2],
    pub(super) levels: Levels,
}

#[derive(Clone, Copy, PartialEq)]
pub(super) struct Content {
    pub(super) content: u64,
    pub(super) style: u64,
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
}

impl Residency {
    pub(super) fn of(
        epoch: u64,
        content: Content,
        mode: Mode,
        time: u32,
        view: ViewState<'_>,
    ) -> Self {
        let ViewState {
            camera,
            size,
            region,
            levels,
        } = view;
        Self {
            epoch,
            content: content.content,
            style: content.style,
            prefetch: mode.prefetch,
            assured: mode.assured,
            time,
            region,
            eye: camera.eye().to_array(),
            focus: camera.pivot().to_array(),
            field_of_view: camera.fov_y(),
            size,
            levels,
        }
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

pub(super) struct FrameTarget {
    pub(super) scene: SceneTarget,
    pub(super) accumulation: Texture,
    pub(super) accumulation_view: TextureView,
    pub(super) count: Texture,
    pub(super) count_view: TextureView,
    pub(super) accumulate_group: BindGroup,
    pub(super) present_group: BindGroup,
    pub(super) fade: Option<FadeTarget>,
    pub(super) oit: Option<Oit>,
    pub(super) ao: Option<crate::ao::Ao>,
    pub(super) size: [u32; 2],
    pub(super) samples: u32,
    pub(super) signature: Option<u64>,
    pub(super) transition: Option<u64>,
    pub(super) levels: Option<LevelKey>,
    pub(super) shot: Option<u64>,
    pub(super) view: FrameView,
    pub(super) history: Option<crate::temporal::History>,
    pub(super) seeded: bool,
    pub(super) holds: bool,
}

pub(super) struct FadeTarget {
    pub(super) texture: Texture,
    pub(super) count: Texture,
    pub(super) group: BindGroup,
}

impl FrameTarget {
    pub(super) fn new(
        device: &Device,
        layouts: &Layouts,
        present: &Present,
        size: [u32; 2],
    ) -> Self {
        let scene = SceneTarget::new(device, &layouts.depth, size);
        let accumulation = target::accumulation(device, size);
        let accumulation_view = target::view(&accumulation);
        let count = target::count(device, size);
        let count_view = target::view(&count);
        let accumulate_group = present.bind(
            device,
            scene.color(),
            scene.color(),
            scene.color(),
            scene.color(),
        );
        let present_group = present.bind(
            device,
            &accumulation_view,
            &count_view,
            &accumulation_view,
            &count_view,
        );
        Self {
            scene,
            accumulation,
            accumulation_view,
            count,
            count_view,
            accumulate_group,
            present_group,
            fade: None,
            oit: None,
            ao: None,
            size,
            samples: 0,
            signature: None,
            transition: None,
            levels: None,
            shot: None,
            view: FrameView::default(),
            history: None,
            seeded: false,
            holds: false,
        }
    }

    pub(super) fn fade_group(&mut self, device: &Device, present: &Present) -> &BindGroup {
        let fade = self.fade.get_or_insert_with(|| {
            let texture = target::accumulation(device, self.size);
            let view = target::view(&texture);
            let count = target::count(device, self.size);
            let count_view = target::view(&count);
            let group = present.bind(
                device,
                &self.accumulation_view,
                &self.count_view,
                &view,
                &count_view,
            );
            FadeTarget {
                texture,
                count,
                group,
            }
        });
        &fade.group
    }

    pub(super) fn snapshot(&mut self, encoder: &mut CommandEncoder) {
        let Some(fade) = self.fade.as_ref() else {
            return;
        };
        let extent = wgpu::Extent3d {
            width: self.size[0],
            height: self.size[1],
            depth_or_array_layers: 1,
        };
        for (source, destination) in [
            (&self.accumulation, &fade.texture),
            (&self.count, &fade.count),
        ] {
            encoder.copy_texture_to_texture(
                TexelCopyTextureInfo {
                    texture: source,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: TextureAspect::All,
                },
                TexelCopyTextureInfo {
                    texture: destination,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: TextureAspect::All,
                },
                extent,
            );
        }
    }
}
