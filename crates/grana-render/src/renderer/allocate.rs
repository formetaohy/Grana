use super::*;

use crate::fragment;

pub(super) struct Needed {
    pub(super) surfaces: bool,
    pub(super) slice: bool,
    pub(super) volume: bool,
    pub(super) layers: Option<u64>,
    pub(super) fragments: u64,
}

pub(super) fn targets(
    device: &Device,
    layouts: &Layouts,
    gtao: &crate::ao::AoPipeline,
    encoder: &mut CommandEncoder,
    target: &mut FrameTarget,
    needed: Needed,
) -> Result<(), String> {
    let size = target.size;
    if needed.surfaces && !needed.slice {
        if target.normal.is_none() {
            target.normal = Some(crate::target::NormalTarget::new(device, size));
            target.ao = Some(crate::ao::Ao::new(device, layouts, gtao, size));
            target.surface = Some(crate::surface::Target::new(device, layouts, size));
        }
    } else if !needed.surfaces && target.normal.is_some() {
        target.normal = None;
        target.ao = None;
        target.surface = None;
        target.objects = None;
        target.geometry = None;
    }
    if needed.volume && !needed.slice {
        if target.volume.is_none() {
            let volume = crate::target::Volume::new(device, &layouts.volume_read, size);
            crate::clear::volume(
                encoder,
                volume.radiance(),
                volume.surface(),
                volume.normal(),
            );
            target.volume = Some(volume);
        }
    } else if !needed.volume && target.volume.is_some() {
        target.volume = None;
        target.geometry = None;
    }
    if let Some(layers) = needed.layers {
        let plan = fragment::fit(size, needed.fragments, &device.limits(), layers)?;
        if target.oit.as_ref().is_none_or(|oit| !oit.fits(plan)) {
            target.oit = Some(Oit::new(device, layouts, plan));
        }
    } else {
        target.oit = None;
    }
    Ok(())
}
