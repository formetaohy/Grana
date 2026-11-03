use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};

pub struct Task<T> {
    events: Receiver<T>,
    cancel: Arc<AtomicBool>,
}

impl<T: Send + 'static> Task<T> {
    pub fn spawn(work: impl FnOnce(&AtomicBool, &mut dyn FnMut(T)) + Send + 'static) -> Self {
        let cancel = Arc::new(AtomicBool::new(false));
        let token = Arc::clone(&cancel);
        let (events, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let mut sink = |event: T| {
                let _ = events.send(event);
            };
            work(&token, &mut sink);
        });
        Self {
            events: receiver,
            cancel,
        }
    }

    pub fn poll(&self) -> Poll<T> {
        match self.events.try_recv() {
            Ok(event) => Poll::Event(event),
            Err(TryRecvError::Empty) => Poll::Empty,
            Err(TryRecvError::Disconnected) => Poll::Closed,
        }
    }
}

impl<T> Drop for Task<T> {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

pub enum Poll<T> {
    Event(T),
    Empty,
    Closed,
}
