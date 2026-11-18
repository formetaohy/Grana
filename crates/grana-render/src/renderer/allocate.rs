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
    if needed.surfaces && !needed.slice && target.normal.is_none() {
        target.normal = Some(crate::target::NormalTarget::new(device, size));
        target.ao = Some(crate::ao::Ao::new(device, layouts, gtao, size));
        target.surface = Some(crate::surface::Target::new(device, layouts, size));
    }
    if needed.volume && !needed.slice && target.volume.is_none() {
        let volume = crate::target::Volume::new(device, &layouts.volume_read, size);
        crate::clear::volume(
            encoder,
            volume.radiance(),
            volume.surface(),
            volume.normal(),
        );
        target.volume = Some(volume);
    }
    if let Some(required) = needed.layers {
        let plan = fragment::fit(size, needed.fragments, &device.limits(), required)?;
        if target.oit.as_ref().is_none_or(|oit| !oit.fits(plan)) {
            target.oit = Some(Oit::new(device, layouts, plan));
        }
    }
    Ok(())
}
