const OIT_DEPTH_SHIFT: u32 = OIT_SLOT_BITS;
const OIT_DEPTH_LOSS: u32 = 32u - OIT_DEPTH_BITS;
const OIT_DEPTH_MASK: u32 = ((1u << OIT_DEPTH_BITS) - 1u) << OIT_DEPTH_SHIFT;
const OIT_SLOT_MASK: u32 = (1u << OIT_SLOT_BITS) - 1u;
const OIT_EPOCH_SHIFT: u32 = OIT_SLOT_BITS + OIT_DEPTH_BITS;
const OIT_COUNT_MASK: u32 = (1u << OIT_EPOCH_SHIFT) - 1u;

fn oit_key(epoch: u32, depth: f32, slot: u32) -> u32 {
    let inverted = (~bitcast<u32>(clamp(depth, 0.0, 1.0)) >> OIT_DEPTH_LOSS) << OIT_DEPTH_SHIFT;
    return (epoch << OIT_EPOCH_SHIFT) | (inverted & OIT_DEPTH_MASK) | (slot & OIT_SLOT_MASK);
}

fn oit_key_epoch(key: u32) -> u32 {
    return key >> OIT_EPOCH_SHIFT;
}

fn oit_key_slot(key: u32) -> u32 {
    return key & OIT_SLOT_MASK;
}

fn oit_key_depth(key: u32) -> f32 {
    let inverted = ((key & OIT_DEPTH_MASK) >> OIT_DEPTH_SHIFT) << OIT_DEPTH_LOSS;
    return bitcast<f32>(~inverted);
}

fn oit_count_epoch(held: u32) -> u32 {
    return held >> OIT_EPOCH_SHIFT;
}

fn oit_count_layers(held: u32, layers: u32, epoch: u32) -> u32 {
    if (oit_count_epoch(held) != epoch) {
        return 0u;
    }
    return min(held & OIT_COUNT_MASK, layers);
}
