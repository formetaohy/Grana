use grana_raster::Overlay;

use crate::object_set::ObjectSetId;

#[derive(Clone, Default)]
pub struct Overlays {
    entries: Vec<(ObjectSetId, Overlay)>,
}

impl Overlays {
    pub fn get(&self, set: ObjectSetId) -> Option<&Overlay> {
        let index = self.index(set).ok()?;
        Some(&self.entries[index].1)
    }

    pub fn iter(&self) -> impl Iterator<Item = (ObjectSetId, &Overlay)> + '_ {
        self.entries.iter().map(|(set, overlay)| (*set, overlay))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn set(&mut self, set: ObjectSetId, overlay: Overlay) -> bool {
        match self.index(set) {
            Ok(index) => {
                let held = &self.entries[index].1;
                let same = held.bitmap().identity() == overlay.bitmap().identity()
                    && held.corners() == overlay.corners()
                    && held.tint() == overlay.tint()
                    && held.opacity() == overlay.opacity();
                if same {
                    return false;
                }
                self.entries[index].1 = overlay;
                true
            }
            Err(index) => {
                self.entries.insert(index, (set, overlay));
                true
            }
        }
    }

    pub fn remove(&mut self, set: ObjectSetId) -> bool {
        let Ok(index) = self.index(set) else {
            return false;
        };
        self.entries.remove(index);
        true
    }

    pub fn retain(&mut self, mut keep: impl FnMut(ObjectSetId) -> bool) -> bool {
        let held = self.entries.len();
        self.entries.retain(|(set, _)| keep(*set));
        held != self.entries.len()
    }

    pub fn clear(&mut self) -> bool {
        let held = !self.entries.is_empty();
        self.entries.clear();
        held
    }

    pub fn snapshot(&self) -> Vec<(ObjectSetId, Overlay)> {
        self.entries.clone()
    }

    fn index(&self, set: ObjectSetId) -> Result<usize, usize> {
        self.entries
            .binary_search_by_key(&set.raw(), |(held, _)| held.raw())
    }
}
