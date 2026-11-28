use std::path::{Path, PathBuf};
use std::process::Command;

use grana_exec::{Executor, Pipe, Poll, Priority, Role, Sender};
use winit::event_loop::EventLoopProxy;

use super::Wake;

const CAPACITY: usize = 8;

enum Event {
    Measured(u64),
    Cleared(Result<u64, String>),
}

pub(super) struct Storage {
    executor: Executor,
    sender: Sender<Event>,
    events: Pipe<Event>,
    wake: EventLoopProxy<Wake>,
    data: Option<PathBuf>,
    cache: Option<u64>,
    freed: Option<u64>,
    clearing: bool,
    measuring: bool,
    error: Option<String>,
}

impl Storage {
    pub(super) fn new(wake: EventLoopProxy<Wake>) -> Self {
        let (sender, events) = Pipe::new(CAPACITY);
        let mut storage = Self {
            executor: Executor::new(Role::Read, 1),
            sender,
            events,
            wake,
            data: grana_dir::data(),
            cache: None,
            freed: None,
            clearing: false,
            measuring: false,
            error: None,
        };
        storage.measure();
        storage
    }

    pub(super) fn data(&self) -> Option<&Path> {
        self.data.as_deref()
    }

    pub(super) fn cache(&self) -> Option<u64> {
        self.cache
    }

    pub(super) fn freed(&self) -> Option<u64> {
        self.freed
    }

    pub(super) fn clearing(&self) -> bool {
        self.clearing
    }

    pub(super) fn measuring(&self) -> bool {
        self.measuring
    }

    pub(super) fn take_error(&mut self) -> Option<String> {
        self.error.take()
    }

    pub(super) fn reveal_data(&self) -> Result<(), String> {
        let path = self
            .data
            .as_ref()
            .ok_or_else(|| "the system assigns no data directory".to_string())?;
        std::fs::create_dir_all(path)
            .map_err(|error| format!("failed to create {}: {error}", path.display()))?;
        Command::new("explorer")
            .arg(path)
            .spawn()
            .map_err(|error| format!("failed to open {}: {error}", path.display()))?;
        Ok(())
    }

    pub(super) fn clear(&mut self) {
        if self.clearing || self.measuring {
            return;
        }
        self.clearing = true;
        self.freed = None;
        let sender = self.sender.clone();
        let wake = self.wake.clone();
        self.executor.submit_with(Priority::Background, move |_| {
            let result = grana_image::clear_cache()
                .and_then(|pyramid| {
                    grana_pipeline::surface::clear_cache().map(|surface| pyramid + surface)
                })
                .map_err(|error| error.to_string());
            if sender.send(Event::Cleared(result)) {
                let _ = wake.send_event(Wake::Redraw);
            }
        });
    }

    pub(super) fn poll(&mut self) -> bool {
        let mut changed = false;
        loop {
            match self.events.poll() {
                Poll::Event(Event::Measured(bytes)) => {
                    self.measuring = false;
                    self.cache = Some(bytes);
                    changed = true;
                }
                Poll::Event(Event::Cleared(result)) => {
                    self.clearing = false;
                    match result {
                        Ok(freed) => {
                            self.freed = Some(freed);
                            self.cache = Some(self.cache.unwrap_or(0).saturating_sub(freed));
                            self.measure();
                        }
                        Err(detail) => self.error = Some(detail),
                    }
                    changed = true;
                }
                Poll::Empty => break,
                Poll::Closed => {
                    self.measuring = false;
                    self.clearing = false;
                    self.error = Some("the storage worker stopped".to_string());
                    changed = true;
                    break;
                }
            }
        }
        changed
    }

    fn measure(&mut self) {
        if self.measuring || self.clearing {
            return;
        }
        self.measuring = true;
        let sender = self.sender.clone();
        let wake = self.wake.clone();
        let roots = [
            grana_image::cache_root(),
            grana_pipeline::surface::cache_root(),
        ];
        self.executor.submit_with(Priority::Background, move |_| {
            let bytes = roots
                .iter()
                .flatten()
                .map(|root| grana_file::bytes(root).unwrap_or(0))
                .sum();
            if sender.send(Event::Measured(bytes)) {
                let _ = wake.send_event(Wake::Redraw);
            }
        });
    }
}
