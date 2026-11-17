use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::error::Error;

const MOVIE_TIMESCALE: u32 = 1000;
const MATRIX: [u32; 9] = [0x0001_0000, 0, 0, 0, 0x0001_0000, 0, 0, 0, 0x4000_0000];
const RESOLUTION: u32 = 0x0048_0000;
const UNKNOWN_LANGUAGE: u16 = 0x55c4;
const HIGH_PROFILES: [u8; 4] = [100, 110, 122, 144];
const AVC_DEPTH: u16 = 0x0018;
const PNG_DEPTH: u16 = 0x0020;
const QUICKTIME: [u8; 4] = *b"qt  ";
const MEDIA_BOX_HEADER: [u8; 8] = [0, 0, 0, 0, b'm', b'd', b'a', b't'];
const MEDIA_BOX_PAYLOAD_LIMIT: u64 = u32::MAX as u64 - 8;

pub(crate) enum Codec {
    H264 { sps: Vec<u8>, pps: Vec<u8> },
    Png,
}

pub(crate) struct Movie {
    path: PathBuf,
    atomic: Option<grana_file::Atomic>,
    sink: BufWriter<File>,
    size: [u32; 2],
    fps: u32,
    samples: Vec<u32>,
    keys: Vec<u32>,
    chunks: Vec<Chunk>,
    media: Option<MediaBox>,
    alpha: bool,
}

struct Chunk {
    offset: u64,
    samples: u32,
}

struct MediaBox {
    header: u64,
    bytes: u64,
    samples: u32,
}

impl MediaBox {
    fn admits(&self, length: u32) -> bool {
        self.bytes + u64::from(length) <= MEDIA_BOX_PAYLOAD_LIMIT
    }

    fn size(&self) -> u32 {
        u32::try_from(self.bytes + 8).expect("a media box carries its size in four bytes")
    }
}

impl Movie {
    pub fn create(path: &Path, size: [u32; 2], fps: u32, alpha: bool) -> Result<Self, Error> {
        let atomic = grana_file::Atomic::file(path).map_err(|error| Error::Write {
            path: path.to_path_buf(),
            detail: error.to_string(),
        })?;
        let temporary = atomic.staging().to_path_buf();
        let file = File::create(&temporary).map_err(|error| Error::Write {
            path: temporary.clone(),
            detail: error.to_string(),
        })?;
        let mut sink = BufWriter::new(file);
        write(&mut sink, &temporary, &ftyp(alpha))?;
        Ok(Self {
            path: path.to_path_buf(),
            atomic: Some(atomic),
            sink,
            size,
            fps,
            samples: Vec::new(),
            keys: Vec::new(),
            chunks: Vec::new(),
            media: None,
            alpha,
        })
    }

    pub fn size(&self) -> [u32; 2] {
        self.size
    }

    pub fn sample(&mut self, bytes: &[u8], key: bool) -> Result<(), Error> {
        let length = sample_length(bytes.len())?;
        if !self
            .media
            .as_ref()
            .is_some_and(|media| media.admits(length))
        {
            self.open_media()?;
        }
        let temporary = self.staging();
        write(&mut self.sink, &temporary, bytes)?;
        self.samples.push(length);
        if key {
            self.keys.push(self.samples.len() as u32);
        }
        let media = self.media.as_mut().expect("a media box is open");
        media.bytes += u64::from(length);
        media.samples += 1;
        Ok(())
    }

    pub fn finish(mut self, codec: Codec) -> Result<(), Error> {
        self.close_media()?;
        let moov = self.moov(&codec);
        let temporary = self.staging();
        let fault = |error: std::io::Error| Error::Write {
            path: temporary.clone(),
            detail: error.to_string(),
        };
        self.sink.seek(SeekFrom::End(0)).map_err(fault)?;
        self.sink.write_all(&moov).map_err(fault)?;
        self.sink.flush().map_err(fault)?;
        self.atomic
            .take()
            .expect("the movie is open")
            .commit()
            .map_err(|error| Error::Write {
                path: self.path.clone(),
                detail: error.to_string(),
            })
    }

