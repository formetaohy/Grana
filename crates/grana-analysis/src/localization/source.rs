use grana_object::LocalizationFilter;

use super::LocalizationFrame;

pub trait LocalizationSource {
    fn len(&self) -> usize;

    fn frame(&self, index: usize, filter: LocalizationFilter) -> LocalizationFrame;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
