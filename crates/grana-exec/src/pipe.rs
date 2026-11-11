use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError};

pub struct Sender<T> {
    inner: SyncSender<T>,
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<T> Sender<T> {
    pub fn send(&self, value: T) -> bool {
        self.inner.send(value).is_ok()
    }
}

pub struct Pipe<T> {
    receiver: Receiver<T>,
}

impl<T> Pipe<T> {
    pub fn new(capacity: usize) -> (Sender<T>, Self) {
        assert!(
            capacity > 0,
            "an event pipe holds at least one buffered event"
        );
        let (sender, receiver) = mpsc::sync_channel(capacity);
        (Sender { inner: sender }, Self { receiver })
    }

    pub fn poll(&self) -> Poll<T> {
        match self.receiver.try_recv() {
            Ok(value) => Poll::Event(value),
            Err(TryRecvError::Empty) => Poll::Empty,
            Err(TryRecvError::Disconnected) => Poll::Closed,
        }
    }

    pub fn receive(&self) -> Option<T> {
        self.receiver.recv().ok()
    }
}

pub enum Poll<T> {
    Event(T),
    Empty,
    Closed,
}
