use grana_optics::{Kernel, MeasuredPsf, Psf, PsfParams};
use grana_scene::{DerivedSpec as SceneDerivedSpec, Scene, mask_info, mask_intensity};
use grana_volume::{ChannelInfo, Intensity, Spacing};

use crate::stage::Staged;
use crate::{DerivedSpec, Protocol, ProtocolError, RestoreSpec as ProtocolRestoreSpec};

impl Protocol {
    pub fn derived_infos(&self, base: &[ChannelInfo]) -> Result<Vec<ChannelInfo>, ProtocolError> {
        let mut channels = base.to_vec();
        let mut infos = Vec::with_capacity(self.derived().len());
        for spec in self.derived() {
            let source = position(&channels, spec.channel()).ok_or_else(|| {
                ProtocolError::Invalid(format!(
                    "the derived channel {} derives from the missing channel {}",
                    spec.name(),
                    spec.channel()
                ))
            })?;
            let info = match spec {
                DerivedSpec::Mask(mask) => mask_info(&mask.name, mask.tint, &channels[source]),
                DerivedSpec::Restore(restore) => restore_info(restore),
                DerivedSpec::Smooth(smooth) => {
                    operator_info(&smooth.name, smooth.tint, smooth.intensity)
                }
                DerivedSpec::Background(background) => {
                    operator_info(&background.name, background.tint, background.intensity)
                }
                DerivedSpec::Ratio(ratio) => {
                    if position(&channels, &ratio.denominator).is_none() {
                        return Err(ProtocolError::Invalid(format!(
                            "the ratio channel {} divides the missing channel {}",
                            ratio.name, ratio.denominator
                        )));
                    }
                    operator_info(&ratio.name, ratio.tint, ratio.intensity)
                }
                DerivedSpec::Bleach(bleach) => {
                    operator_info(&bleach.name, bleach.tint, bleach.intensity)
                }
            };
            channels.push(info.clone());
            infos.push(info);
        }
        Ok(infos)
    }

    pub fn derived_specs(
        &self,
        scene: &Scene,
        staged: &Staged,
    ) -> Result<Vec<SceneDerivedSpec>, ProtocolError> {
        let channels = scene.dataset.channels();
        let base = channels
            .len()
            .checked_sub(self.derived().len())
            .ok_or_else(|| {
                ProtocolError::Invalid(
                    "the scene holds fewer channels than the protocol derives".to_string(),
                )
            })?;
        self.derived()
            .iter()
            .enumerate()
            .map(|(index, spec)| {
                let source = position(channels, spec.channel()).ok_or_else(|| {
                    ProtocolError::Invalid(format!(
                        "the derived channel {} derives from a channel of the dataset that the protocol does not stage",
                        spec.name()
                    ))
                })?;
                if source >= base + index {
                    return Err(ProtocolError::Invalid(format!(
                        "the derived channel {} derives from a channel that follows it",
                        spec.name()
                    )));
                }
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
                        SceneDerivedSpec::masked(
                            staged.set(position),
                            source,
                            mask.name.clone(),
                            mask.tint,
                            mask_intensity(&channels[source]),
                        )
                    }
                    DerivedSpec::Restore(restore) => SceneDerivedSpec::restored(
                        source,
                        restore.name.clone(),
                        restore.tint,
                        Intensity::new(restore.intensity[0], restore.intensity[1]),
                        psf(restore)?,
                        restore.iterations,
                    ),
                    DerivedSpec::Smooth(smooth) => SceneDerivedSpec::smoothed(
                        source,
                        smooth.name.clone(),
                        smooth.tint,
                        Intensity::new(smooth.intensity[0], smooth.intensity[1]),
                        smooth.sigma,
                    ),
                    DerivedSpec::Background(background) => {
                        SceneDerivedSpec::background_subtracted(
                            source,
                            background.name.clone(),
                            background.tint,
                            Intensity::new(background.intensity[0], background.intensity[1]),
                            background.radius,
                        )
                    }
                    DerivedSpec::Ratio(ratio) => {
                        let denominator =
                            position(channels, &ratio.denominator).ok_or_else(|| {
                                ProtocolError::Invalid(format!(
                                    "the ratio channel {} divides a channel of the dataset that the protocol does not stage",
                                    ratio.name
                                ))
                            })?;
                        if denominator >= base + index || denominator == source {
                            return Err(ProtocolError::Invalid(format!(
                                "the ratio channel {} divides a channel that follows it",
                                ratio.name
                            )));
                        }
                        SceneDerivedSpec::ratio(
                            source,
                            denominator,
                            ratio.name.clone(),
                            ratio.tint,
                            Intensity::new(ratio.intensity[0], ratio.intensity[1]),
                        )
                    }
                    DerivedSpec::Bleach(bleach) => SceneDerivedSpec::bleached(
                        source,
                        bleach.name.clone(),
                        bleach.tint,
                        Intensity::new(bleach.intensity[0], bleach.intensity[1]),
                    ),
                })
            })
            .collect()
    }
}

fn position(channels: &[ChannelInfo], name: &str) -> Option<usize> {
    channels.iter().position(|channel| channel.name() == name)
}

fn operator_info(name: &str, tint: [f32; 3], intensity: [f32; 2]) -> ChannelInfo {
    let intensity = Intensity::new(intensity[0], intensity[1]);
    ChannelInfo::new(
        name,
        tint,
        intensity,
        intensity,
        grana_volume::VoxelFormat::F32,
    )
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
