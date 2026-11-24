use grana_volume::{Calibration, ChannelInfo};

use crate::Error;

use super::member::Member;

const INTERVAL_TOLERANCE: f64 = 1.0e-6;

pub(super) fn channels(path: &str, members: &[Member]) -> Result<Vec<ChannelInfo>, Error> {
    let reference = members
        .first()
        .expect("a scene holds at least one member")
        .image
        .pyramid();
    let count = reference.channel_count();
    for member in members {
        let pyramid = member.image.pyramid();
        if pyramid.channel_count() != count {
            return Err(Error::Unsupported {
                path: path.to_string(),
                detail: format!(
                    "the images of the scene hold {} and {} channels",
                    count,
                    pyramid.channel_count()
                ),
            });
        }
        for (index, channel) in pyramid.channels().iter().enumerate() {
            if channel.format() != reference.channels()[index].format() {
                return Err(Error::Unsupported {
                    path: path.to_string(),
                    detail: "the images of the scene disagree on the voxel data type".to_string(),
                });
            }
            if channel.no_data() != reference.channels()[index].no_data() {
                return Err(Error::Unsupported {
                    path: path.to_string(),
                    detail: "the images of the scene disagree on the fill value".to_string(),
                });
            }
        }
    }
    Ok((0..count)
        .map(|index| {
            let channel = reference.channels()[index].clone();
            let mut statistics = channel.statistics();
            for member in members.iter().skip(1) {
                statistics =
                    statistics.folded(member.image.pyramid().channels()[index].statistics());
            }
            channel.with_statistics(statistics)
        })
        .collect())
}

pub(super) fn frames(path: &str, members: &[Member]) -> Result<u32, Error> {
    let frames = members
        .iter()
        .map(|member| member.frames)
        .max()
        .expect("a scene holds at least one member");
    for member in members {
        if member.frames != 1 && member.frames != frames {
            return Err(Error::Unsupported {
                path: path.to_string(),
                detail: format!(
                    "the images of the scene hold {} and {frames} frames",
                    member.frames
                ),
            });
        }
    }
    Ok(frames)
}

pub(super) fn interval(path: &str, members: &[Member]) -> Result<f32, Error> {
    let mut interval = None;
    for member in members.iter().filter(|member| member.frames > 1) {
        let value = member.image.pyramid().frame_interval();
        match interval {
            None => interval = Some(value),
            Some(held) => {
                let scale = f64::from(held.abs().max(value.abs()).max(1.0e-6));
                if (f64::from(held) - f64::from(value)).abs() > INTERVAL_TOLERANCE * scale {
                    return Err(Error::Unsupported {
                        path: path.to_string(),
                        detail: "the images of the scene disagree on the frame interval"
                            .to_string(),
                    });
                }
            }
        }
    }
    Ok(interval.unwrap_or_else(|| {
        members
            .first()
            .expect("a scene holds at least one member")
            .image
            .pyramid()
            .frame_interval()
    }))
}

pub(super) fn calibration(
    path: &str,
    members: &[Member],
    reference: usize,
) -> Result<Calibration, Error> {
    let chosen = members[reference].image.pyramid().calibration();
    for member in members {
        if member.image.pyramid().calibration().measured() != chosen.measured() {
            return Err(Error::Unsupported {
                path: path.to_string(),
                detail: "the scene mixes calibrated and uncalibrated images".to_string(),
            });
        }
    }
    Ok(chosen)
}
