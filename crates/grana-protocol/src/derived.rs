use grana_optics::{Kernel, MeasuredPsf, Psf, PsfParams};
use grana_scene::{DerivedSpec as SceneDerivedSpec, MaskSpec as SceneMaskSpec, RestoreSpec, Scene};
use grana_volume::{ChannelInfo, Intensity, Spacing};

use crate::stage::Staged;
use crate::{DerivedSpec, Protocol, ProtocolError, RestoreSpec as ProtocolRestoreSpec};

impl Protocol {
    pub fn derived_infos(&self, base: &[ChannelInfo]) -> Result<Vec<ChannelInfo>, ProtocolError> {
        self.derived()
            .iter()
            .map(|spec| {
                let source = base
                    .iter()
                    .find(|channel| channel.name() == spec.channel())
                    .ok_or_else(|| {
                        ProtocolError::Invalid(format!(
                            "the derived channel {} derives from the missing channel {}",
                            spec.name(),
                            spec.channel()
                        ))
                    })?;
                Ok(match spec {
                    DerivedSpec::Mask(mask) => {
                        grana_scene::mask_info(&mask.name, mask.tint, source)
                    }
                    DerivedSpec::Restore(restore) => restore_info(restore),
                })
            })
            .collect()
    }

    pub fn derived_specs(
        &self,
        scene: &Scene,
        staged: &Staged,
    ) -> Result<Vec<SceneDerivedSpec>, ProtocolError> {
        let channels = scene.dataset.channels();
        self.derived()
            .iter()
            .map(|spec| {
                let channel = channels
                    .iter()
                    .position(|channel| channel.name() == spec.channel())
                    .ok_or_else(|| {
                        ProtocolError::Invalid(format!(
                            "the derived channel {} derives from a channel of the dataset that the protocol does not stage",
                            spec.name()
                        ))
                    })?;
                Ok(match spec {
                    DerivedSpec::Mask(mask) => {
                        let position = self
                            .sets()
                            .iter()
                            .position(|set| set.key == mask.set)
                            .ok_or_else(|| {
                                ProtocolError::Invalid(format!(
                                    "the mask channel {} masks an object set that the protocol does not stage",
                                    mask.name
                                ))
                            })?;
                        SceneDerivedSpec::Mask(SceneMaskSpec::new(
                            staged.set(position),
                            channel,
                            mask.name.clone(),
                            mask.tint,
                        ))
                    }
                    DerivedSpec::Restore(restore) => SceneDerivedSpec::Restore(RestoreSpec::new(
                        channel,
                        restore.name.clone(),
                        restore.tint,
                        Intensity::new(restore.intensity[0], restore.intensity[1]),
                        psf(restore)?,
                        restore.iterations,
                    )),
                })
            })
            .collect()
    }
}

fn restore_info(spec: &ProtocolRestoreSpec) -> ChannelInfo {
    let intensity = Intensity::new(spec.intensity[0], spec.intensity[1]);
    ChannelInfo::new(
        spec.name.clone(),
        spec.tint,
        intensity,
        intensity,
        grana_volume::VoxelFormat::F32,
    )
}

fn psf(spec: &ProtocolRestoreSpec) -> Result<Psf, ProtocolError> {
    let Some(measured) = &spec.measured else {
        return Ok(Psf::theoretical(PsfParams::new(
            spec.numerical_aperture,
            spec.wavelength,
            spec.immersion,
            spec.sample,
            spec.depth,
        )));
    };
    let invalid = || {
        ProtocolError::Invalid(format!(
            "the measured point spread function of the restored channel {} is malformed",
            spec.name
        ))
    };
    let dims = measured.dims;
    let voxels = (dims[0] as usize) * (dims[1] as usize) * (dims[2] as usize);
    let layers = measured.values.len() / voxels.max(1);
    if voxels == 0
        || measured.values.len() % voxels != 0
        || !(1..=16).contains(&layers)
        || dims
            .iter()
            .any(|extent| *extent % 2 == 0 || !(1..=63).contains(extent))
        || measured
            .values
            .iter()
            .any(|value| !value.is_finite() || *value < 0.0)
        || measured
            .values
            .chunks(voxels)
            .any(|values| values.iter().sum::<f32>() <= 0.0)
    {
        return Err(invalid());
    }
    let kernels = measured
        .values
        .chunks(voxels)
        .map(|values| Kernel::from_values(dims, values.to_vec()))
        .collect::<Vec<_>>();
    if kernels.is_empty()
        || !measured
            .spacing
            .iter()
            .all(|value| value.is_finite() && *value > 0.0)
    {
        return Err(invalid());
    }
    Ok(Psf::measured(MeasuredPsf::new(
        Spacing::new(
            measured.spacing[0],
            measured.spacing[1],
            measured.spacing[2],
        ),
        kernels,
    )))
}