    fn staging(&self) -> PathBuf {
        self.atomic
            .as_ref()
            .expect("the movie is open")
            .staging()
            .to_path_buf()
    }

    fn open_media(&mut self) -> Result<(), Error> {
        self.close_media()?;
        let temporary = self.staging();
        let fault = |error: std::io::Error| Error::Write {
            path: temporary.clone(),
            detail: error.to_string(),
        };
        let header = self.sink.seek(SeekFrom::End(0)).map_err(fault)?;
        self.sink.write_all(&MEDIA_BOX_HEADER).map_err(fault)?;
        self.media = Some(MediaBox {
            header,
            bytes: 0,
            samples: 0,
        });
        Ok(())
    }

    fn close_media(&mut self) -> Result<(), Error> {
        let Some(media) = self.media.take() else {
            return Ok(());
        };
        let temporary = self.staging();
        let fault = |error: std::io::Error| Error::Write {
            path: temporary.clone(),
            detail: error.to_string(),
        };
        self.sink
            .seek(SeekFrom::Start(media.header))
            .map_err(fault)?;
        self.sink
            .write_all(&media.size().to_be_bytes())
            .map_err(fault)?;
        self.chunks.push(Chunk {
            offset: media.header + 8,
            samples: media.samples,
        });
        Ok(())
    }

    fn moov(&self, codec: &Codec) -> Vec<u8> {
        let frames = self.samples.len() as u32;
        let duration = ((u64::from(frames) * u64::from(MOVIE_TIMESCALE) + u64::from(self.fps) / 2)
            / u64::from(self.fps)) as u32;
        let mvhd = mvhd(duration);
        let tkhd = tkhd(self.size, duration);
        let mdhd = mdhd(self.fps, frames);
        let hdlr = hdlr();
        let vmhd = vmhd();
        let dinf = dinf();
        let stbl = self.stbl(codec);
        let minf = boxed(*b"minf", [vmhd, dinf, stbl].concat());
        let mdia = boxed(*b"mdia", [mdhd, hdlr, minf].concat());
        let trak = boxed(*b"trak", [tkhd, mdia].concat());
        boxed(*b"moov", [mvhd, trak].concat())
    }

    fn stbl(&self, codec: &Codec) -> Vec<u8> {
        let entry = match codec {
            Codec::H264 { sps, pps } => {
                assert!(!self.alpha, "an h264 movie carries no alpha channel");
                video_entry(*b"avc1", AVC_DEPTH, self.size, avcc(sps, pps))
            }
            Codec::Png => {
                assert!(self.alpha, "a png movie carries its alpha channel");
                video_entry(*b"png ", PNG_DEPTH, self.size, Vec::new())
            }
        };
        let mut stsd = Vec::new();
        push_u32(&mut stsd, 1);
        stsd.extend_from_slice(&entry);

        let mut stts = Vec::new();
        push_u32(&mut stts, 1);
        push_u32(&mut stts, self.samples.len() as u32);
        push_u32(&mut stts, 1);

        let runs = chunk_runs(&self.chunks);
        let mut stsc = Vec::new();
        push_u32(&mut stsc, runs.len() as u32);
        for (first, samples) in runs {
            push_u32(&mut stsc, first);
            push_u32(&mut stsc, samples);
            push_u32(&mut stsc, 1);
        }

        let mut stsz = Vec::new();
        push_u32(&mut stsz, 0);
        push_u32(&mut stsz, self.samples.len() as u32);
        for sample in &self.samples {
            push_u32(&mut stsz, *sample);
        }

        let mut stss = Vec::new();
        push_u32(&mut stss, self.keys.len() as u32);
        for key in &self.keys {
            push_u32(&mut stss, *key);
        }

        boxed(
            *b"stbl",
            [
                full_box(*b"stsd", 0, 0, stsd),
                full_box(*b"stts", 0, 0, stts),
                full_box(*b"stsc", 0, 0, stsc),
                full_box(*b"stsz", 0, 0, stsz),
                chunk_offsets(&self.chunks),
                full_box(*b"stss", 0, 0, stss),
            ]
            .concat(),
        )
    }
}

