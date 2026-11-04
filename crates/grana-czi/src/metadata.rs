use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

const METRES_PER_MICROMETRE: f32 = 1.0e6;

#[derive(Default)]
pub(crate) struct Metadata {
    pub(crate) images: Vec<ImageMetadata>,
    pub(crate) spacing: [Option<f32>; 3],
    pub(crate) interval: Option<f32>,
    pub(crate) channels: Vec<Channel>,
    pub(crate) display: Vec<Display>,
}

#[derive(Default)]
pub(crate) struct ImageMetadata {
    pub(crate) name: Option<String>,
    pub(crate) size_x: Option<u32>,
    pub(crate) size_y: Option<u32>,
    pub(crate) size_z: Option<u32>,
    pub(crate) size_c: Option<u32>,
    pub(crate) size_t: Option<u32>,
}

#[derive(Default)]
pub(crate) struct Channel {
    pub(crate) name: Option<String>,
    pub(crate) tint: Option<[f32; 3]>,
}

#[derive(Default)]
pub(crate) struct Display {
    pub(crate) name: Option<String>,
    pub(crate) tint: Option<[f32; 3]>,
    pub(crate) low: Option<f32>,
    pub(crate) high: Option<f32>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    SizeX,
    SizeY,
    SizeZ,
    SizeC,
    SizeT,
    Distance(usize),
    Increment,
    ChannelName,
    ChannelTint,
    DisplayTint,
    DisplayLow,
    DisplayHigh,
}

struct Capture {
    field: Field,
    depth: usize,
    text: String,
}

struct State {
    stack: Vec<Vec<u8>>,
    capture: Option<Capture>,
    distance: Option<usize>,
    metadata: Metadata,
}

