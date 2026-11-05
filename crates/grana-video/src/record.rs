use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};

use crate::encoder::Encoder;
use crate::error::Error;
use crate::movie::{Codec, Movie};

enum Message {
    Frame(Vec<u8>),
    Finish,
}

enum Compressor {
    Lossy(Box<Encoder>),
    Lossless,
}

struct Clip {
    movie: Movie,
    compressor: Compressor,
}

impl Clip {
    fn push(&mut self, rgba: &[u8]) -> Result<(), Error> {
        match &mut self.compressor {
            Compressor::Lossy(encoder) => {
                let sample = encoder.encode(rgba)?;
                self.movie.sample(&sample.bytes, sample.key)
            }
            Compressor::Lossless => self
                .movie
                .sample(&png_frame(self.movie.size(), rgba)?, true),
        }
    }

    fn finish(self) -> Result<(), Error> {
        match self.compressor {
            Compressor::Lossy(encoder) => {
                let (sps, pps) = encoder.parameter_sets()?;
                self.movie.finish(Codec::H264 {
                    sps: sps.to_vec(),
                    pps: pps.to_vec(),
                })
            }
            Compressor::Lossless => self.movie.finish(Codec::Png),
        }
    }
}

pub struct Recorder {
    size: [u32; 2],
    sender: Option<Sender<Message>>,
    worker: Option<JoinHandle<Result<(), Error>>>,
}

impl Recorder {
    pub fn start(path: &Path, size: [u32; 2], fps: u32, alpha: bool) -> Result<Self, Error> {
        let compressor = if alpha {
            if size[0] == 0 || size[1] == 0 {
                return Err(Error::Size { size });
            }
            Compressor::Lossless
        } else {
            Compressor::Lossy(Box::new(Encoder::new(size, fps)?))
        };
        let clip = Clip {
            movie: Movie::create(path, size, fps, alpha)?,
            compressor,
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

fn png_frame(size: [u32; 2], rgba: &[u8]) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, size[0], size[1]);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
    let mut writer = encoder
        .write_header()
        .map_err(|error| Error::Encode(error.to_string()))?;
    writer
        .write_image_data(rgba)
        .map_err(|error| Error::Encode(error.to_string()))?;
    writer
        .finish()
        .map_err(|error| Error::Encode(error.to_string()))?;
    Ok(bytes)
}
