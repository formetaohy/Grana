use std::ops::Range;

use super::bits::Bits;

const WORD: usize = 64;
const REACH: usize = 3;
const SHIFT: usize = 1;

pub(super) struct Band {
    active: Bits,
    needed: Bits,
}

impl Band {
    pub(super) fn new(mask: &[bool], dims: [usize; 3], inside: &[Range<usize>; 3]) -> Self {
        assert!(
            mask.len() == dims[0] * dims[1] * dims[2],
            "a surface mask covers its array"
        );
        let spans = dims.map(|value| value + 2 * SHIFT);
        let mut solid = Bits::new(spans);
        for z in inside[2].clone() {
            for y in inside[1].clone() {
                let base = dims[0] * (y + dims[1] * z);
                for x in inside[0].clone() {
                    if mask[base + x] {
                        solid.set(x + SHIFT, y + SHIFT, z + SHIFT);
                    }
                }
            }
        }
        let rows = solid.rows();
        let mut boundary = Bits::new(spans);
        for z in 0..spans[2] {
            for y in 0..spans[1] {
                for word in 0..rows {
                    let line = solid.line(y, z);
                    let value = line[word];
                    let next = line.get(word + 1).copied().unwrap_or(0) & 1;
                    let diff = value ^ ((value >> 1) | (next << (WORD - 1)));
                    boundary.set_word(y, z, word, boundary.word(y, z, word) | diff | (diff << 1));
                    if diff >> (WORD - 1) != 0 && word + 1 < rows {
                        boundary.set_word(y, z, word + 1, boundary.word(y, z, word + 1) | 1);
                    }
                }
            }
        }
        for z in 0..spans[2] {
            for y in 0..spans[1] {
                if y + 1 < spans[1] {
                    for word in 0..rows {
                        let diff = solid.word(y, z, word) ^ solid.word(y + 1, z, word);
                        boundary.set_word(y, z, word, boundary.word(y, z, word) | diff);
                        boundary.set_word(y + 1, z, word, boundary.word(y + 1, z, word) | diff);
                    }
                }
                if z + 1 < spans[2] {
                    for word in 0..rows {
                        let diff = solid.word(y, z, word) ^ solid.word(y, z + 1, word);
                        boundary.set_word(y, z, word, boundary.word(y, z, word) | diff);
                        boundary.set_word(y, z + 1, word, boundary.word(y, z + 1, word) | diff);
                    }
                }
            }
        }
        let mut active = Bits::new(spans);
        for z in 0..spans[2] {
            for y in 0..spans[1] {
                for word in 0..rows {
                    let mut corners = boundary.line(y, z)[word];
                    if y + 1 < spans[1] {
                        corners |= boundary.line(y + 1, z)[word];
                        if z + 1 < spans[2] {
                            corners |= boundary.line(y + 1, z + 1)[word];
                        }
                    }
                    if z + 1 < spans[2] {
                        corners |= boundary.line(y, z + 1)[word];
                    }
                    let next = if word + 1 < rows {
                        let mut lower = boundary.line(y, z)[word + 1];
                        if y + 1 < spans[1] {
                            lower |= boundary.line(y + 1, z)[word + 1];
                            if z + 1 < spans[2] {
                                lower |= boundary.line(y + 1, z + 1)[word + 1];
                            }
                        }
                        if z + 1 < spans[2] {
                            lower |= boundary.line(y, z + 1)[word + 1];
                        }
                        lower & 1
                    } else {
                        0
                    };
                    active.set_word(y, z, word, corners | (corners >> 1) | (next << (WORD - 1)));
                }
            }
        }
        let needed = dilate(active.words(), spans, rows, REACH);
        Self { active, needed }
    }

    pub(super) fn active_word(&self, y: usize, z: usize, word: usize) -> u64 {
        self.active.word(y, z, word)
    }

    pub(super) fn needed_word(&self, y: usize, z: usize, word: usize) -> u64 {
        self.needed.word(y, z, word)
    }
}

fn dilate(words: &[u64], spans: [usize; 3], rows: usize, reach: usize) -> Bits {
    let mut current = words.to_vec();
    let mut scratch = vec![0u64; words.len()];
    for z in 0..spans[2] {
        for y in 0..spans[1] {
            let base = rows * (y + spans[1] * z);
            let source = &current[base..base + rows];
            let target = &mut scratch[base..base + rows];
            for word in 0..rows {
                let previous = if word > 0 { source[word - 1] } else { 0 };
                let next = source.get(word + 1).copied().unwrap_or(0);
                let mut value = source[word];
                for step in 1..=reach {
                    value |= (source[word] >> step)
                        | (source[word] << step)
                        | (previous >> (WORD - step))
                        | (next << (WORD - step));
                }
                target[word] = value;
            }
        }
    }
    std::mem::swap(&mut current, &mut scratch);
    for z in 0..spans[2] {
        for y in 0..spans[1] {
            let base = rows * (y + spans[1] * z);
            let target = &mut scratch[base..base + rows];
            for word in 0..rows {
                let mut value = 0u64;
                for step in 0..=reach {
                    if let Some(lower) = y.checked_sub(step) {
                        value |= current[rows * (lower + spans[1] * z) + word];
                    }
                    if y + step < spans[1] {
                        value |= current[rows * (y + step + spans[1] * z) + word];
                    }
                }
                target[word] = value;
            }
        }
    }
    std::mem::swap(&mut current, &mut scratch);
    for z in 0..spans[2] {
        for y in 0..spans[1] {
            let base = rows * (y + spans[1] * z);
            let target = &mut scratch[base..base + rows];
            for word in 0..rows {
                let mut value = 0u64;
                for step in 0..=reach {
                    if let Some(lower) = z.checked_sub(step) {
                        value |= current[rows * (y + spans[1] * lower) + word];
                    }
                    if z + step < spans[2] {
                        value |= current[rows * (y + spans[1] * (z + step)) + word];
                    }
                }
                target[word] = value;
            }
        }
    }
    std::mem::swap(&mut current, &mut scratch);
    Bits::from_words(spans, current)
}
