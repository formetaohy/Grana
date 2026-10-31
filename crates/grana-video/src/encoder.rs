use openh264::OpenH264API;
use openh264::encoder::{
    BitRate, Complexity, Encoder as OpenH264Encoder, EncoderConfig, FrameRate, FrameType,
    IntraFramePeriod, Profile, RateControlMode, SpsPpsStrategy, VuiConfig,
};
use openh264::formats::YUVSlices;

use crate::error::Error;
use crate::yuv::{self, Planes};

const BITS_PER_PIXEL: u64 = 4;
const MIN_BITRATE: u64 = 4_000_000;
const MAX_BITRATE: u64 = 80_000_000;
const KEYFRAME_SECONDS: u32 = 2;
const NAL_SPS: u8 = 7;
const NAL_PPS: u8 = 8;

pub(crate) struct Sample {
    pub bytes: Vec<u8>,
    pub key: bool,
}

pub(crate) struct Encoder {
    inner: OpenH264Encoder,
    size: [u32; 2],
    luma: Vec<u8>,
    chroma_u: Vec<u8>,
    chroma_v: Vec<u8>,
    sps: Option<Vec<u8>>,
    pps: Option<Vec<u8>>,
}

impl Encoder {
    pub fn new(size: [u32; 2], fps: u32) -> Result<Self, Error> {
        if size[0] == 0 || size[1] == 0 || !size[0].is_multiple_of(2) || !size[1].is_multiple_of(2)
        {
            return Err(Error::Size { size });
        }
        let pixels = u64::from(size[0]) * u64::from(size[1]);
        let bitrate =
            (pixels * u64::from(fps) / BITS_PER_PIXEL).clamp(MIN_BITRATE, MAX_BITRATE) as u32;
        let config = EncoderConfig::new()
            .bitrate(BitRate::from_bps(bitrate))
            .max_frame_rate(FrameRate::from_hz(fps as f32))
            .rate_control_mode(RateControlMode::Bitrate)
            .skip_frames(false)
            .sps_pps_strategy(SpsPpsStrategy::ConstantId)
            .profile(Profile::High)
            .complexity(Complexity::High)
            .intra_frame_period(IntraFramePeriod::from_num_frames(fps * KEYFRAME_SECONDS))
            .vui(VuiConfig::bt709())
            .num_threads(0);
        let inner = OpenH264Encoder::with_api_config(OpenH264API::from_source(), config)
            .map_err(|error| Error::Encode(error.to_string()))?;
        let width = size[0] as usize;
        let height = size[1] as usize;
        Ok(Self {
            inner,
            size,
            luma: vec![0; width * height],
            chroma_u: vec![0; width * height / 4],
            chroma_v: vec![0; width * height / 4],
            sps: None,
            pps: None,
        })
    }

    pub fn encode(&mut self, rgba: &[u8]) -> Result<Sample, Error> {
        yuv::from_rgba(
            rgba,
            self.size,
            Planes {
                y: &mut self.luma,
                u: &mut self.chroma_u,
                v: &mut self.chroma_v,
            },
        );
        let width = self.size[0] as usize;
        let height = self.size[1] as usize;
        let source = YUVSlices::new(
            (&self.luma, &self.chroma_u, &self.chroma_v),
            (width, height),
            (width, width / 2, width / 2),
        );
        let stream = self
            .inner
            .encode(&source)
            .map_err(|error| Error::Encode(error.to_string()))?;
        let key = match stream.frame_type() {
            FrameType::IDR => true,
            FrameType::P => false,
            other => {
                return Err(Error::Encode(format!(
                    "the encoder produced a {other:?} frame"
                )));
            }
        };
        let mut bytes = Vec::new();
        for index in 0..stream.num_layers() {
            let Some(layer) = stream.layer(index) else {
                continue;
            };
            for unit in 0..layer.nal_count() {
                let Some(nal) = layer.nal_unit(unit).and_then(payload) else {
                    continue;
                };
                match nal[0] & 0x1f {
                    NAL_SPS => remember(&mut self.sps, nal)?,
                    NAL_PPS => remember(&mut self.pps, nal)?,
                    _ => {
                        let length = u32::try_from(nal.len()).map_err(|_| {
                            Error::Encode("an encoded slice outgrew its length prefix".to_string())
                        })?;
                        bytes.extend_from_slice(&length.to_be_bytes());
                        bytes.extend_from_slice(nal);
                    }
                }
            }
        }
        if bytes.is_empty() {
            return Err(Error::Encode(
                "the encoder produced no slice for a frame".to_string(),
            ));
        }
        Ok(Sample { bytes, key })
    }

    pub fn parameter_sets(&self) -> Result<(&[u8], &[u8]), Error> {
        match (&self.sps, &self.pps) {
            (Some(sps), Some(pps)) => Ok((sps, pps)),
            _ => Err(Error::Encode(
                "the encoder produced no parameter sets".to_string(),
            )),
        }
    }
}

fn remember(set: &mut Option<Vec<u8>>, nal: &[u8]) -> Result<(), Error> {
    match set {
        Some(existing) if existing != nal => Err(Error::Encode(
            "the encoder revised its parameter sets".to_string(),
        )),
        Some(_) => Ok(()),
        None => {
            *set = Some(nal.to_vec());
            Ok(())
        }
    }
}

fn payload(nal: &[u8]) -> Option<&[u8]> {
    let start = nal.iter().position(|byte| *byte != 0)?;
    (nal[start] == 1).then(|| &nal[start + 1..])
}
