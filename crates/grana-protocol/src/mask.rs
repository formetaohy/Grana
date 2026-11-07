use grana_scene::{MaskSpec as SceneMaskSpec, Scene};
use grana_volume::ChannelInfo;

use crate::stage::Staged;
use crate::{Protocol, ProtocolError};

impl Protocol {
    pub fn mask_infos(&self, base: &[ChannelInfo]) -> Result<Vec<ChannelInfo>, ProtocolError> {
        self.masks()
            .iter()
            .map(|spec| {
                let source = base
                    .iter()
                    .find(|channel| channel.name() == spec.channel)
                    .ok_or_else(|| {
                        ProtocolError::Invalid(format!(
                            "the mask channel {} masks the missing channel {}",
                            spec.name, spec.channel
                        ))
                    })?;
                Ok(grana_scene::mask_info(&spec.name, spec.tint, source))
            })
            .collect()
    }

    pub fn mask_specs(
        &self,
        scene: &Scene,
        staged: &Staged,
    ) -> Result<Vec<SceneMaskSpec>, ProtocolError> {
        let channels = scene.dataset.channels();
        self.masks()
            .iter()
            .map(|spec| {
                let position = self
                    .sets()
                    .iter()
                    .position(|set| set.key == spec.set)
                    .ok_or_else(|| {
                        ProtocolError::Invalid(format!(
                            "the mask channel {} masks an object set that the protocol does not stage",
                            spec.name
                        ))
                    })?;
                let channel = channels
                    .iter()
                    .position(|channel| channel.name() == spec.channel)
                    .ok_or_else(|| {
                        ProtocolError::Invalid(format!(
                            "the mask channel {} masks a channel of the dataset that the protocol does not stage",
                            spec.name
                        ))
                    })?;
                Ok(SceneMaskSpec::new(
                    staged.set(position),
                    channel,
                    spec.name.clone(),
                    spec.tint,
                ))
            })
            .collect()
    }
}
