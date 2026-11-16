use super::*;

pub(super) struct Needed {
    pub(super) surfaces: bool,
    pub(super) slice: bool,
    pub(super) volume: bool,
    pub(super) translucent: bool,
    pub(super) budget: u64,
}

pub(super) fn targets(
    device: &Device,
    layouts: &Layouts,
    gtao: &crate::ao::AoPipeline,
    encoder: &mut CommandEncoder,
    target: &mut FrameTarget,
    needed: Needed,
) {
    let size = target.size;
    if needed.surfaces && !needed.slice && target.gbuffer.is_none() {
        let gbuffer =
            crate::target::Gbuffer::new(device, &layouts.gbuffer, target.scene.depth(), size);
        target.ao = Some(crate::ao::Ao::new(device, layouts, gtao, size));
        target.surface = Some(crate::surface::Target::new(device, layouts, size));
        target.gbuffer = Some(gbuffer);
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
    let pixels = u64::from(size[0]) * u64::from(size[1]);
    if needed.translucent
        && target
            .oit
            .as_ref()
            .is_none_or(|oit| !oit.fits(size, pixels))
    {
        target.oit = Some(Oit::new(
            device,
            layouts,
            size,
            pixels,
            needed.budget / FRAGMENT_BUDGET_DIVISOR,
        ));
    }
}
