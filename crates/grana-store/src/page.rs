use std::any::Any;
use std::marker::PhantomData;
use std::sync::Arc;

use crate::error::Error;
use crate::store::SpillStore;

pub trait Payload: Send + Sync + 'static {
    fn encode(&self) -> Vec<u8>;

    fn stored_bytes(&self) -> u64;

    fn into_any(self: Arc<Self>) -> Arc<dyn Any + Send + Sync>;
}

pub trait Decode: Payload + Sized {
    fn decode(bytes: &[u8]) -> Option<Self>;
}

pub struct StoreId<T> {
    raw: u32,
    payload: PhantomData<fn() -> T>,
}

impl<T> StoreId<T> {
    pub(crate) fn new(raw: u32) -> Self {
        Self {
            raw,
            payload: PhantomData,
        }
    }

    pub(crate) fn raw(self) -> u32 {
        self.raw
    }
}

impl<T> Clone for StoreId<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for StoreId<T> {}

impl<T> PartialEq for StoreId<T> {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }
}

impl<T> Eq for StoreId<T> {}

impl<T> std::hash::Hash for StoreId<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.raw.hash(state);
    }
}

impl<T> std::fmt::Debug for StoreId<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_tuple("StoreId").field(&self.raw).finish()
    }
}

pub struct Page<T> {
    store: Arc<SpillStore>,
    id: StoreId<T>,
}

impl<T: Decode> Page<T> {
    pub fn stored(store: &Arc<SpillStore>, payload: T) -> Result<Arc<Self>, Error> {
        let id = store.insert(payload)?;
        Ok(Arc::new(Self {
            store: Arc::clone(store),
            id,
        }))
    }

    pub fn load(&self) -> Result<Arc<T>, Error> {
        self.store.load(self.id)
    }

    pub fn bytes(&self) -> u64 {
        self.store.bytes(self.id.raw()).unwrap_or_default()
    }
}

impl<T> Drop for Page<T> {
    fn drop(&mut self) {
        self.store.release(self.id.raw());
    }
}

pub enum Stored<T> {
    Resident(Arc<T>),
    Paged(Arc<Page<T>>),
}

impl<T: Decode> Stored<T> {
    pub fn resident(payload: T) -> Self {
        Self::Resident(Arc::new(payload))
    }

    pub fn paged(store: &Arc<SpillStore>, payload: T) -> Result<Self, Error> {
        Ok(Self::Paged(Page::stored(store, payload)?))
    }

    pub fn load(&self) -> Result<Arc<T>, Error> {
        match self {
            Self::Resident(payload) => Ok(Arc::clone(payload)),
            Self::Paged(page) => page.load(),
        }
    }

    pub fn bytes(&self) -> u64 {
        match self {
            Self::Resident(payload) => payload.stored_bytes(),
            Self::Paged(page) => page.bytes(),
        }
    }

    pub fn replace(&self, payload: T) -> Result<Self, Error> {
        Ok(match self {
            Self::Resident(_) => Self::resident(payload),
            Self::Paged(page) => Self::paged(&page.store(), payload)?,
        })
    }
}

impl<T> Page<T> {
    pub fn store(&self) -> Arc<SpillStore> {
        Arc::clone(&self.store)
    }
}

impl<T> Clone for Stored<T> {
    fn clone(&self) -> Self {
        match self {
            Self::Resident(payload) => Self::Resident(Arc::clone(payload)),
            Self::Paged(page) => Self::Paged(Arc::clone(page)),
        }
    }
}

impl<T: Decode + std::fmt::Debug> std::fmt::Debug for Stored<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Resident(payload) => formatter.debug_tuple("Resident").field(payload).finish(),
            Self::Paged(page) => formatter
                .debug_struct("Paged")
                .field("bytes", &page.bytes())
                .finish(),
        }
    }
}

impl<T: Decode + PartialEq> PartialEq for Stored<T> {
    fn eq(&self, other: &Self) -> bool {
        match (self.load(), other.load()) {
            (Ok(left), Ok(right)) => left == right,
            _ => false,
        }
    }
}
