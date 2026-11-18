use std::ops::Range;
use std::sync::atomic::AtomicBool;

use super::SurfaceBuilder;
use crate::detection::DetectionImage;

pub(super) use crate::detection::Slab as Detection;

pub(super) struct DetectionSource<'a> {
    plan: &'a SurfaceBuilder,
    cover: &'a [Range<u32>; 3],
    workers: usize,
    cancel: &'a AtomicBool,
    image: &'a DetectionImage,
}

impl<'a> DetectionSource<'a> {
    pub(super) fn new(
        plan: &'a SurfaceBuilder,
        cover: &'a [Range<u32>; 3],
        workers: usize,
        cancel: &'a AtomicBool,
        image: &'a DetectionImage,
    ) -> Self {
        Self {
            plan,
            cover,
            workers,
            cancel,
            image,
        }
    }

    pub(super) fn plan(&self) -> &'a SurfaceBuilder {
        self.plan
    }

    pub(super) fn cover(&self) -> &'a [Range<u32>; 3] {
        self.cover
    }

    pub(super) fn workers(&self) -> usize {
        self.workers
    }

    pub(super) fn cancel(&self) -> &'a AtomicBool {
        self.cancel
    }

    pub(super) fn slab(&self, z: Range<u32>) -> Result<Option<Detection<'_>>, String> {
        self.image.slab(z)
    }
}
