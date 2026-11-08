use super::*;

#[derive(Clone, Copy, PartialEq)]
pub(super) struct Levels {
    pub(super) volume: usize,
    pub(super) section: usize,
    pub(super) volume_fallback: usize,
    pub(super) section_fallback: usize,
    pub(super) volume_capacity: usize,
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
    pub(super) axis: grana_volume::Axis,
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
            axis: view.section.axis(),
            span: view.section.span(),
            volume: [view.volume_region[2], view.volume_region[3]],
            section: [view.section_region[2], view.section_region[3]],
            region: view.region,
            levels,
        }
    }
}

pub(super) struct FrameTarget {
    pub(super) scene: SceneTarget,
    pub(super) accumulation: TextureView,
    pub(super) accumulate_group: BindGroup,
    pub(super) present_group: BindGroup,
    pub(super) oit: Option<Oit>,
    pub(super) size: [u32; 2],
    pub(super) samples: u32,
    pub(super) signature: Option<u64>,
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
        let accumulate_group = present.bind(device, scene.color());
        let present_group = present.bind(device, &accumulation);
        Self {
            scene,
            accumulation,
            accumulate_group,
            present_group,
            oit: None,
            size,
            samples: 0,
            signature: None,
        }
    }
}
