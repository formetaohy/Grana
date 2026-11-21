use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use grana_editor::{UpdateFailure, UpdatePhase, UpdateRequest};
use grana_exec::{Executor, Pipe, Poll, Role};
use grana_update::{Failure, Offer, Staged, StagedArtifact, Target};
use winit::event_loop::EventLoopProxy;

use super::{App, Resume, Wake};
use crate::preference::Preference;

const INTERVAL: u64 = 24 * 60 * 60;
const CAPACITY: usize = 64;
const STEP: u64 = 256 << 10;
const GRACE: Duration = Duration::from_secs(3);

enum Event {
    Checked(Result<Option<Offer>, grana_update::Error>),
    Progress(u64, u64),
    Staged(Result<Staged, grana_update::Error>),
}

struct Running {
    cancel: Arc<AtomicBool>,
    events: Pipe<Event>,
}

pub(super) struct Update {
    executor: Executor,
    running: Option<Running>,
    source: grana_update::Source,
    target: Target,
    offer: Option<Offer>,
    staged: Option<Staged>,
    phase: UpdatePhase,
    progress: Option<(u64, u64)>,
    automatic: bool,
    last: Option<u64>,
    wake: EventLoopProxy<Wake>,
}

impl Update {
    pub fn new(preference: &Preference, wake: EventLoopProxy<Wake>) -> Self {
        Self {
            executor: Executor::new(Role::Read, 1),
            running: None,
            source: grana_update::Source::github(),
            target: grana_update::detect(),
            offer: None,
            staged: None,
            phase: UpdatePhase::Idle,
            progress: None,
            automatic: preference.check,
            last: preference.last,
            wake,
        }
    }

    pub fn phase(&self) -> UpdatePhase {
        self.phase
    }

    pub fn progress(&self) -> Option<(u64, u64)> {
        self.progress
    }

    pub fn automatic(&self) -> bool {
        self.automatic
    }

    pub fn last(&self) -> Option<u64> {
        self.last
    }

    pub fn size(&self) -> Option<u64> {
        self.offer.as_ref().map(|offer| match self.target {
            Target::Portable => offer.portable.size,
            Target::Installed { .. } => offer.installer.size,
        })
    }

    pub fn notes(&self) -> Option<&str> {
        self.offer.as_ref().map(|offer| offer.notes.as_str())
    }

    pub fn version(&self) -> Option<String> {
        self.offer.as_ref().map(|offer| offer.version.to_string())
    }

    pub fn due(&self) -> bool {
        self.automatic
            && self.running.is_none()
            && self
                .last
                .is_none_or(|last| now().saturating_sub(last) >= INTERVAL)
    }

    pub fn check(&mut self) {
        if self.running.is_some() {
            return;
        }
        let source = self.source.clone();
        let current = grana_update::current();
        let (sender, events) = Pipe::new(CAPACITY);
        let wake = self.wake.clone();
        self.executor.submit(move |_| {
            let result = grana_update::check(&source, &current);
            if sender.send(Event::Checked(result)) {
                let _ = wake.send_event(Wake::Redraw);
            }
        });
        self.running = Some(Running {
            cancel: Arc::new(AtomicBool::new(false)),
            events,
        });
        self.phase = UpdatePhase::Checking;
        self.progress = None;
    }

