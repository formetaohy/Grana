use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use grana_geometry::{Geometry, PackedMesh, simplify};

pub(super) const LIMIT: usize = 5;

#[derive(Debug)]
pub(super) struct Details {
    levels: Mutex<Vec<Option<Geometry<PackedMesh>>>>,
    requested: AtomicUsize,
    hint: AtomicUsize,
    ceiling: AtomicUsize,
    building: AtomicBool,
}

impl Details {
    pub(super) fn new() -> Self {
        Self {
            levels: Mutex::new(vec![None; LIMIT + 1]),
            requested: AtomicUsize::new(0),
            hint: AtomicUsize::new(0),
            ceiling: AtomicUsize::new(LIMIT),
            building: AtomicBool::new(false),
        }
    }

    pub(super) fn level(
        &self,
        level: usize,
        base: &Geometry<PackedMesh>,
    ) -> Result<(Arc<PackedMesh>, usize, bool), grana_geometry::Error> {
        let level = level.min(self.ceiling.load(Ordering::Relaxed)).min(LIMIT);
        if level == 0 {
            return Ok((base.load()?, 0, false));
        }
        let available = {
            let levels = self.levels();
            (1..=level)
                .rev()
                .find(|index| levels[*index].is_some())
                .unwrap_or(0)
        };
        let mesh = if available == 0 {
            base.load()?
        } else {
            let levels = self.levels();
            levels[available]
                .as_ref()
                .expect("the available detail level is built")
                .load()?
        };
        let start = available < level && self.request(level);
        Ok((mesh, available, start))
    }

    pub(super) fn select(&self, base: usize, target: usize) -> usize {
        let target = target.max(1);
        let ceiling = self.ceiling.load(Ordering::Relaxed);
        if base <= target.saturating_mul(2) {
            self.hint.store(0, Ordering::Relaxed);
            return 0;
        }
        let current = self.hint.load(Ordering::Relaxed).min(ceiling);
        if current > 0 {
            let estimate = (base >> current).max(1);
            if estimate <= target.saturating_mul(4) && estimate.saturating_mul(4) >= target {
                return current;
            }
        }
        let mut level = 0;
        let mut triangles = base;
        while triangles > target.saturating_mul(2) && level < ceiling {
            triangles /= 2;
            level += 1;
        }
        self.hint.store(level, Ordering::Relaxed);
        level
    }

    pub(super) fn bytes(&self) -> u64 {
        let levels = self.levels();
        levels
            .iter()
            .flatten()
            .map(|geometry| geometry.bytes())
            .sum()
    }

    pub(super) fn cancel(&self) {
        self.building.store(false, Ordering::Release);
    }

    fn halt(&self, built: usize) {
        self.ceiling.store(built, Ordering::Relaxed);
        self.requested.store(built, Ordering::Relaxed);
        self.building.store(false, Ordering::Release);
    }

    fn request(&self, level: usize) -> bool {
        if self.requested.fetch_max(level, Ordering::Relaxed) >= level {
            return false;
        }
        !self.building.swap(true, Ordering::AcqRel)
    }

    fn levels(&self) -> std::sync::MutexGuard<'_, Vec<Option<Geometry<PackedMesh>>>> {
        self.levels
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }
}

pub(super) fn build(details: Arc<Details>, base: Geometry<PackedMesh>) {
    loop {
        let target = details
            .requested
            .load(Ordering::Relaxed)
            .min(details.ceiling.load(Ordering::Relaxed))
            .min(LIMIT);
        let next = {
            let levels = details.levels();
            (1..=target).find(|index| levels[*index].is_none())
        };
        let Some(next) = next else {
            details.building.store(false, Ordering::Release);
            if details.requested.load(Ordering::Relaxed) <= target {
                break;
            }
            if details.building.swap(true, Ordering::AcqRel) {
                break;
            }
            continue;
        };
        let previous = if next == 1 {
            base.load()
        } else {
            let levels = details.levels();
            levels[next - 1]
                .as_ref()
                .expect("the previous detail level is built")
                .load()
        };
        let Ok(previous) = previous else {
            details.halt(next - 1);
            break;
        };
        let mesh = previous.to_mesh();
        let simplified = simplify(&mesh, (mesh.triangle_count() / 2).max(1));
        if simplified.triangle_count() >= previous.triangle_count() {
            details.halt(next - 1);
            break;
        }
        let packed = PackedMesh::new(simplified);
        let Ok(geometry) = base.replace(packed) else {
            details.halt(next - 1);
            break;
        };
        details.levels()[next] = Some(geometry);
    }
}
