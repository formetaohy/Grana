use grana_ome::length_unit;

const HEADER: &str = "ImageJ=";

const VERSION: usize = 1;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct ImageJ {
    channels: u32,
    slices: u32,
    frames: u32,
    declared: bool,
    spacing: Option<f32>,
    unit: Option<f32>,
    interval: Option<f32>,
}

impl ImageJ {
    pub(crate) fn parse(text: &str) -> Result<Option<Self>, String> {
        let Some(rest) = text.trim_start().strip_prefix(HEADER) else {
            return Ok(None);
        };
        let mut held = Self {
            channels: 1,
            slices: 1,
            frames: 1,
            ..Self::default()
        };
        let mut spacing = None;
        let mut declared_unit = None;
        let mut interval = None;
        let mut rate = None;
        for line in rest.split('\n').skip(VERSION) {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim();
            match key.trim() {
                "channels" => {
                    held.channels = count(value, "channels")?;
                    held.declared = true;
                }
                "slices" => {
                    held.slices = count(value, "slices")?;
                    held.declared = true;
                }
                "frames" => {
                    held.frames = count(value, "frames")?;
                    held.declared = true;
                }
                "spacing" => spacing = number(value, "spacing").ok().filter(|held| *held > 0.0),
                "unit" => declared_unit = unit(value),
                "finterval" => {
                    interval = number(value, "finterval").ok().filter(|held| *held > 0.0)
                }
                "fps" => rate = number(value, "fps").ok().filter(|held| *held > 0.0),
                _ => {}
            }
        }
        held.spacing = spacing
            .zip(declared_unit)
            .map(|(spacing, unit)| spacing * unit)
            .filter(|micrometres| *micrometres > 0.0);
        held.unit = declared_unit;
        held.interval = interval.or(rate.map(|rate| 1.0 / rate));
        Ok(Some(held))
    }

    pub(crate) fn sizes(&self) -> Option<[u32; 3]> {
        self.declared
            .then_some([self.frames, self.channels, self.slices])
    }

    pub(crate) fn spacing(&self) -> Option<f32> {
        self.spacing
    }

    pub(crate) fn unit(&self) -> Option<f32> {
        self.unit
    }

    pub(crate) fn interval(&self) -> Option<f32> {
        self.interval
    }
}

fn count(value: &str, key: &str) -> Result<u32, String> {
    match value.parse::<u32>() {
        Ok(count) if count > 0 => Ok(count),
        _ => Err(format!(
            "the ImageJ metadata declares `{key}={value}` outside the planes a stack holds"
        )),
    }
}

fn number(value: &str, key: &str) -> Result<f32, String> {
    match value.parse::<f32>() {
        Ok(number) if number.is_finite() => Ok(number),
        _ => Err(format!(
            "the ImageJ metadata declares `{key}={value}` outside the magnitudes it holds"
        )),
    }
}

fn unit(value: &str) -> Option<f32> {
    let value = value.trim().to_lowercase();
    let unit = match value.as_str() {
        "" | "pixel" | "pixels" | "px" => return None,
        "micron" | "microns" | "micrometer" | "micrometers" | "micrometre" | "micrometres" => "µm",
        "in" => "inch",
        "mil" => return Some(25.4),
        "point" | "points" | "pt" => return Some(352.777_78),
        other => other,
    };
    length_unit(Some(unit)).ok()
}