    pub fn download(&mut self) {
        if self.running.is_some() {
            return;
        }
        let Some(offer) = self.offer.clone() else {
            return;
        };
        let staging = match grana_update::Staging::system() {
            Ok(staging) => staging,
            Err(error) => {
                self.fail(&error);
                return;
            }
        };
        let target = self.target.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancel);
        let (sender, events) = Pipe::new(CAPACITY);
        let completion = sender.clone();
        let wake = self.wake.clone();
        self.executor.submit(move |_| {
            let mut sent = 0;
            let mut report = |done: u64, total: u64| {
                if done < sent + STEP && (total == 0 || done < total) {
                    return;
                }
                sent = done;
                if sender.send_until(Event::Progress(done, total), || {
                    flag.load(Ordering::Relaxed)
                }) {
                    let _ = wake.send_event(Wake::Redraw);
                }
            };
            let result = staging.stage(&target, &offer, &flag, &mut report);
            if completion.send(Event::Staged(result)) {
                let _ = wake.send_event(Wake::Redraw);
            }
        });
        self.running = Some(Running { cancel, events });
        self.phase = UpdatePhase::Downloading;
        self.progress = Some((0, 0));
    }

    pub fn cancel(&mut self) {
        if let Some(running) = self.running.as_ref() {
            running.cancel.store(true, Ordering::Relaxed);
        }
    }

    pub fn toggle(&mut self) {
        self.automatic = !self.automatic;
        if self.automatic {
            self.check();
        }
    }

    pub fn apply(&mut self) -> Option<Result<(), grana_update::Error>> {
        let staged = self.staged.take()?;
        let outcome = match &staged.artifact {
            StagedArtifact::Portable(executable) => grana_update::relaunch(executable),
            StagedArtifact::Installer(setup) => grana_update::hand_over(setup, &self.target),
        };
        Some(outcome)
    }

    pub fn fail(&mut self, error: &grana_update::Error) {
        grana_log::error!(target: "grana::update", %error, "the update did not apply");
        self.phase = phase_of(error.failure());
        self.progress = None;
    }

    pub fn poll(&mut self) -> bool {
        let Some(running) = self.running.take() else {
            return false;
        };
        let mut changed = false;
        let mut ended = false;
        loop {
            match running.events.poll() {
                Poll::Event(Event::Progress(done, total)) => {
                    self.progress = Some((done, total));
                    changed = true;
                }
                Poll::Event(Event::Checked(result)) => {
                    self.finish_check(result);
                    ended = true;
                    changed = true;
                    break;
                }
                Poll::Event(Event::Staged(result)) => {
                    self.finish_stage(result);
                    ended = true;
                    changed = true;
                    break;
                }
                Poll::Empty => break,
                Poll::Closed => {
                    self.phase = UpdatePhase::Failed(UpdateFailure::Other);
                    ended = true;
                    changed = true;
                    break;
                }
            }
        }
        if !ended {
            self.running = Some(running);
        }
        changed
    }

    fn finish_check(&mut self, result: Result<Option<Offer>, grana_update::Error>) {
        self.last = Some(now());
        self.progress = None;
        match result {
            Ok(Some(offer)) => {
                grana_log::info!(target: "grana::update", version = %offer.version, "an update is available");
                self.phase = UpdatePhase::Available;
                self.offer = Some(offer);
            }
            Ok(None) => {
                self.phase = UpdatePhase::Current;
                self.offer = None;
            }
            Err(error) => {
                grana_log::warn!(target: "grana::update", %error, "the update check failed");
                self.phase = phase_of(error.failure());
                self.offer = None;
            }
        }
    }

    fn finish_stage(&mut self, result: Result<Staged, grana_update::Error>) {
        self.progress = None;
        match result {
            Ok(staged) => {
                grana_log::info!(target: "grana::update", version = %staged.version, "the update is staged");
                self.staged = Some(staged);
                self.phase = UpdatePhase::Ready;
            }
            Err(grana_update::Error::Cancelled) => {
                self.phase = UpdatePhase::Available;
            }
            Err(error) => {
                grana_log::error!(target: "grana::update", %error, "the update download failed");
                self.phase = phase_of(error.failure());
            }
        }
    }
}

fn phase_of(failure: Failure) -> UpdatePhase {
    UpdatePhase::Failed(match failure {
        Failure::Offline => UpdateFailure::Offline,
        Failure::Corrupt => UpdateFailure::Corrupt,
        Failure::Storage => UpdateFailure::Storage,
        Failure::Installer => UpdateFailure::Installer,
        Failure::Other => UpdateFailure::Other,
    })
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

impl App {
    pub(super) fn poll_update(&mut self) {
        if self.update.poll() {
            self.sync_update();
            self.request_redraw();
        }
    }

    pub(super) fn begin_update_check(&mut self) {
        if self.capture.is_some() || self.start.elapsed() < GRACE {
            return;
        }
        if self.update.due() {
            self.update.check();
        }
    }

    pub(super) fn update_wait(&self) -> Option<Duration> {
        if self.capture.is_some() || !self.update.due() {
            return None;
        }
        Some(GRACE.saturating_sub(self.start.elapsed()))
    }

    pub(super) fn sync_update(&mut self) {
        let phase = self.update.phase();
        let progress = self.update.progress();
        let size = self.update.size();
        let automatic = self.update.automatic();
        let version = self.update.version();
        let notes = self.update.notes().map(str::to_string);
        let view = &mut self.workspace.update;
        view.phase = phase;
        view.progress = progress;
        view.size = size;
        view.automatic = automatic;
        view.version = version;
        view.notes = notes;
    }

    pub(super) fn consume_update(&mut self) -> bool {
        let Some(request) = self.workspace.update.request.take() else {
            return false;
        };
        match request {
            UpdateRequest::Check => self.update.check(),
            UpdateRequest::Download => self.update.download(),
            UpdateRequest::Cancel => self.update.cancel(),
            UpdateRequest::ToggleCheck => self.update.toggle(),
            UpdateRequest::Notes => {
                if let Some(notes) = self.update.notes().map(str::to_string)
                    && let Err(error) = grana_update::open(&notes)
                {
                    self.update.fail(&error);
                }
            }
            UpdateRequest::Apply => {
                self.resume = Some(Resume::Update);
                return self.follow_resume();
            }
        }
        self.sync_update();
        self.request_redraw();
        false
    }
}
