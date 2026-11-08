use std::sync::Arc;

use grana_calibration::{Settings, calibrate as measure};
use grana_image::Image;

use crate::event::Event;

pub fn calibrate(
    image: Arc<Image>,
    settings: Settings,
    iterations: u32,
    sink: &mut dyn FnMut(Event),
) -> Result<(), String> {
    sink(Event::Progress {
        time: 0,
        done: 0,
        total: 1,
    });
    let calibration = measure(&image, settings).map_err(|error| error.to_string())?;
    let beads = calibration.beads();
    let psf = calibration.psf().clone();
    sink(Event::Progress {
        time: 0,
        done: 1,
        total: 1,
    });
    sink(Event::Calibration {
        source: settings.channel(),
        psf: Box::new(psf),
        beads,
        iterations,
    });
    sink(Event::Done);
    Ok(())
}
