use std::sync::atomic::{AtomicBool, Ordering};

use grana_scene::Registration;
use grana_warp::Warp;

use crate::event::Event;

pub struct FiducialChannel {
    channel: usize,
    points: Vec<[f32; 3]>,
}

impl FiducialChannel {
    pub fn new(channel: usize, points: Vec<[f32; 3]>) -> Self {
        assert!(
            points
                .iter()
                .all(|point| point.iter().all(|value| value.is_finite())),
            "a fiducial channel holds finite bead positions"
        );
        Self { channel, points }
    }

    pub fn channel(&self) -> usize {
        self.channel
    }

    pub fn points(&self) -> &[[f32; 3]] {
        &self.points
    }
}

pub struct FiducialRequest {
    reference: FiducialChannel,
    moving: Vec<FiducialChannel>,
    channels: usize,
    scale: f32,
}

impl FiducialRequest {
    pub fn new(
        reference: FiducialChannel,
        moving: Vec<FiducialChannel>,
        channels: usize,
        scale: f32,
    ) -> Self {
        assert!(
            channels > 1 && reference.channel < channels,
            "a fiducial registration aligns stored channels"
        );
        assert!(
            !moving.is_empty(),
            "a fiducial registration aligns at least one moving channel"
        );
        assert!(
            moving
                .iter()
                .all(|channel| channel.channel < channels && channel.channel != reference.channel),
            "a fiducial registration moves another stored channel than its reference"
        );
        assert!(
            moving
                .iter()
                .enumerate()
                .all(|(index, channel)| moving[..index]
                    .iter()
                    .all(|other| other.channel != channel.channel)),
            "a fiducial registration moves each channel once"
        );
        assert!(
            scale.is_finite() && scale > 0.0,
            "a fiducial registration knows the voxel size of its dataset"
        );
        Self {
            reference,
            moving,
            channels,
            scale,
        }
    }

    pub fn reference(&self) -> &FiducialChannel {
        &self.reference
    }

    pub fn moving(&self) -> &[FiducialChannel] {
        &self.moving
    }
}

pub fn fiducial(
    request: FiducialRequest,
    cancel: &AtomicBool,
    sink: &mut dyn FnMut(Event),
) -> Result<(), String> {
    let FiducialRequest {
        reference,
        moving,
        channels,
        scale,
    } = request;
    let mut warps = vec![Warp::IDENTITY; channels];
    let mut pairs = 0usize;
    let mut residual = 0.0f32;
    let total = moving.len();
    sink(Event::Progress {
        time: 0,
        done: 0,
        total,
    });
    for (index, channel) in moving.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        if channel.points.is_empty() {
            return Err(format!(
                "the channel {} carries no bead to register",
                channel.channel
            ));
        }
        let fiducial =
            grana_analysis::fiducial_registration(&reference.points, &channel.points, scale)?;
        warps[channel.channel] = fiducial.warp().clone();
        pairs += fiducial.pairs();
        residual = residual.max(fiducial.residual());
        sink(Event::Progress {
            time: 0,
            done: index + 1,
            total,
        });
    }
    if cancel.load(Ordering::Relaxed) {
        return Ok(());
    }
    sink(Event::Fiducial {
        channels: total,
        pairs,
        residual,
    });
    sink(Event::Registration(Registration::new(
        reference.channel,
        warps,
    )));
    sink(Event::Done);
    Ok(())
}
