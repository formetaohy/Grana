use std::ops::Range;

use burn::prelude::*;

pub(crate) type Source<'a> = &'a dyn Fn(Range<usize>) -> Tensor<5>;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Span {
    start: usize,
    extent: usize,
    side: usize,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Stripe {
    side: usize,
    count: usize,
}

impl Span {
    pub(crate) fn new(start: usize, extent: usize, side: usize) -> Self {
        assert!(
            side > 0 && extent > 0 && start + extent <= side,
            "a span covers a non empty block inside its side"
        );
        Self {
            start,
            extent,
            side,
        }
    }

    pub(crate) fn start(self) -> usize {
        self.start
    }

    pub(crate) fn extent(self) -> usize {
        self.extent
    }

    pub(crate) fn front(self, halo: usize) -> usize {
        halo.min(self.start)
    }

    pub(crate) fn back(self, halo: usize) -> usize {
        halo.min(self.side - self.start - self.extent)
    }

    pub(crate) fn context(self, halo: usize) -> Range<usize> {
        self.start - self.front(halo)..self.start + self.extent + self.back(halo)
    }

    pub(crate) fn crop(self, tensor: Tensor<5>, halo: usize) -> Tensor<5> {
        tensor.narrow(4, self.front(halo), self.extent)
    }
}

impl Stripe {
    pub(crate) fn new(side: usize, count: usize) -> Self {
        assert!(
            count > 0 && side > 0 && side.is_multiple_of(count),
            "a stripe divides its side into equal spans"
        );
        Self { side, count }
    }

    pub(crate) fn side(self) -> usize {
        self.side
    }

    pub(crate) fn spans(self) -> Vec<Span> {
        let extent = self.side / self.count;
        (0..self.count)
            .map(|index| Span::new(index * extent, extent, self.side))
            .collect()
    }
}

pub(crate) fn parent(target: &Range<usize>, side: usize) -> Range<usize> {
    let start = (target.start / 2).saturating_sub(1);
    let end = ((target.end + 1).div_ceil(2) + 1).min(side);
    start..end
}

pub(crate) fn raised(target: &Range<usize>, parent: &Range<usize>) -> usize {
    target.start - 2 * parent.start
}