fn sample_length(bytes: usize) -> Result<u32, Error> {
    u32::try_from(bytes)
        .ok()
        .filter(|length| u64::from(*length) <= MEDIA_BOX_PAYLOAD_LIMIT)
        .ok_or_else(|| Error::Encode("an encoded sample outgrew a media box".to_string()))
}

fn chunk_runs(chunks: &[Chunk]) -> Vec<(u32, u32)> {
    let mut runs: Vec<(u32, u32)> = Vec::new();
    for (index, chunk) in chunks.iter().enumerate() {
        let repeated = runs
            .last()
            .is_some_and(|(_, samples)| *samples == chunk.samples);
        if !repeated {
            runs.push((index as u32 + 1, chunk.samples));
        }
    }
    runs
}

fn chunk_offsets(chunks: &[Chunk]) -> Vec<u8> {
    let narrow: Option<Vec<u32>> = chunks
        .iter()
        .map(|chunk| u32::try_from(chunk.offset).ok())
        .collect();
    match narrow {
        Some(offsets) => {
            let mut stco = Vec::new();
            push_u32(&mut stco, offsets.len() as u32);
            for offset in offsets {
                push_u32(&mut stco, offset);
            }
            full_box(*b"stco", 0, 0, stco)
        }
        None => {
            let mut co64 = Vec::new();
            push_u32(&mut co64, chunks.len() as u32);
            for chunk in chunks {
                push_u64(&mut co64, chunk.offset);
            }
            full_box(*b"co64", 0, 0, co64)
        }
    }
}

fn mvhd(duration: u32) -> Vec<u8> {
    let mut payload = Vec::new();
    push_u32(&mut payload, 0);
    push_u32(&mut payload, 0);
    push_u32(&mut payload, MOVIE_TIMESCALE);
    push_u32(&mut payload, duration);
    push_u32(&mut payload, 0x0001_0000);
    push_u16(&mut payload, 0x0100);
    push_u16(&mut payload, 0);
    push_u32(&mut payload, 0);
    push_u32(&mut payload, 0);
    for value in MATRIX {
        push_u32(&mut payload, value);
    }
    for _ in 0..6 {
        push_u32(&mut payload, 0);
    }
    push_u32(&mut payload, 2);
    full_box(*b"mvhd", 0, 0, payload)
}

fn tkhd(size: [u32; 2], duration: u32) -> Vec<u8> {
    let mut payload = Vec::new();
    push_u32(&mut payload, 0);
    push_u32(&mut payload, 0);
    push_u32(&mut payload, 1);
    push_u32(&mut payload, 0);
    push_u32(&mut payload, duration);
    push_u32(&mut payload, 0);
    push_u32(&mut payload, 0);
    push_u16(&mut payload, 0);
    push_u16(&mut payload, 0);
    push_u16(&mut payload, 0);
    push_u16(&mut payload, 0);
    for value in MATRIX {
        push_u32(&mut payload, value);
    }
    push_u32(&mut payload, size[0] << 16);
    push_u32(&mut payload, size[1] << 16);
    full_box(*b"tkhd", 0, 3, payload)
}

fn mdhd(fps: u32, frames: u32) -> Vec<u8> {
    let mut payload = Vec::new();
    push_u32(&mut payload, 0);
    push_u32(&mut payload, 0);
    push_u32(&mut payload, fps);
    push_u32(&mut payload, frames);
    push_u16(&mut payload, UNKNOWN_LANGUAGE);
    push_u16(&mut payload, 0);
    full_box(*b"mdhd", 0, 0, payload)
}

fn hdlr() -> Vec<u8> {
    let mut payload = Vec::new();
    push_u32(&mut payload, 0);
    payload.extend_from_slice(b"vide");
    for _ in 0..3 {
        push_u32(&mut payload, 0);
    }
    payload.extend_from_slice(b"VideoHandler\0");
    full_box(*b"hdlr", 0, 0, payload)
}

