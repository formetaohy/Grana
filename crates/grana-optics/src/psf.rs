use grana_volume::Spacing;

use crate::bessel;

const DEPTH_STEP: f32 = 4.0;
const KERNEL_LIMIT: u32 = 16;
const LAYER_LIMIT: u32 = 16;
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

    pub fn from_values(dims: [u32; 3], values: Vec<f32>) -> Self {
        assert!(
            dims.iter().all(|extent| *extent % 2 == 1),
            "a point spread function carries an odd extent on every axis"
        );
        assert_eq!(
            values.len(),
            (dims[0] * dims[1] * dims[2]) as usize,
            "a point spread function holds every voxel of its extent"
        );
        let total: f32 = values.iter().sum();
        assert!(
            values
                .iter()
                .all(|value| value.is_finite() && *value >= 0.0)
                && total.is_finite()
                && total > 0.0,
            "a point spread function carries finite non negative energy"
        );
        let values = values.into_iter().map(|value| value / total).collect();
        Self { dims, values }
    }

    pub fn resample(&self, from: Spacing, to: Spacing) -> Self {
        let source = self.dims;
        let half = [
            resampled_half(source[0], from.x(), to.x()),
            resampled_half(source[1], from.y(), to.y()),
            resampled_half(source[2], from.z(), to.z()),
        ];
        let dims = [2 * half[0] + 1, 2 * half[1] + 1, 2 * half[2] + 1];
        let centre = [
            (source[0] / 2) as f32,
            (source[1] / 2) as f32,
            (source[2] / 2) as f32,
        ];
        let scale = [to.x() / from.x(), to.y() / from.y(), to.z() / from.z()];
        let mut values = vec![0.0f32; (dims[0] * dims[1] * dims[2]) as usize];
        for z in 0..dims[2] {
            for y in 0..dims[1] {
                for x in 0..dims[0] {
                    let position = [x, y, z];
                    let at = [0, 1, 2].map(|axis| {
                        centre[axis] + (position[axis] as f32 - half[axis] as f32) * scale[axis]
                    });
                    let value = self.sample(at);
                    values[(x + dims[0] * (y + dims[1] * z)) as usize] = value;
                }
            }
        }
        Self::from_values(dims, values)
    }

    pub fn trimmed(&self, energy: f32) -> Self {
        assert!(
            energy.is_finite() && energy > 0.0 && energy <= 1.0,
            "a trimmed point spread function keeps a fraction of its energy"
        );
        let total: f32 = self.values.iter().sum();
        let half = [0, 1, 2].map(|axis| {
            let centre = self.dims[axis] / 2;
            let mut mass = 0.0f32;
            let mut half = 0;
            loop {
                for z in 0..self.dims[2] {
                    for y in 0..self.dims[1] {
                        for x in 0..self.dims[0] {
                            let offset = [x, y, z][axis].abs_diff(centre);
                            if offset == half {
                                mass += self.values
                                    [(x + self.dims[0] * (y + self.dims[1] * z)) as usize];
                            }
                        }
                    }
                }
                if mass >= total * energy || half >= centre {
                    break;
                }
                half += 1;
            }
            half.max(1)
        });
        let dims = [2 * half[0] + 1, 2 * half[1] + 1, 2 * half[2] + 1];
        let source = self.dims;
        let mut values = vec![0.0f32; (dims[0] * dims[1] * dims[2]) as usize];
        for z in 0..dims[2] {
            for y in 0..dims[1] {
                for x in 0..dims[0] {
                    let from = x + source[0] / 2 - half[0]
                        + source[0] * (y + source[1] / 2 - half[1])
                        + source[0] * source[1] * (z + source[2] / 2 - half[2]);
                    values[(x + dims[0] * (y + dims[1] * z)) as usize] = self.values[from as usize];
                }
            }
        }
        Self::from_values(dims, values)
    }

    pub fn pad(&self, dims: [u32; 3]) -> Self {
        assert!(
            (0..3).all(|axis| dims[axis] >= self.dims[axis] && dims[axis] % 2 == 1),
            "a padded point spread function grows to an odd extent"
        );
        Self::from_values(dims, self.padded(dims))
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

    fn sample(&self, at: [f32; 3]) -> f32 {
        let source = self.dims;
        let low = [0, 1, 2].map(|axis| at[axis].floor() as i64);
        let weight = [0, 1, 2].map(|axis| at[axis] - at[axis].floor());
        let mut total = 0.0f32;
        for z in 0..2 {
            for y in 0..2 {
                for x in 0..2 {
                    let corner = [low[0] + x, low[1] + y, low[2] + z];
                    if corner
                        .iter()
                        .enumerate()
                        .any(|(axis, value)| *value < 0 || *value >= i64::from(source[axis]))
                    {
                        continue;
                    }
                    let corner_weight = [
                        if x == 0 { 1.0 - weight[0] } else { weight[0] },
                        if y == 0 { 1.0 - weight[1] } else { weight[1] },
                        if z == 0 { 1.0 - weight[2] } else { weight[2] },
                    ];
                    let index = (corner[0]
                        + i64::from(source[0]) * (corner[1] + i64::from(source[1]) * corner[2]))
                        as usize;
                    total += self.values[index] * corner_weight.iter().product::<f32>();
                }
            }
        }
        total
    }

    fn padded(&self, dims: [u32; 3]) -> Vec<f32> {
        let source = self.dims;
        let offset = [0, 1, 2].map(|axis| (dims[axis] - source[axis]) / 2);
        let mut values = vec![0.0f32; (dims[0] * dims[1] * dims[2]) as usize];
        for z in 0..source[2] {
            for y in 0..source[1] {
                for x in 0..source[0] {
                    let from = (x + source[0] * (y + source[1] * z)) as usize;
                    let to = (x + offset[0] + dims[0] * (y + offset[1] + dims[1] * (z + offset[2])))
                        as usize;
                    values[to] = self.values[from];
                }
            }
        }
        values
    }
}