pub(crate) fn parse(xml: &str) -> Result<Metadata, String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut state = State {
        stack: Vec::new(),
        capture: None,
        distance: None,
        metadata: Metadata::default(),
    };
    loop {
        match reader.read_event().map_err(|error| error.to_string())? {
            Event::Start(start) => {
                state
                    .stack
                    .push(start.name().local_name().as_ref().to_vec());
                state.enter(&start)?;
            }
            Event::Empty(start) => {
                state
                    .stack
                    .push(start.name().local_name().as_ref().to_vec());
                state.enter(&start)?;
                state.leave()?;
            }
            Event::End(_) => state.leave()?,
            Event::Text(text) => {
                if let Some(capture) = state.capture.as_mut() {
                    capture
                        .text
                        .push_str(&text.decode().map_err(|error| error.to_string())?);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(state.metadata)
}

impl State {
    fn enter(&mut self, start: &BytesStart) -> Result<(), String> {
        let depth = self.stack.len();
        let field = if suffix(&self.stack, &["Information", "Image"]) {
            self.metadata.images.push(ImageMetadata {
                name: attribute(start, b"Name")?.filter(|name| !name.trim().is_empty()),
                ..ImageMetadata::default()
            });
            return Ok(());
        } else if suffix(&self.stack, &["Information", "Image", "SizeX"]) {
            Field::SizeX
        } else if suffix(&self.stack, &["Information", "Image", "SizeY"]) {
            Field::SizeY
        } else if suffix(&self.stack, &["Information", "Image", "SizeZ"]) {
            Field::SizeZ
        } else if suffix(&self.stack, &["Information", "Image", "SizeC"]) {
            Field::SizeC
        } else if suffix(&self.stack, &["Information", "Image", "SizeT"]) {
            Field::SizeT
        } else if suffix(&self.stack, &["Scaling", "Items", "Distance"]) {
            self.distance = attribute(start, b"Id")?.as_deref().and_then(dimension);
            return Ok(());
        } else if suffix(&self.stack, &["Scaling", "Items", "Distance", "Value"]) {
            match self.distance {
                Some(dimension) => Field::Distance(dimension),
                None => return Ok(()),
            }
        } else if suffix(
            &self.stack,
            &["Dimensions", "T", "Positions", "Interval", "Increment"],
        ) {
            Field::Increment
        } else if suffix(&self.stack, &["Dimensions", "Channels", "Channel"]) {
            self.metadata.channels.push(Channel::default());
            return Ok(());
        } else if suffix(&self.stack, &["Dimensions", "Channels", "Channel", "Color"]) {
            Field::ChannelTint
        } else if suffix(&self.stack, &["Dimensions", "Channels", "Channel", "Fluor"]) {
            Field::ChannelName
        } else if suffix(&self.stack, &["DisplaySetting", "Channels", "Channel"]) {
            self.metadata.display.push(Display {
                name: attribute(start, b"Name")?,
                ..Display::default()
            });
            return Ok(());
        } else if suffix(
            &self.stack,
            &["DisplaySetting", "Channels", "Channel", "Color"],
        ) {
            Field::DisplayTint
        } else if suffix(
            &self.stack,
            &["DisplaySetting", "Channels", "Channel", "Low"],
        ) {
            Field::DisplayLow
        } else if suffix(
            &self.stack,
            &["DisplaySetting", "Channels", "Channel", "High"],
        ) {
            Field::DisplayHigh
        } else {
            return Ok(());
        };
        self.capture = Some(Capture {
            field,
            depth,
            text: String::new(),
        });
        Ok(())
    }

    fn leave(&mut self) -> Result<(), String> {
        let Some(name) = self.stack.pop() else {
            return Ok(());
        };
        let depth = self.stack.len() + 1;
        if self
            .capture
            .as_ref()
            .is_some_and(|capture| capture.depth == depth)
        {
            let capture = self.capture.take().expect("the capture is pending");
            self.commit(capture.field, capture.text.trim())?;
        }
        if name == b"Distance" {
            self.distance = None;
        }
        Ok(())
    }

    fn commit(&mut self, field: Field, text: &str) -> Result<(), String> {
        if text.is_empty() {
            return Ok(());
        }
        match field {
            Field::SizeX => self.size(text, |image| &mut image.size_x, "SizeX")?,
            Field::SizeY => self.size(text, |image| &mut image.size_y, "SizeY")?,
            Field::SizeZ => self.size(text, |image| &mut image.size_z, "SizeZ")?,
            Field::SizeC => self.size(text, |image| &mut image.size_c, "SizeC")?,
            Field::SizeT => self.size(text, |image| &mut image.size_t, "SizeT")?,
            Field::Distance(dimension) => {
                let length = number(text, "Distance")? * METRES_PER_MICROMETRE;
                if length <= 0.0 {
                    return Err(format!("the CZI metadata declares the distance `{text}`"));
                }
                self.metadata.spacing[dimension] = Some(length);
            }
            Field::Increment => {
                let interval = number(text, "Increment")?;
                if interval <= 0.0 {
                    return Err(format!(
                        "the CZI metadata declares the frame interval `{text}`"
                    ));
                }
                self.metadata.interval = Some(interval);
            }
            Field::ChannelName => {
                if let Some(channel) = self.metadata.channels.last_mut()
                    && channel.name.is_none()
                {
                    channel.name = Some(text.to_string());
                }
            }
            Field::ChannelTint => {
                if let Some(channel) = self.metadata.channels.last_mut()
                    && channel.tint.is_none()
                {
                    channel.tint = color(text);
                }
            }
            Field::DisplayTint => {
                if let Some(display) = self.metadata.display.last_mut()
                    && display.tint.is_none()
                {
                    display.tint = color(text);
                }
            }
            Field::DisplayLow => {
                if let Some(display) = self.metadata.display.last_mut() {
                    display.low = Some(number(text, "Low")?);
                }
            }
            Field::DisplayHigh => {
                if let Some(display) = self.metadata.display.last_mut() {
                    display.high = Some(number(text, "High")?);
                }
            }
        }
        Ok(())
    }

    fn size(
        &mut self,
        text: &str,
        field: fn(&mut ImageMetadata) -> &mut Option<u32>,
        label: &str,
    ) -> Result<(), String> {
        let Some(image) = self.metadata.images.last_mut() else {
            return Ok(());
        };
        *field(image) = Some(count(text, label)?);
        Ok(())
    }
}

fn suffix(stack: &[Vec<u8>], names: &[&str]) -> bool {
    stack.len() >= names.len()
        && stack[stack.len() - names.len()..]
            .iter()
            .zip(names)
            .all(|(name, wanted)| name == wanted.as_bytes())
}

fn attribute(start: &BytesStart, name: &[u8]) -> Result<Option<String>, String> {
    for attribute in start.attributes() {
        let attribute = attribute.map_err(|error| error.to_string())?;
        if attribute.key.local_name().as_ref() == name {
            let value = attribute
                .normalized_value(Default::default())
                .map_err(|error| error.to_string())?;
            return Ok(Some(value.into_owned()));
        }
    }
    Ok(None)
}

fn count(text: &str, label: &str) -> Result<u32, String> {
    let value = text
        .parse::<u32>()
        .map_err(|error| format!("the CZI metadata {label} `{text}` is invalid: {error}"))?;
    if value == 0 {
        return Err(format!("the CZI metadata declares a zero {label}"));
    }
    Ok(value)
}

fn number(text: &str, label: &str) -> Result<f32, String> {
    let value = text
        .parse::<f32>()
        .map_err(|error| format!("the CZI metadata {label} `{text}` is invalid: {error}"))?;
    if !value.is_finite() {
        return Err(format!("the CZI metadata {label} `{text}` is not finite"));
    }
    Ok(value)
}

fn dimension(id: &str) -> Option<usize> {
    match id {
        "X" => Some(0),
        "Y" => Some(1),
        "Z" => Some(2),
        _ => None,
    }
}

fn color(text: &str) -> Option<[f32; 3]> {
    let digits = text.strip_prefix('#').unwrap_or(text);
    if digits.len() != 6 && digits.len() != 8 {
        return None;
    }
    let value = u32::from_str_radix(digits, 16).ok()?;
    let tint = [
        f32::from((value >> 16) as u8) / 255.0,
        f32::from((value >> 8) as u8) / 255.0,
        f32::from(value as u8) / 255.0,
    ];
    tint.iter()
        .any(|component| *component > 0.0)
        .then_some(tint)
}
