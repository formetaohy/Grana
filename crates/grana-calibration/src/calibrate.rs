use grana_image::Image;
use grana_optics::{Kernel, MeasuredPsf};

use crate::detect;
use crate::error::{Error, calibration};
use crate::extract;
use crate::settings::Settings;

const TRIM_ENERGY: f32 = 0.99;

pub struct Calibration {
    psf: MeasuredPsf,
    beads: usize,
    bands: Vec<usize>,
}

impl Calibration {
    pub fn psf(&self) -> &MeasuredPsf {
        &self.psf
    }

    pub fn beads(&self) -> usize {
        self.beads
    }

    pub fn bands(&self) -> &[usize] {
        &self.bands
    }
}

pub fn calibrate(image: &Image, settings: Settings) -> Result<Calibration, Error> {
    let pyramid = image.pyramid();
    if settings.channel() >= pyramid.channel_count() {
        return Err(calibration("the calibrated channel does not exist"));
    }
    if settings.time() >= pyramid.frame_count() {
        return Err(calibration("the calibrated frame does not exist"));
    }
    let level = detect::detection_level(image, settings.detection_voxels());
    let info = pyramid.level(level);
    let extent = info.extent();
    let dims = [extent.x(), extent.y(), extent.z()];
    let volume = detect::read_level(image, settings.time(), settings.channel(), level)?;
    let beads = detect::beads(
        &volume,
        dims,
        info,
        settings.threshold(),
        settings.separation(),
        settings.bead_limit(),
    )?;
    let mut patches = Vec::new();
    let mut depths = Vec::new();
    for bead in &beads {
        let Some(patch) = extract::patch(image, &settings, bead)? else {
            continue;
        };
        depths.push(info.voxel_of(bead.world)[2] / dims[2] as f32);
        patches.push(patch);
    }
    if patches.is_empty() {
        return Err(calibration(
            "every detected bead lies too close to the calibration volume border",
        ));
    }
    let finest = pyramid.level(0);
    let spacing = finest.spacing();
    let bands = settings.bands().min(patches.len() as u32).max(1);
    let mut groups = vec![Vec::new(); bands as usize];
    for (index, depth) in depths.iter().enumerate() {
        let position = depth.clamp(0.0, 1.0);
        let band = ((position * bands as f32) as usize).min(bands as usize - 1);
        groups[band].push(index);
    }
    let counts = groups.iter().map(Vec::len).collect::<Vec<_>>();
    let dims = patches[0].dims();
    let mut kernels = Vec::with_capacity(groups.len());
    for (band, group) in groups.iter().enumerate() {
        let members = if group.is_empty() {
            nearest(&groups, band)
        } else {
            group
        };
        let mut values = vec![0.0f32; (dims[0] * dims[1] * dims[2]) as usize];
        for index in members {
            let source = patches[*index].values();
            for (slot, value) in values.iter_mut().zip(source) {
                *slot += *value;
            }
        }
        kernels.push(Kernel::from_values(dims, values).trimmed(TRIM_ENERGY));
    }
    let dims = kernels.iter().fold([1u32; 3], |dims, kernel| {
        [0, 1, 2].map(|axis| dims[axis].max(kernel.dims()[axis]))
    });
    let kernels = kernels
        .into_iter()
        .map(|kernel| kernel.pad(dims))
        .collect::<Vec<_>>();
    Ok(Calibration {
        psf: MeasuredPsf::new(spacing, kernels),
        beads: patches.len(),
        bands: counts,
    })
}

fn nearest(groups: &[Vec<usize>], band: usize) -> &Vec<usize> {
    let mut distance = 1;
    while distance < groups.len() {
        if let Some(above) = groups
            .get(band + distance)
            .filter(|group| !group.is_empty())
        {
            return above;
        }
        if let Some(below) = band
            .checked_sub(distance)
            .and_then(|index| groups.get(index))
            .filter(|group| !group.is_empty())
        {
            return below;
        }
        distance += 1;
    }
    panic!("a calibrated point spread function holds a non empty depth band")
}
