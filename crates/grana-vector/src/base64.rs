const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub(crate) fn encode(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let first = u32::from(chunk[0]);
        let second = chunk.get(1).map_or(0, |value| u32::from(*value));
        let third = chunk.get(2).map_or(0, |value| u32::from(*value));
        let word = (first << 16) | (second << 8) | third;
        text.push(ALPHABET[(word >> 18) as usize & 63] as char);
        text.push(ALPHABET[(word >> 12) as usize & 63] as char);
        text.push(if chunk.len() > 1 {
            ALPHABET[(word >> 6) as usize & 63] as char
        } else {
            '='
        });
        text.push(if chunk.len() > 2 {
            ALPHABET[word as usize & 63] as char
        } else {
            '='
        });
    }
    text
}