fn resampled_half(extent: u32, from: f32, to: f32) -> u32 {
    let half = extent / 2;
    let cells = (half as f32 * from / to).ceil().max(1.0);
    (cells as u32).min(KERNEL_LIMIT)
}

#[derive(Clone, Debug, PartialEq)]
pub struct KernelBank {
    dims: [u32; 3],
    values: Vec<f32>,
    band: u32,
    layers: u32,
}

impl KernelBank {
    pub fn new(params: PsfParams, spacing: Spacing, planes: u32) -> Self {
        assert!(planes > 0, "a kernel bank covers at least one plane");
        let span = (planes - 1) as f32 * spacing.z();
        let layers = (span / DEPTH_STEP).ceil().max(1.0) as u32;
        let layers = layers.min(LAYER_LIMIT).min(planes);
        let band = planes.div_ceil(layers);
        let middle = 0.5 * (planes - 1) as f32;
        let kernels = (0..layers)
            .map(|layer| {
                let start = layer * band;
                let end = ((layer + 1) * band).min(planes);
                let centre = 0.5 * (start + end - 1) as f32;
                let depth = if layers == 1 {
                    params.depth()
                } else {
                    (params.depth() + (centre - middle) * spacing.z()).max(0.0)
                };
                Kernel::new(params.with_depth(depth), spacing)
            })
            .collect::<Vec<_>>();
        let dims = kernels.iter().fold([1u32; 3], |dims, kernel| {
            [0, 1, 2].map(|axis| dims[axis].max(kernel.dims()[axis]))
        });
        let mut values =
            Vec::with_capacity((dims[0] * dims[1] * dims[2]) as usize * layers as usize);
        for kernel in &kernels {
            values.extend(kernel.padded(dims));
        }
        Self {
            dims,
            values,
            band,
            layers,
        }
    }

    pub fn from_kernels(kernels: &[Kernel], planes: u32) -> Self {
        assert!(
            !kernels.is_empty() && kernels.len() <= LAYER_LIMIT as usize,
            "a kernel bank holds one to {LAYER_LIMIT} point spread functions"
        );
        assert!(planes > 0, "a kernel bank covers at least one plane");
        let dims = kernels.iter().fold([1u32; 3], |dims, kernel| {
            [0, 1, 2].map(|axis| dims[axis].max(kernel.dims()[axis]))
        });
        let layers = kernels.len() as u32;
        let band = planes.div_ceil(layers).max(1);
        let mut values = Vec::with_capacity((dims[0] * dims[1] * dims[2]) as usize * kernels.len());
        for kernel in kernels {
            values.extend(kernel.padded(dims));
        }
        Self {
            dims,
            values,
            band,
            layers,
        }
    }

