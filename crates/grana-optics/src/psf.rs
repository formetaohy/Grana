use grana_volume::Spacing;

use crate::bessel;

const GENEROUS_HALF: [u32; 3] = [9, 9, 6];
const HALF_LIMIT: [u32; 3] = [6, 6, 4];
const QUADRATURE: usize = 512;
const ENCLOSED_ENERGY: f64 = 0.99;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PsfParams {
    numerical_aperture: f32,
    wavelength: f32,
    immersion: f32,
    sample: f32,
    depth: f32,
}

impl PsfParams {
    pub fn new(
        numerical_aperture: f32,
        wavelength: f32,
        immersion: f32,
        sample: f32,
        depth: f32,
    ) -> Self {
        assert!(
            numerical_aperture.is_finite() && numerical_aperture > 0.0 && numerical_aperture <= 1.6,
            "a numerical aperture lies between zero and the widest dry objective"
        );
        assert!(
            wavelength.is_finite() && (0.2..=2.0).contains(&wavelength),
            "an emission wavelength is given in microns"
        );
        for (name, value) in [("immersion", immersion), ("sample", sample)] {
            assert!(
                value.is_finite() && (1.0..=2.0).contains(&value),
                "the {name} refractive index lies above vacuum and below diamond"
            );
        }
        assert!(
            depth.is_finite() && (0.0..=1.0e4).contains(&depth),
            "the emitter depth is given in microns"
        );
        Self {
            numerical_aperture,
            wavelength,
            immersion,
            sample,
            depth,
        }
    }

    pub fn numerical_aperture(self) -> f32 {
        self.numerical_aperture
    }

    pub fn wavelength(self) -> f32 {
        self.wavelength
    }

    pub fn immersion(self) -> f32 {
        self.immersion
    }

    pub fn sample(self) -> f32 {
        self.sample
    }

    pub fn depth(self) -> f32 {
        self.depth
    }

    pub fn with_depth(self, depth: f32) -> Self {
        Self::new(
            self.numerical_aperture,
            self.wavelength,
            self.immersion,
            self.sample,
            depth,
        )
    }

    fn pupil(self) -> f64 {
        let aperture = f64::from(self.numerical_aperture);
        let immersion = f64::from(self.immersion) / aperture;
        let sample = f64::from(self.sample) / aperture;
        immersion.min(sample).min(1.0)
    }

    fn intensity_at(self, pupil: &Pupil, radius: f64, height: f64) -> f64 {
        let aperture = f64::from(self.numerical_aperture);
        let immersion = f64::from(self.immersion);
        let sample = f64::from(self.sample);
        let depth = f64::from(self.depth);
        let mut real = 0.0;
        let mut imaginary = 0.0;
        for index in 0..=QUADRATURE {
            let rho = index as f64 * pupil.step;
            let weight = simpson(index) * pupil.step;
            let transverse = pupil.wave * aperture * radius * rho;
            let axial = immersion * immersion - (aperture * rho).powi(2);
            let specimen = sample * sample - (aperture * rho).powi(2);
            let phase = pupil.wave
                * (depth * (specimen.max(0.0).sqrt() - sample)
                    + height * (axial.max(0.0).sqrt() - immersion));
            let amplitude = weight * rho * bessel::j0(transverse);
            real += amplitude * phase.cos();
            imaginary += amplitude * phase.sin();
        }
        real * real + imaginary * imaginary
    }
}

struct Pupil {
    wave: f64,
    step: f64,
}

impl Pupil {
    fn of(params: PsfParams) -> Self {
        let limit = params.pupil();
        Self {
            wave: 2.0 * std::f64::consts::PI / f64::from(params.wavelength),
            step: limit / QUADRATURE as f64,
        }
    }
}

