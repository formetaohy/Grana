use super::*;

#[derive(Clone, Copy, PartialEq)]
pub(super) struct Levels {
    pub(super) ladder: Ladder,
    pub(super) capacity: [usize; BAND],
    pub(super) section: usize,
    pub(super) section_fallback: usize,
    pub(super) section_capacity: usize,
}

#[derive(Clone, Copy)]
pub(super) struct Regions {
    pub(super) volume: [u32; 4],
    pub(super) section: [u32; 4],
}

#[derive(Clone, Copy, PartialEq)]
pub(super) struct Residency {
    pub(super) epoch: u64,
    pub(super) content: u64,
    pub(super) style: u64,
    pub(super) prefetch: bool,
    pub(super) time: u32,
    pub(super) eye: [f32; 3],
    pub(super) focus: [f32; 3],
    pub(super) field_of_view: f32,
    pub(super) section_center: [f32; 3],
    pub(super) section_normal: [f32; 3],
    pub(super) section_right: [f32; 3],
    pub(super) span: f32,
    pub(super) volume: [u32; 2],
    pub(super) section: [u32; 2],
    pub(super) region: Region,
    pub(super) levels: Levels,
}

pub(super) struct ResidencyView<'a> {
    pub(super) camera: &'a Camera,
    pub(super) section: &'a SectionCamera,
    pub(super) volume_region: [u32; 4],
    pub(super) section_region: [u32; 4],
    pub(super) region: Region,
}

impl Residency {
    pub(super) fn of(
        epoch: u64,
        content: u64,
        style: u64,
        prefetch: bool,
        time: u32,
        view: ResidencyView<'_>,
        levels: Levels,
    ) -> Self {
        Self {
            epoch,
            content,
            style,
            prefetch,
            time,
            eye: view.camera.eye().to_array(),
            focus: view.camera.focus().to_array(),
            field_of_view: view.camera.fov_y(),
            section_center: view.section.center().to_array(),
            section_normal: view.section.normal().to_array(),
            section_right: view.section.right().to_array(),
            span: view.section.span(),
            volume: [view.volume_region[2], view.volume_region[3]],
            section: [view.section_region[2], view.section_region[3]],
            region: view.region,
            levels,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct LevelKey {
    pub(super) base: usize,
    pub(super) count: usize,
    pub(super) section: usize,
}

impl LevelKey {
    pub(super) fn of(levels: Levels) -> Self {
        Self {
            base: levels.ladder.base,
            count: levels.ladder.count,
            section: levels.section,
        }
    }
}

pub(super) struct FrameTarget {
    pub(super) scene: SceneTarget,
    pub(super) accumulation: Texture,
    pub(super) accumulation_view: TextureView,
    pub(super) accumulate_group: BindGroup,
    pub(super) present_group: BindGroup,
    pub(super) fade: Option<FadeTarget>,
    pub(super) oit: Option<Oit>,
    pub(super) size: [u32; 2],
    pub(super) samples: u32,
    pub(super) signature: Option<u64>,
    pub(super) transition: Option<u64>,
    pub(super) levels: Option<LevelKey>,
}

pub(super) struct FadeTarget {
    pub(super) texture: Texture,
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
        let accumulate_group = present.bind(device, scene.color(), scene.color());
        let present_group = present.bind(device, &accumulation_view, &accumulation_view);
        Self {
            scene,
            accumulation,
            accumulation_view,
            accumulate_group,
            present_group,
            fade: None,
            oit: None,
            size,
            samples: 0,
            signature: None,
            transition: None,
            levels: None,
        }
    }

    pub(super) fn fade_group(&mut self, device: &Device, present: &Present) -> &BindGroup {
        let fade = self.fade.get_or_insert_with(|| {
            let texture = target::accumulation(device, self.size);
            let view = target::view(&texture);
            let group = present.bind(device, &self.accumulation_view, &view);
            FadeTarget { texture, group }
        });
        &fade.group
    }

    pub(super) fn snapshot(&mut self, encoder: &mut CommandEncoder) {
        let Some(fade) = self.fade.as_ref() else {
            return;
        };
        encoder.copy_texture_to_texture(
            TexelCopyTextureInfo {
                texture: &self.accumulation,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            TexelCopyTextureInfo {
                texture: &fade.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            wgpu::Extent3d {
                width: self.size[0],
                height: self.size[1],
                depth_or_array_layers: 1,
            },
        );
    }
}
