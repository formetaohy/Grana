use grana_calibration::{Settings, calibrate};
use grana_image::{Image, Source};
use grana_volume::{Channel, Extent, Spacing, Volume, VolumeSeries, VoxelData};

fn image_with_beads() -> Image {
    let dims = Extent::new(64, 64, 24);
    let spacing = Spacing::new(0.1, 0.1, 0.2);
    let mut values = vec![0.0f32; dims.voxel_count() as usize];
    for (index, value) in values.iter_mut().enumerate() {
        *value = 100.0 + ((index * 37) % 17) as f32 - 8.0;
    }
    let beads = [
        [16.0f32, 16.0, 6.0],
        [32.0, 32.0, 12.0],
        [48.0, 48.0, 18.0],
        [20.0, 44.0, 9.0],
    ];
    for bead in beads {
        for z in 0..dims.z() {
            for y in 0..dims.y() {
                for x in 0..dims.x() {
                    let offset = [
                        (x as f32 - bead[0]) / 1.5,
                        (y as f32 - bead[1]) / 1.5,
                        (z as f32 - bead[2]) / 1.6,
                    ];
                    let falloff =
                        (-0.5 * offset.iter().map(|value| value * value).sum::<f32>()).exp();
                    values[(x + dims.x() * (y + dims.y() * z)) as usize] += 900.0 * falloff;
                }
            }
        }
    }
    let volume = Volume::new(
        dims,
        spacing,
        vec![Channel::new(
            "beads",
            [0.0, 1.0, 0.0],
            VoxelData::F32(values),
        )],
    );
    Image::open(&Source::memory(VolumeSeries::new(vec![volume], 1.0)))
        .expect("the synthetic bead stack opens")
}

#[test]
fn calibrates_a_point_spread_function_from_beads() {
    let image = image_with_beads();
    let settings = Settings::for_image(&image, 0, 0);
    let calibration = calibrate(&image, settings).expect("the beads calibrate");
    assert!(calibration.beads() >= 3, "the stack holds isolated beads");
    assert_eq!(calibration.psf().dims(), [13, 13, 13]);
    assert_eq!(calibration.psf().layers(), 1);
    let kernel = calibration.psf().kernels()[0].values();
    let dims = calibration.psf().dims();
    let centre = (dims[0] / 2 + dims[0] * (dims[1] / 2 + dims[1] * (dims[2] / 2))) as usize;
    let total: f32 = kernel.iter().sum();
    assert!(
        (total - 1.0).abs() < 1.0e-5,
        "the kernel carries unit energy"
    );
    assert!(
        kernel[centre] > kernel[0] * 100.0,
        "the calibrated kernel peaks at its centre"
    );
}

#[test]
fn splits_the_calibrated_profile_into_depth_bands() {
    let image = image_with_beads();
    let defaults = Settings::for_image(&image, 0, 0);
    let settings = Settings::new(
        0,
        0,
        defaults.threshold(),
        defaults.separation(),
        defaults.half(),
        2,
    );
    let calibration = calibrate(&image, settings).expect("the beads calibrate");
    assert_eq!(calibration.psf().layers(), 2);
    assert_eq!(
        calibration.bands().iter().sum::<usize>(),
        calibration.beads()
    );
}

#[test]
fn refuses_a_channel_without_beads() {
    let dims = Extent::new(32, 32, 8);
    let volume = Volume::new(
        dims,
        Spacing::new(0.1, 0.1, 0.2),
        vec![Channel::new(
            "flat",
            [1.0, 1.0, 1.0],
            VoxelData::F32(vec![100.0; dims.voxel_count() as usize]),
        )],
    );
    let image = Image::open(&Source::memory(VolumeSeries::new(vec![volume], 1.0)))
        .expect("the synthetic stack opens");
    let settings = Settings::for_image(&image, 0, 0);
    assert!(calibrate(&image, settings).is_err());
}
