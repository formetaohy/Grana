use grana_optics::PsfParams;
use grana_scene::{DerivedSpec as SceneDerivedSpec, MaskSpec as SceneMaskSpec, RestoreSpec, Scene};
use grana_volume::{ChannelInfo, Intensity};

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
                        PsfParams::new(
                            restore.numerical_aperture,
                            restore.wavelength,
                            restore.immersion,
                            restore.sample,
                            restore.depth,
                        ),
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
