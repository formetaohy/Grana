use std::ops::Range;

use rayon::prelude::*;

use crate::{
    BRICK_APRON, BRICK_SIDE, BrickData, CELL_AXIS, CELL_COUNT, CELL_SIDE, CHANNEL_LIMIT,
    ChannelSet, PADDED_SIDE, Pyramid,
};

pub const PACK: usize = 4;

pub const PACK_LIMIT: usize = CHANNEL_LIMIT / PACK;

const PADDED: usize = PADDED_SIDE as usize;

const APRON: usize = BRICK_APRON as usize;

const CELL: usize = CELL_SIDE as usize;

const INNER: Range<usize> = APRON..APRON + BRICK_SIDE as usize;

pub fn pack_count(channels: usize) -> usize {
    channels.div_ceil(PACK)
}

pub fn components(channels: usize, pack: usize) -> usize {
    assert!(pack * PACK < channels, "a pack holds at least one channel");
    (channels - pack * PACK).min(PACK)
}

pub fn texels(components: usize) -> usize {
    match components {
        1 => 1,
        2 => 2,
        3 | 4 => 4,
        _ => unreachable!("a channel pack holds one to four channels"),
    }
}

fn covers(channels: usize, scope: ChannelSet, pack: usize) -> bool {
    (pack * PACK..(pack + 1) * PACK).all(|channel| channel >= channels || scope.contains(channel))
}

pub struct Pack {
    components: usize,
    maximum: f32,
    cells: Vec<f32>,
    rows: Vec<u16>,
}

impl Pack {
    pub fn texel_bytes(&self) -> u32 {
        (texels(self.components) * size_of::<u16>()) as u32
    }

    pub fn maximum(&self) -> f32 {
        self.maximum
    }

    pub fn cells(&self) -> &[f32] {
        &self.cells
    }

    pub fn rows(&self) -> &[u16] {
        &self.rows
    }

    fn new(components: usize) -> Self {
        Self {
            components,
            maximum: 0.0,
            cells: vec![0.0; CELL_COUNT],
            rows: vec![0; PADDED * PADDED * PADDED * texels(components)],
        }
    }

    fn stage(&mut self, pyramid: &Pyramid, data: &BrickData, pack: usize) {
        let sources = (0..self.components)
            .map(|component| {
                data.channels()[pack * PACK + component]
                    .units(pyramid.channels()[pack * PACK + component].intensity())
            })
            .collect::<Vec<_>>();
        let stride = texels(self.components);
        let row = PADDED * stride;
        let (maximum, cells) = self
            .rows
            .par_chunks_mut(row * PADDED)
            .enumerate()
            .map(|(slice, chunk)| {
                let mut cells = [0.0f32; CELL_COUNT];
                let mut maximum = 0.0f32;
                let range = slice * PADDED * PADDED..(slice + 1) * PADDED * PADDED;
                let plane = INNER
                    .contains(&slice)
                    .then(|| ((slice - APRON) / CELL) * CELL_AXIS * CELL_AXIS);
                for (component, source) in sources.iter().enumerate() {
                    let mut x = 0;
                    let mut y = 0;
                    source.visit(range.clone(), |fixed| {
                        if fixed > maximum {
                            maximum = fixed;
                        }
                        if let Some(plane) = plane
                            && INNER.contains(&x)
                            && INNER.contains(&y)
                        {
                            let cell =
                                (x - APRON) / CELL + ((y - APRON) / CELL) * CELL_AXIS + plane;
                            if fixed > cells[cell] {
                                cells[cell] = fixed;
                            }
                        }
                        chunk[y * row + x * stride + component] = fixed.round() as u16;
                        x += 1;
                        if x == PADDED {
                            x = 0;
                            y += 1;
                        }
                    });
                }
                (maximum, cells)
            })
            .reduce(
                || (0.0f32, [0.0f32; CELL_COUNT]),
                |left, right| {
                    let mut cells = left.1;
                    for (cell, other) in cells.iter_mut().zip(right.1.iter()) {
                        *cell = cell.max(*other);
                    }
                    (left.0.max(right.0), cells)
                },
            );
        self.maximum = maximum;
        self.cells.copy_from_slice(&cells);
    }
}

pub struct PackedBrick {
    channels: usize,
    packs: Vec<Option<Pack>>,
}

impl PackedBrick {
    pub fn of(data: &BrickData, pyramid: &Pyramid, scope: ChannelSet) -> Self {
        let channels = pyramid.channel_count();
        assert_eq!(
            data.channels().len(),
            channels,
            "a brick carries one value per dataset channel"
        );
        let packs = (0..pack_count(channels))
            .map(|pack| {
                covers(channels, scope, pack).then(|| {
                    let mut held = Pack::new(components(channels, pack));
                    held.stage(pyramid, data, pack);
                    held
                })
            })
            .collect::<Vec<_>>();
        Self { channels, packs }
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    pub fn pack(&self, pack: usize) -> Option<&Pack> {
        self.packs.get(pack).and_then(Option::as_ref)
    }

    pub fn packs(&self) -> impl Iterator<Item = (usize, &Pack)> {
        self.packs
            .iter()
            .enumerate()
            .filter_map(|(pack, held)| held.as_ref().map(|held| (pack, held)))
    }
}
