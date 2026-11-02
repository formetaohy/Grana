pub(crate) fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    data.get(at..at + 4)
        .map(|slice| u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

pub(crate) fn i32_at(data: &[u8], at: usize) -> Option<i32> {
    u32_at(data, at).map(|value| value as i32)
}

pub(crate) fn u64_at(data: &[u8], at: usize) -> Option<u64> {
    data.get(at..at + 8).map(|slice| {
        u64::from_le_bytes([
            slice[0], slice[1], slice[2], slice[3], slice[4], slice[5], slice[6], slice[7],
        ])
    })
}

pub(crate) fn i64_at(data: &[u8], at: usize) -> Option<i64> {
    u64_at(data, at).map(|value| value as i64)
}
