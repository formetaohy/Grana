use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};

use crate::encoder::Encoder;
use crate::error::Error;
use crate::mp4::Mp4;

enum Message {
    Frame(Vec<u8>),
    Finish,
}

struct Clip {
    encoder: Encoder,
    mp4: Mp4,
}

impl Clip {
    fn push(&mut self, rgba: &[u8]) -> Result<(), Error> {
        let sample = self.encoder.encode(rgba)?;
        self.mp4.sample(&sample.bytes, sample.key)
    }

    fn finish(self) -> Result<(), Error> {
        let (sps, pps) = self.encoder.parameter_sets()?;
        self.mp4.finish(sps, pps)
    }
}

pub struct Recorder {
    size: [u32; 2],
    sender: Option<Sender<Message>>,
    worker: Option<JoinHandle<Result<(), Error>>>,
}

impl Recorder {
    pub fn start(path: &Path, size: [u32; 2], fps: u32) -> Result<Self, Error> {
        let clip = Clip {
            encoder: Encoder::new(size, fps)?,
            mp4: Mp4::create(path, size, fps)?,
        };
        let (sender, receiver) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("grana.movie".to_string())
            .spawn(move || encode(clip, receiver))
            .map_err(|error| Error::Encode(error.to_string()))?;
        Ok(Self {
            size,
            sender: Some(sender),
            worker: Some(worker),
        })
    }

    pub fn push(&mut self, rgba: &[u8]) -> Result<(), Error> {
        assert_eq!(
            rgba.len(),
            (self.size[0] * self.size[1] * 4) as usize,
            "a movie frame covers its size"
        );
        let Some(sender) = self.sender.as_ref() else {
            return Err(Error::Stopped);
        };
        sender
            .send(Message::Frame(rgba.to_vec()))
            .map_err(|_| Error::Stopped)
    }

    pub fn finish(mut self) -> Result<(), Error> {
        let Some(sender) = self.sender.take() else {
            return Err(Error::Stopped);
        };
        sender.send(Message::Finish).map_err(|_| Error::Stopped)?;
        self.join()
    }

    fn join(&mut self) -> Result<(), Error> {
        let Some(worker) = self.worker.take() else {
            return Ok(());
        };
        worker.join().map_err(|_| Error::Stopped)?
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.sender.take();
        let _ = self.join();
    }
}

fn encode(mut clip: Clip, receiver: Receiver<Message>) -> Result<(), Error> {
    let mut failure = None;
    loop {
        match receiver.recv() {
            Ok(Message::Frame(rgba)) => {
                if failure.is_none()
                    && let Err(error) = clip.push(&rgba)
                {
                    failure = Some(error);
                }
            }
            Ok(Message::Finish) => {
                if let Some(error) = failure {
                    return Err(error);
                }
                return clip.finish();
            }
            Err(_) => return Ok(()),
        }
    }
}
