use std::fs::File;
use std::io::BufWriter;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};

use crate::error::Error;

enum Message {
    Frame(Vec<u8>),
    Finish,
}

pub struct Animation {
    size: [u32; 2],
    frames: u32,
    written: u32,
    sender: Option<Sender<Message>>,
    worker: Option<JoinHandle<Result<(), Error>>>,
}

impl Animation {
    pub fn start(path: &Path, size: [u32; 2], frames: u32, fps: u32) -> Result<Self, Error> {
        assert!(frames > 0, "an animation holds at least one frame");
        assert!(
            size[0] > 0 && size[1] > 0,
            "an animation frame covers at least one pixel"
        );
        let fps = u16::try_from(fps).expect("an animation frame rate fits a frame delay");
        assert!(fps > 0, "an animation frame rate is a positive delay");
        let file = File::create(path).map_err(|error| Error::Write {
            path: path.to_path_buf(),
            detail: error.to_string(),
        })?;
        let (sender, receiver) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("grana.animation".to_string())
            .spawn(move || encode(file, size, frames, fps, receiver))
            .map_err(|error| Error::Encode(error.to_string()))?;
        Ok(Self {
            size,
            frames,
            written: 0,
            sender: Some(sender),
            worker: Some(worker),
        })
    }

    pub fn frames(&self) -> u32 {
        self.frames
    }

    pub fn frame(&self) -> u32 {
        self.written
    }

    pub fn complete(&self) -> bool {
        self.written >= self.frames
    }

    pub fn push(&mut self, rgba: &[u8]) -> Result<(), Error> {
        assert_eq!(
            rgba.len(),
            (self.size[0] * self.size[1] * 4) as usize,
            "an animation frame covers its size"
        );
        assert!(
            self.written < self.frames,
            "an animation holds no more than its frames"
        );
        let Some(sender) = self.sender.as_ref() else {
            return Err(Error::Stopped);
        };
        sender
            .send(Message::Frame(rgba.to_vec()))
            .map_err(|_| Error::Stopped)?;
        self.written += 1;
        Ok(())
    }

    pub fn finish(mut self) -> Result<(), Error> {
        if !self.complete() {
            return Err(Error::Incomplete {
                index: self.written,
                frames: self.frames,
            });
        }
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

impl Drop for Animation {
    fn drop(&mut self) {
        self.sender.take();
        let _ = self.join();
    }
}

fn encode(
    file: File,
    size: [u32; 2],
    frames: u32,
    fps: u16,
    receiver: Receiver<Message>,
) -> Result<(), Error> {
    let mut encoder = png::Encoder::new(BufWriter::new(file), size[0], size[1]);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
    encoder
        .set_animated(frames, 0)
        .map_err(|error| Error::Encode(error.to_string()))?;
    let mut writer = encoder
        .write_header()
        .map_err(|error| Error::Encode(error.to_string()))?;
    let mut failure = None;
    loop {
        match receiver.recv() {
            Ok(Message::Frame(pixels)) => {
                if failure.is_none() {
                    let written = writer
                        .set_frame_delay(1, fps)
                        .and_then(|()| writer.write_image_data(&pixels));
                    if let Err(error) = written {
                        failure = Some(Error::Encode(error.to_string()));
                    }
                }
            }
            Ok(Message::Finish) => {
                if let Some(error) = failure {
                    return Err(error);
                }
                return writer
                    .finish()
                    .map_err(|error| Error::Encode(error.to_string()));
            }
            Err(_) => return Ok(()),
        }
    }
}
