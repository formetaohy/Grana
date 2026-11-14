use std::sync::Arc;

use grana_object::{SpotPage, SpotSet};
use grana_store::{Page, SpillStore};

use crate::object::{ObjectFrame, ObjectKind};

const SPOT_BUDGET: u64 = 64 << 20;

pub(crate) const SPOT_PAGE_THRESHOLD: usize = 1 << 20;

#[derive(Clone)]
pub enum Frame {
    Resident(Arc<ObjectFrame>),
    Paged {
        page: Arc<Page<SpotPage>>,
        objects: usize,
    },
}

impl Frame {
    pub fn resident(frame: Arc<ObjectFrame>) -> Self {
        Self::Resident(frame)
    }

    pub(crate) fn paged(store: &Arc<SpillStore>, spots: Arc<SpotSet>) -> Self {
        let objects = spots.len();
        let page = Page::stored(store, SpotPage::new(spots))
            .unwrap_or_else(|error| panic!("a localization frame needs a page: {error}"));
        Self::Paged { page, objects }
    }

    pub fn kind(&self) -> ObjectKind {
        match self {
            Self::Resident(frame) => frame.kind(),
            Self::Paged { .. } => ObjectKind::Spots,
        }
    }

    pub fn objects(&self) -> usize {
        match self {
            Self::Resident(frame) => frame.object_count(),
            Self::Paged { objects, .. } => *objects,
        }
    }

    pub fn store(&self) -> Option<Arc<SpillStore>> {
        match self {
            Self::Resident(_) => None,
            Self::Paged { page, .. } => Some(page.store()),
        }
    }

    pub fn shared(&self) -> Arc<ObjectFrame> {
        match self {
            Self::Resident(frame) => Arc::clone(frame),
            Self::Paged { page, .. } => {
                let page = self.load(page);
                Arc::new(ObjectFrame::Spots(page.spots()))
            }
        }
    }

    pub fn spot_frame(&self) -> Option<Arc<SpotSet>> {
        match self {
            Self::Resident(frame) => match frame.as_ref() {
                ObjectFrame::Spots(spots) => Some(Arc::clone(spots)),
                ObjectFrame::Surfaces(_) | ObjectFrame::Filaments(_) | ObjectFrame::Cells(_) => {
                    None
                }
            },
            Self::Paged { page, .. } => Some(self.load(page).spots()),
        }
    }

    fn load(&self, page: &Page<SpotPage>) -> Arc<SpotPage> {
        page.load()
            .unwrap_or_else(|error| panic!("a paged localization frame cannot load: {error}"))
    }
}

pub(crate) fn open_store() -> Arc<SpillStore> {
    SpillStore::temporary(SPOT_BUDGET)
        .unwrap_or_else(|error| panic!("a localization set needs a spill store: {error}"))
}

pub(crate) fn place(store: Option<&Arc<SpillStore>>, frame: Arc<ObjectFrame>) -> Frame {
    match (frame.as_ref(), store) {
        (ObjectFrame::Spots(spots), Some(store)) => Frame::paged(store, Arc::clone(spots)),
        (ObjectFrame::Spots(_), None) => Frame::resident(frame),
        (ObjectFrame::Surfaces(_), _) => Frame::resident(frame),
        (ObjectFrame::Filaments(_), _) => Frame::resident(frame),
        (ObjectFrame::Cells(_), _) => Frame::resident(frame),
    }
}

impl std::fmt::Debug for Frame {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Resident(frame) => formatter.debug_tuple("Resident").field(frame).finish(),
            Self::Paged { objects, .. } => formatter
                .debug_struct("Paged")
                .field("objects", objects)
                .finish(),
        }
    }
}

impl PartialEq for Frame {
    fn eq(&self, other: &Self) -> bool {
        if self.objects() != other.objects() {
            return false;
        }
        self.shared() == other.shared()
    }
}