fn simpson(index: usize) -> f64 {
    if index == 0 || index == QUADRATURE {
        1.0
    } else if index.is_multiple_of(2) {
        2.0
    } else {
        4.0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Kernel {
    dims: [u32; 3],
    values: Vec<f32>,
}

impl Kernel {
    pub fn new(params: PsfParams, spacing: Spacing) -> Self {
        let sampled = samples(params, spacing, GENEROUS_HALF);
        let (radius, height) = enclosure(sampled.dims, &sampled.values, spacing);
        let half = [
            clamp_half(radius / spacing.x(), HALF_LIMIT[0]),
            clamp_half(radius / spacing.y(), HALF_LIMIT[1]),
            clamp_half(height / spacing.z(), HALF_LIMIT[2]),
        ];
        crop(&sampled, half)
    }

    pub fn dims(&self) -> [u32; 3] {
        self.dims
    }

    pub fn values(&self) -> &[f32] {
        &self.values
    }

    pub fn voxels(&self) -> usize {
        self.values.len()
    }
}

struct Sampled {
    dims: [u32; 3],
    values: Vec<f32>,
}

fn samples(params: PsfParams, spacing: Spacing, half: [u32; 3]) -> Sampled {
    let dims = [2 * half[0] + 1, 2 * half[1] + 1, 2 * half[2] + 1];
    let mut values = vec![0.0f32; (dims[0] * dims[1] * dims[2]) as usize];
    let pupil = Pupil::of(params);
    for z in 0..dims[2] {
        let height = f64::from(z as f32 - half[2] as f32) * f64::from(spacing.z());
        let mut known: Vec<(f64, f64)> = Vec::new();
        for y in 0..dims[1] {
            let offset = f64::from(y as f32 - half[1] as f32) * f64::from(spacing.y());
            for x in 0..dims[0] {
                let lateral = f64::from(x as f32 - half[0] as f32) * f64::from(spacing.x());
                let radius = lateral.hypot(offset);
                let intensity = match known.iter().find(|(seen, _)| *seen == radius) {
                    Some((_, value)) => *value,
                    None => {
                        let value = params.intensity_at(&pupil, radius, height);
                        known.push((radius, value));
                        value
                    }
                };
                values[(x + dims[0] * (y + dims[1] * z)) as usize] = intensity as f32;
            }
        }
    }
    Sampled { dims, values }
}

fn enclosure(dims: [u32; 3], values: &[f32], spacing: Spacing) -> (f32, f32) {
    let [width, height, depth] = dims;
    let mut lateral = Vec::with_capacity(values.len());
    let mut axial = Vec::with_capacity(depth as usize);
    let mut total = 0.0f64;
    for z in 0..depth {
        let mut slice = 0.0f64;
        for y in 0..height {
            for x in 0..width {
                let value = f64::from(values[(x + width * (y + height * z)) as usize]);
                let dx = (x as f32 - width as f32 / 2.0) * spacing.x();
                let dy = (y as f32 - height as f32 / 2.0) * spacing.y();
                lateral.push((f64::from(dx.abs().max(dy.abs())), value));
                slice += value;
            }
        }
        let dz = (z as f32 - depth as f32 / 2.0) * spacing.z();
        axial.push((f64::from(dz.abs()), slice));
        total += slice;
    }
    (quantile(lateral, total), quantile(axial, total))
}

fn quantile(mut bins: Vec<(f64, f64)>, total: f64) -> f32 {
    if total <= 0.0 {
        return 0.0;
    }
    bins.sort_by(|left, right| left.0.total_cmp(&right.0));
    let mut enclosed = 0.0;
    for (distance, weight) in bins {
        enclosed += weight;
        if enclosed >= total * ENCLOSED_ENERGY {
            return distance as f32;
        }
    }
    0.0
}

fn clamp_half(half: f32, limit: u32) -> u32 {
    assert!(half.is_finite(), "a kernel half extent is finite");
    half.ceil().max(1.0).min(limit as f32) as u32
}

fn crop(sampled: &Sampled, half: [u32; 3]) -> Kernel {
    let source = sampled.dims;
    let dims = [2 * half[0] + 1, 2 * half[1] + 1, 2 * half[2] + 1];
    let center = [source[0] / 2, source[1] / 2, source[2] / 2];
    let mut values = vec![0.0f32; (dims[0] * dims[1] * dims[2]) as usize];
    let mut total = 0.0f32;
    for z in 0..dims[2] {
        for y in 0..dims[1] {
            for x in 0..dims[0] {
                let source_z = center[2] + z - half[2];
                let source_y = center[1] + y - half[1];
                let source_x = center[0] + x - half[0];
                let value = sampled.values
                    [(source_x + source[0] * (source_y + source[1] * source_z)) as usize];
                values[(x + dims[0] * (y + dims[1] * z)) as usize] = value;
                total += value;
            }
        }
    }
    assert!(
        total > 0.0 && total.is_finite(),
        "a point spread function carries energy"
    );
    for value in &mut values {
        *value /= total;
    }
    Kernel { dims, values }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bessel;

    #[test]
    fn an_aberration_free_psf_follows_the_airy_pattern() {
        let params = PsfParams::new(1.4, 0.52, 1.515, 1.515, 0.0);
        let spacing = Spacing::new(0.04, 0.04, 0.1);
        let kernel = Kernel::new(params, spacing);
        let [width, height, depth] = kernel.dims();
        let plane = depth / 2;
        let row = height / 2;
        let centre = kernel.values()[(width / 2 + width * (row + height * plane)) as usize];
        assert!(centre > 0.0, "the focus carries the brightest voxel");
        let wave = 2.0 * std::f64::consts::PI / f64::from(params.wavelength());
        for offset in 1..=3u32 {
            let radius = f64::from(offset as f32 * spacing.x());
            let argument = wave * f64::from(params.numerical_aperture()) * radius;
            let airy = (2.0 * bessel::j1(argument) / argument).powi(2);
            let sampled =
                kernel.values()[(offset + width / 2 + width * (row + height * plane)) as usize];
            let relative = f64::from(sampled / centre);
            assert!(
                (relative - airy).abs() < 0.02,
                "offset {offset}: {relative} against {airy}"
            );
        }
    }
}
