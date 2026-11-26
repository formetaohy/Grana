use grana_volume::{ChannelInfo, Pyramid, VoxelSemantics};

pub(crate) fn categorical(channel: &ChannelInfo) -> bool {
    channel.color_table().is_some() || matches!(channel.semantics(), VoxelSemantics::Label)
}

pub(crate) fn samples_per_voxel(pyramid: &Pyramid) -> f32 {
    if pyramid.channels().iter().any(categorical) {
        2.0
    } else {
        1.0
    }
}
