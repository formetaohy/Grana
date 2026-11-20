use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};

use grana_session::{Inquiry, InquiryKind};
use winit::window::Window;

use super::App;

pub(super) type Answer = Box<dyn FnOnce(&mut App, Option<PathBuf>)>;

pub(super) enum Awaiting {
    Session,
    App(Answer),
}

pub(super) enum Reply {
    Chosen(PathBuf),
    Cancelled,
}

impl Reply {
    fn path(self) -> Option<PathBuf> {
        match self {
            Self::Chosen(path) => Some(path),
            Self::Cancelled => None,
        }
    }
}

pub(super) struct Service {
    requests: Sender<Inquiry>,
    results: Receiver<Reply>,
}

impl Service {
    pub(super) fn spawn(window: Arc<Window>, wake: impl Fn() + Send + 'static) -> Self {
        let (requests, queue) = mpsc::channel();
        let (sender, results) = mpsc::channel();
        std::thread::Builder::new()
            .name("grana-dialog".to_string())
            .spawn(move || {
                while let Ok(inquiry) = queue.recv() {
                    let reply = ask(&inquiry, &window);
                    if sender.send(reply).is_err() {
                        return;
                    }
                    wake();
                }
            })
            .expect("the dialog worker starts");
        Self { requests, results }
    }

    fn ask(&self, inquiry: Inquiry) {
        self.requests
            .send(inquiry)
            .expect("the dialog worker answers until the window closes");
    }

    fn poll(&self) -> Option<Reply> {
        match self.results.try_recv() {
            Ok(reply) => Some(reply),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => panic!("the dialog worker stopped"),
        }
    }
}

fn ask(inquiry: &Inquiry, parent: &Window) -> Reply {
    let mut dialog = rfd::FileDialog::new()
        .set_title(inquiry.title())
        .set_parent(parent);
    for filter in inquiry.mode().filters() {
        dialog = dialog.add_filter(&filter.name, &filter.extensions);
    }
    let chosen = match inquiry.mode() {
        InquiryKind::Folder => dialog.pick_folder(),
        InquiryKind::File { .. } => dialog.pick_file(),
        InquiryKind::Save { name, .. } => dialog.set_file_name(name).save_file(),
    };
    chosen.map_or(Reply::Cancelled, Reply::Chosen)
}

impl App {
    pub(super) fn ask(
        &mut self,
        inquiry: Inquiry,
        answer: impl FnOnce(&mut App, Option<PathBuf>) + 'static,
    ) {
        let service = self.dialog.as_ref().expect("a window answers an inquiry");
        service.ask(inquiry);
        self.awaiting.push_back(Awaiting::App(Box::new(answer)));
    }

    pub(super) fn poll_inquiry(&mut self) -> bool {
        let Some(dialog) = self.dialog.as_ref() else {
            return false;
        };
        if let Some(inquiry) = self.session.take_inquiry() {
            dialog.ask(inquiry);
            self.awaiting.push_back(Awaiting::Session);
        }
        let Some(reply) = dialog.poll() else {
            return false;
        };
        let Some(awaiting) = self.awaiting.pop_front() else {
            panic!("a dialog reply arrives for a pending inquiry only");
        };
        let path = reply.path();
        match awaiting {
            Awaiting::Session => self.session.answer_inquiry(path),
            Awaiting::App(answer) => answer(self, path),
        }
        self.request_redraw();
        true
    }
}