    pub fn dims(&self) -> [u32; 3] {
        self.dims
    }

    pub fn layers(&self) -> u32 {
        self.layers
    }

    pub fn band(&self) -> u32 {
        self.band
    }

    pub fn voxels(&self) -> usize {
        (self.dims[0] * self.dims[1] * self.dims[2]) as usize
    }

    pub fn taps(&self, layer: u32) -> &[f32] {
        assert!(layer < self.layers, "a kernel bank names one of its layers");
        let voxels = self.voxels();
        &self.values[layer as usize * voxels..(layer as usize + 1) * voxels]
    }

    pub fn values(&self) -> &[f32] {
        &self.values
    }

    pub fn layer(&self, first_plane: u32, plane: u32) -> u32 {
        ((first_plane + plane) / self.band).min(self.layers - 1)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MeasuredPsf {
    spacing: Spacing,
    kernels: Vec<Kernel>,
}

impl MeasuredPsf {
    pub fn new(spacing: Spacing, kernels: Vec<Kernel>) -> Self {
        assert!(
            !kernels.is_empty() && kernels.len() <= LAYER_LIMIT as usize,
            "a measured point spread function holds one to {LAYER_LIMIT} depth bands"
        );
        let dims = kernels[0].dims();
        assert!(
            kernels.iter().all(|kernel| kernel.dims() == dims),
            "the depth bands of a measured point spread function share one extent"
        );
        Self { spacing, kernels }
    }

    pub fn spacing(&self) -> Spacing {
        self.spacing
    }

    pub fn kernels(&self) -> &[Kernel] {
        &self.kernels
    }

    pub fn layers(&self) -> u32 {
        self.kernels.len() as u32
    }

    pub fn dims(&self) -> [u32; 3] {
        self.kernels[0].dims()
    }

    pub fn bank(&self, spacing: Spacing, planes: u32) -> KernelBank {
        let kernels = if spacing == self.spacing {
            self.kernels.clone()
        } else {
            self.kernels
                .iter()
                .map(|kernel| kernel.resample(self.spacing, spacing))
                .collect::<Vec<_>>()
        };
        KernelBank::from_kernels(&kernels, planes)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Psf {
    Theoretical(PsfParams),
    Measured(MeasuredPsf),
}

impl Psf {
    pub fn theoretical(params: PsfParams) -> Self {
        Self::Theoretical(params)
    }

    pub fn measured(psf: MeasuredPsf) -> Self {
        Self::Measured(psf)
    }

    pub fn params(&self) -> Option<PsfParams> {
        match self {
            Self::Theoretical(params) => Some(*params),
            Self::Measured(_) => None,
        }
    }

    pub fn measured_span(&self) -> Option<(usize, [u32; 3])> {
        match self {
            Self::Theoretical(_) => None,
            Self::Measured(psf) => Some((psf.kernels().len(), psf.dims())),
        }
    }

    pub fn bank(&self, spacing: Spacing, planes: u32) -> KernelBank {
        match self {
            Self::Theoretical(params) => KernelBank::new(*params, spacing, planes),
            Self::Measured(psf) => psf.bank(spacing, planes),
        }
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

    fn spread(taps: &[f32], dims: [u32; 3], axis: usize) -> f32 {
        let centre = dims[axis] / 2;
        let mut total = 0.0f32;
        let mut moment = 0.0f32;
        for z in 0..dims[2] {
            for y in 0..dims[1] {
                for x in 0..dims[0] {
                    let position = [x, y, z][axis];
                    let offset = position as f32 - centre as f32;
                    let value = taps[(x + dims[0] * (y + dims[1] * z)) as usize];
                    total += value;
                    moment += value * offset * offset;
                }
            }
        }
        (moment / total).sqrt()
    }

    #[test]
    fn a_kernel_bank_samples_the_depth_it_covers() {
        let params = PsfParams::new(1.4, 0.52, 1.515, 1.515, 0.0);
        let spacing = Spacing::new(0.04, 0.04, 0.2);
        let bank = KernelBank::new(params, spacing, 101);
        assert_eq!(bank.layers(), 5, "a 20 µm stack samples its depth");
        assert_eq!(bank.band(), 21, "the layers tile every plane");
        for layer in 0..bank.layers() {
            let total = bank.taps(layer).iter().sum::<f32>();
            assert!(
                (total - 1.0).abs() < 1.0e-4,
                "layer {layer} carries {total} of energy"
            );
        }
        let near = spread(bank.taps(0), bank.dims(), 2);
        let far = spread(bank.taps(bank.layers() - 1), bank.dims(), 2);
        assert!(
            far > near * 1.2,
            "the deepest layer is axially wider: {far} against {near}"
        );
    }

    #[test]
    fn a_single_layer_bank_is_the_kernel_it_holds() {
        let params = PsfParams::new(1.4, 0.52, 1.515, 1.515, 0.4);
        let spacing = Spacing::new(0.04, 0.04, 0.2);
        let bank = KernelBank::new(params, spacing, 6);
        let kernel = Kernel::new(params, spacing);
        assert_eq!(
            bank.layers(),
            1,
            "a thin stack keeps one point spread function"
        );
        assert_eq!(bank.dims(), kernel.dims());
        assert_eq!(bank.taps(0), kernel.values());
        assert_eq!(bank.layer(0, 5), 0);
    }

    fn point(dims: [u32; 3]) -> Kernel {
        let mut values = vec![0.0f32; (dims[0] * dims[1] * dims[2]) as usize];
        let centre = (dims[0] / 2 + dims[0] * (dims[1] / 2 + dims[1] * (dims[2] / 2))) as usize;
        values[centre] = 1.0;
        Kernel::from_values(dims, values)
    }

    #[test]
    fn a_measured_point_spread_function_resamples_and_bands() {
        let spacing = Spacing::new(0.1, 0.1, 0.3);
        let psf = MeasuredPsf::new(spacing, vec![point([5, 5, 3]), point([5, 5, 3])]);
        assert_eq!(psf.layers(), 2);
        assert_eq!(psf.dims(), [5, 5, 3]);
        let bank = psf.bank(spacing, 8);
        assert_eq!(bank.layers(), 2);
        assert_eq!(bank.band(), 4);
        assert_eq!(bank.layer(0, 3), 0);
        assert_eq!(bank.layer(0, 4), 1);
        let coarse = psf.bank(Spacing::new(0.2, 0.2, 0.6), 4);
        assert_eq!(coarse.dims(), [3, 3, 3]);
        let total: f32 = coarse.taps(0).iter().sum();
        assert!(
            (total - 1.0).abs() < 1.0e-5,
            "the resampled kernel keeps its energy"
        );
        let centre = 1 + 3 * (1 + 3);
        assert!(
            coarse.taps(0)[centre] > 0.9,
            "the point stays at the kernel centre: {:?}",
            coarse.taps(0)
        );
    }

    #[test]
    fn trimming_crops_a_kernel_to_the_energy_it_carries() {
        let kernel = point([7, 7, 7]);
        let trimmed = kernel.trimmed(1.0);
        assert_eq!(trimmed.dims(), [3, 3, 3]);
        let padded = trimmed.pad([5, 5, 5]);
        assert_eq!(padded.dims(), [5, 5, 5]);
        assert!((padded.values().iter().sum::<f32>() - 1.0).abs() < 1.0e-6);
        assert!(padded.values()[2 + 5 * (2 + 5 * 2)] > 0.99);
    }

    #[test]
    fn a_profile_builds_the_bank_of_its_model() {
        let params = PsfParams::new(1.4, 0.52, 1.515, 1.33, 1.0);
        let spacing = Spacing::new(0.04, 0.04, 0.1);
        let theoretical = Psf::theoretical(params).bank(spacing, 40);
        let expected = KernelBank::new(params, spacing, 40);
        assert_eq!(theoretical.dims(), expected.dims());
        assert_eq!(theoretical.layers(), expected.layers());
        assert_eq!(theoretical.taps(0), expected.taps(0));
        let measured = Psf::measured(MeasuredPsf::new(spacing, vec![point([3, 3, 3])]));
        assert!(measured.params().is_none());
        assert_eq!(measured.measured_span(), Some((1, [3, 3, 3])));
        assert_eq!(measured.bank(spacing, 3).layers(), 1);
    }
}