fn vmhd() -> Vec<u8> {
    let mut payload = Vec::new();
    push_u16(&mut payload, 0);
    push_u16(&mut payload, 0);
    push_u16(&mut payload, 0);
    push_u16(&mut payload, 0);
    full_box(*b"vmhd", 0, 1, payload)
}

fn dinf() -> Vec<u8> {
    let mut dref = Vec::new();
    push_u32(&mut dref, 1);
    dref.extend_from_slice(&full_box(*b"url ", 0, 1, Vec::new()));
    boxed(*b"dinf", full_box(*b"dref", 0, 0, dref))
}

fn video_entry(kind: [u8; 4], depth: u16, size: [u32; 2], children: Vec<u8>) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&[0; 6]);
    push_u16(&mut payload, 1);
    push_u16(&mut payload, 0);
    push_u16(&mut payload, 0);
    for _ in 0..3 {
        push_u32(&mut payload, 0);
    }
    push_u16(&mut payload, size[0] as u16);
    push_u16(&mut payload, size[1] as u16);
    push_u32(&mut payload, RESOLUTION);
    push_u32(&mut payload, RESOLUTION);
    push_u32(&mut payload, 0);
    push_u16(&mut payload, 1);
    payload.extend_from_slice(&[0; 32]);
    push_u16(&mut payload, depth);
    push_u16(&mut payload, 0xffff);
    payload.extend_from_slice(&children);
    boxed(kind, payload)
}

fn avcc(sps: &[u8], pps: &[u8]) -> Vec<u8> {
    assert!(
        sps.len() >= 4 && sps.len() <= usize::from(u16::MAX),
        "a sequence parameter set carries a profile and a length"
    );
    assert!(
        !pps.is_empty() && pps.len() <= usize::from(u16::MAX),
        "a picture parameter set carries a length"
    );
    let mut payload = vec![1, sps[1], sps[2], sps[3], 0xff, 0xe1];
    push_u16(&mut payload, sps.len() as u16);
    payload.extend_from_slice(sps);
    payload.push(1);
    push_u16(&mut payload, pps.len() as u16);
    payload.extend_from_slice(pps);
    if HIGH_PROFILES.contains(&sps[1]) {
        payload.push(0xfd);
        payload.push(0xf8);
        payload.push(0xf8);
        payload.push(0);
    }
    boxed(*b"avcC", payload)
}

fn ftyp(alpha: bool) -> Vec<u8> {
    let mut payload = Vec::new();
    if alpha {
        payload.extend_from_slice(&QUICKTIME);
        push_u32(&mut payload, 512);
        payload.extend_from_slice(&QUICKTIME);
        return boxed(*b"ftyp", payload);
    }
    payload.extend_from_slice(b"isom");
    push_u32(&mut payload, 512);
    for brand in [b"isom", b"iso2", b"avc1", b"mp41"] {
        payload.extend_from_slice(brand);
    }
    boxed(*b"ftyp", payload)
}

fn boxed(kind: [u8; 4], payload: Vec<u8>) -> Vec<u8> {
    let size = u32::try_from(payload.len() + 8).expect("a box fits its four byte size");
    let mut out = Vec::with_capacity(payload.len() + 8);
    out.extend_from_slice(&size.to_be_bytes());
    out.extend_from_slice(&kind);
    out.extend_from_slice(&payload);
    out
}

fn full_box(kind: [u8; 4], version: u8, flags: u32, payload: Vec<u8>) -> Vec<u8> {
    let mut body = Vec::with_capacity(payload.len() + 4);
    body.push(version);
    body.extend_from_slice(&flags.to_be_bytes()[1..]);
    body.extend_from_slice(&payload);
    boxed(kind, body)
}

fn push_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn push_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn write(sink: &mut BufWriter<File>, path: &Path, bytes: &[u8]) -> Result<(), Error> {
    sink.write_all(bytes).map_err(|error| Error::Write {
        path: path.to_path_buf(),
        detail: error.to_string(),
    })
}
