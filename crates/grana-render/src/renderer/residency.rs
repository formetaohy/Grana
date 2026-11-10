use super::*;

#[derive(Clone, Copy, PartialEq)]
pub(super) struct VolumeLevels {
    pub(super) ladder: Ladder,
    pub(super) capacity: [usize; BAND],
}

#[derive(Clone, Copy, PartialEq)]
pub(super) struct SectionLevels {
    pub(super) level: usize,
    pub(super) fallback: usize,
    pub(super) capacity: usize,
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum PaneLevels {
    Volume(VolumeLevels),
    Section(SectionLevels),
}

#[derive(Clone, Copy, PartialEq)]
pub(super) struct Levels {
    pub(super) panes: [Option<PaneLevels>; PANE_COUNT],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PaneRegion {
    pub(super) kind: PaneKind,
    pub(super) region: [u32; 4],
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Regions {
    pub(super) panes: [Option<PaneRegion>; PANE_COUNT],
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum PaneResidency {
    Volume {
        eye: [f32; 3],
        focus: [f32; 3],
        field_of_view: f32,
        size: [u32; 2],
        levels: VolumeLevels,
    },
    Section {
        center: [f32; 3],
        normal: [f32; 3],
        right: [f32; 3],
        thickness: f32,
        projection: u8,
        span: f32,
        size: [u32; 2],
        levels: SectionLevels,
    },
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
    pub(super) panes: [Option<PaneResidency>; PANE_COUNT],
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

impl Residency {
    pub(super) fn of(
        epoch: u64,
        content: Content,
        mode: Mode,
        time: u32,
        frames: &[Option<PaneFrame<'_>>; PANE_COUNT],
        region: Region,
        levels: Levels,
    ) -> Self {
        let panes = std::array::from_fn(|index| match (frames[index], levels.panes[index]) {
            (Some(PaneFrame::Volume { camera, region, .. }), Some(PaneLevels::Volume(levels))) => {
                Some(PaneResidency::Volume {
                    eye: camera.eye().to_array(),
                    focus: camera.pivot().to_array(),
                    field_of_view: camera.fov_y(),
                    size: [region[2], region[3]],
                    levels,
                })
            }
            (Some(PaneFrame::Section { section, region }), Some(PaneLevels::Section(levels))) => {
                Some(PaneResidency::Section {
                    center: section.center().to_array(),
                    normal: section.normal().to_array(),
                    right: section.right().to_array(),
                    thickness: section.thickness(),
                    projection: section.projection().index() as u8,
                    span: section.span(),
                    size: [region[2], region[3]],
                    levels,
                })
            }
            _ => None,
        });
        Self {
            epoch,
            content: content.content,
            style: content.style,
            prefetch: mode.prefetch,
            assured: mode.assured,
            time,
            region,
            panes,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct PaneLevelKey {
    pub(super) volume: Option<(usize, usize)>,
    pub(super) section: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct LevelKey {
    pub(super) panes: [Option<PaneLevelKey>; PANE_COUNT],
}

impl LevelKey {
    pub(super) fn of(levels: Levels) -> Self {
        Self {
            panes: std::array::from_fn(|index| {
                levels.panes[index].map(|levels| match levels {
                    PaneLevels::Volume(volume) => PaneLevelKey {
                        volume: Some((volume.ladder.base, volume.ladder.count)),
                        section: None,
                    },
                    PaneLevels::Section(section) => PaneLevelKey {
                        volume: None,
                        section: Some(section.level),
                    },
                })
            }),
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
    pub(super) oit: [Option<Oit>; PANE_COUNT],
    pub(super) ao: [Option<crate::ao::Ao>; PANE_COUNT],
    pub(super) size: [u32; 2],
    pub(super) samples: u32,
    pub(super) signature: Option<u64>,
    pub(super) transition: Option<u64>,
    pub(super) levels: Option<LevelKey>,
    pub(super) shot: Option<u64>,
    pub(super) panes: FramePanes,
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
            oit: std::array::from_fn(|_| None),
            ao: std::array::from_fn(|_| None),
            size,
            samples: 0,
            signature: None,
            transition: None,
            levels: None,
            shot: None,
            panes: FramePanes::default(),
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
