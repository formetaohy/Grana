use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::error::Error;

const MOVIE_TIMESCALE: u32 = 1000;
const MATRIX: [u32; 9] = [0x0001_0000, 0, 0, 0, 0x0001_0000, 0, 0, 0, 0x4000_0000];
const SAMPLE_DEPTH: u16 = 0x0018;
const RESOLUTION: u32 = 0x0048_0000;
const UNKNOWN_LANGUAGE: u16 = 0x55c4;
const HIGH_PROFILES: [u8; 4] = [100, 110, 122, 144];

pub(crate) struct Mp4 {
    path: PathBuf,
    temporary: PathBuf,
    sink: BufWriter<File>,
    size: [u32; 2],
    fps: u32,
    samples: Vec<u32>,
    keys: Vec<u32>,
    media: u64,
    start: u64,
}

impl Mp4 {
    pub fn create(path: &Path, size: [u32; 2], fps: u32) -> Result<Self, Error> {
        let temporary = temporary_path(path);
        let file = File::create(&temporary).map_err(|error| Error::Write {
            path: temporary.clone(),
            detail: error.to_string(),
        })?;
        let mut sink = BufWriter::new(file);
        let header = ftyp();
        write(&mut sink, &temporary, &header)?;
        write(&mut sink, &temporary, &[0; 8])?;
        let start = (header.len() + 8) as u64;
        Ok(Self {
            path: path.to_path_buf(),
            temporary,
            sink,
            size,
            fps,
            samples: Vec::new(),
            keys: Vec::new(),
            media: 0,
            start,
        })
    }

    pub fn sample(&mut self, bytes: &[u8], key: bool) -> Result<(), Error> {
        let length = u32::try_from(bytes.len()).map_err(|_| {
            Error::Encode("an encoded sample outgrew its four byte length".to_string())
        })?;
        write(&mut self.sink, &self.temporary, bytes)?;
        self.samples.push(length);
        if key {
            self.keys.push(self.samples.len() as u32);
        }
        self.media += u64::from(length);
        Ok(())
    }

    pub fn finish(mut self, sps: &[u8], pps: &[u8]) -> Result<(), Error> {
        let mdat = u32::try_from(self.media + 8)
            .map_err(|_| Error::Encode("the movie outgrew a four gigabyte file".to_string()))?;
        let temporary = self.temporary.clone();
        let fault = |error: std::io::Error| Error::Write {
            path: temporary.clone(),
            detail: error.to_string(),
        };
        self.sink
            .seek(SeekFrom::Start(self.start - 8))
            .map_err(fault)?;
        self.sink.write_all(&mdat.to_be_bytes()).map_err(fault)?;
        self.sink.write_all(b"mdat").map_err(fault)?;
        self.sink.seek(SeekFrom::End(0)).map_err(fault)?;
        let moov = self.moov(sps, pps);
        self.sink.write_all(&moov).map_err(fault)?;
        self.sink.flush().map_err(fault)?;
        std::fs::rename(&self.temporary, &self.path).map_err(|error| Error::Write {
            path: self.path.clone(),
            detail: error.to_string(),
        })
    }

    fn moov(&self, sps: &[u8], pps: &[u8]) -> Vec<u8> {
        let frames = self.samples.len() as u32;
        let duration = ((u64::from(frames) * u64::from(MOVIE_TIMESCALE) + u64::from(self.fps) / 2)
            / u64::from(self.fps)) as u32;
        let mvhd = mvhd(duration);
        let tkhd = tkhd(self.size, duration);
        let mdhd = mdhd(self.fps, frames);
        let hdlr = hdlr();
        let vmhd = vmhd();
        let dinf = dinf();
        let stbl = self.stbl(sps, pps);
        let minf = boxed(*b"minf", [vmhd, dinf, stbl].concat());
        let mdia = boxed(*b"mdia", [mdhd, hdlr, minf].concat());
        let trak = boxed(*b"trak", [tkhd, mdia].concat());
        boxed(*b"moov", [mvhd, trak].concat())
    }

    fn stbl(&self, sps: &[u8], pps: &[u8]) -> Vec<u8> {
        let entry = avc1(sps, pps, self.size);
        let mut stsd = Vec::new();
        push_u32(&mut stsd, 1);
        stsd.extend_from_slice(&entry);

        let mut stts = Vec::new();
        push_u32(&mut stts, 1);
        push_u32(&mut stts, self.samples.len() as u32);
        push_u32(&mut stts, 1);

        let mut stsc = Vec::new();
        push_u32(&mut stsc, 1);
        push_u32(&mut stsc, 1);
        push_u32(&mut stsc, self.samples.len() as u32);
        push_u32(&mut stsc, 1);

        let mut stsz = Vec::new();
        push_u32(&mut stsz, 0);
        push_u32(&mut stsz, self.samples.len() as u32);
        for sample in &self.samples {
            push_u32(&mut stsz, *sample);
        }

        let mut stco = Vec::new();
        push_u32(&mut stco, 1);
        push_u32(&mut stco, self.start as u32);

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
                full_box(*b"stco", 0, 0, stco),
                full_box(*b"stss", 0, 0, stss),
            ]
            .concat(),
        )
    }
}

impl Drop for Mp4 {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.temporary);
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

fn avc1(sps: &[u8], pps: &[u8], size: [u32; 2]) -> Vec<u8> {
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
    push_u16(&mut payload, SAMPLE_DEPTH);
    push_u16(&mut payload, 0xffff);
    payload.extend_from_slice(&avcc(sps, pps));
    boxed(*b"avc1", payload)
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

fn ftyp() -> Vec<u8> {
    let mut payload = Vec::new();
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

fn write(sink: &mut BufWriter<File>, path: &Path, bytes: &[u8]) -> Result<(), Error> {
    sink.write_all(bytes).map_err(|error| Error::Write {
        path: path.to_path_buf(),
        detail: error.to_string(),
    })
}

fn temporary_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("movie");
    path.with_file_name(format!(".{name}.{}.part", std::process::id()))
}
