use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use crate::image::{Channel, Image, Plane, TiffData};

pub struct Metadata {
    pub images: Vec<Image>,
    pub binary_only: bool,
}

pub fn parse(xml: &str) -> Result<Metadata, String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut images: Vec<Image> = Vec::new();
    let mut binary_only = false;
    let mut depth = 0usize;
    let mut image_depth = None;
    let mut pending: Option<TiffData> = None;
    let mut annotation: Option<Annotation> = None;
    loop {
        let event = reader.read_event().map_err(|error| error.to_string())?;
        match event {
            Event::Start(start) => {
                depth += 1;
                let name = start.name();
                let name = name.local_name();
                if name.as_ref() == b"Image" {
                    if image_depth.is_none() {
                        image_depth = Some(depth);
                        images.push(series(&start)?);
                    }
                } else if image_depth.is_some() {
                    let series = images.last_mut().expect("an Image element opened a series");
                    element(&start, series, &mut pending)?;
                } else if name.as_ref() == b"BinaryOnly" {
                    binary_only = true;
                }
                if name.as_ref() == b"MapAnnotation" {
                    annotation = Some(Annotation::new());
                } else if let Some(held) = annotation.as_mut() {
                    held.enter(name.as_ref());
                }
            }
            Event::Empty(start) => {
                let name = start.name();
                let name = name.local_name();
                if name.as_ref() == b"Image" {
                    if image_depth.is_none() {
                        images.push(series(&start)?);
                    }
                } else if image_depth.is_some() {
                    let series = images.last_mut().expect("an Image element opened a series");
                    element(&start, series, &mut pending)?;
                    if name.as_ref() == b"TiffData"
                        && let Some(block) = pending.take()
                    {
                        series.tiff_data.push(block);
                    }
                } else if name.as_ref() == b"BinaryOnly" {
                    binary_only = true;
                }
            }
            Event::Text(text) => {
                if let Some(held) = annotation.as_mut() {
                    let decoded = text.decode().map_err(|error| error.to_string())?;
                    let value =
                        quick_xml::escape::unescape(&decoded).map_err(|error| error.to_string())?;
                    held.text(&value);
                }
            }
            Event::End(end) => {
                let name = end.name();
                let name = name.local_name();
                if image_depth.is_some() && name.as_ref() == b"TiffData" {
                    let series = images.last_mut().expect("an Image element opened a series");
                    if let Some(block) = pending.take() {
                        series.tiff_data.push(block);
                    }
                }
                if image_depth == Some(depth) && name.as_ref() == b"Image" {
                    image_depth = None;
                }
                if name.as_ref() == b"MapAnnotation"
                    && let Some(held) = annotation.take()
                {
                    held.commit(&mut images)?;
                } else if let Some(held) = annotation.as_mut() {
                    held.leave(name.as_ref());
                }
                depth = depth.saturating_sub(1);
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if let (Some(series), Some(block)) = (images.last_mut(), pending) {
        series.tiff_data.push(block);
    }
    Ok(Metadata {
        images,
        binary_only,
    })
}

struct Annotation {
    pairs: Vec<(String, String)>,
    key: Option<String>,
    capturing: bool,
    text: String,
}

impl Annotation {
    fn new() -> Self {
        Self {
            pairs: Vec::new(),
            key: None,
            capturing: false,
            text: String::new(),
        }
    }

    fn enter(&mut self, name: &[u8]) {
        if matches!(name, b"M" | b"S") {
            self.capturing = true;
            self.text.clear();
        }
    }

    fn leave(&mut self, name: &[u8]) {
        match name {
            b"M" => {
                self.capturing = false;
                self.key = Some(self.text.trim().to_string());
            }
            b"S" => {
                self.capturing = false;
                if let Some(key) = self.key.take() {
                    self.pairs.push((key, self.text.clone()));
                }
            }
            _ => {}
        }
    }

    fn text(&mut self, value: &str) {
        if self.capturing {
            self.text.push_str(value);
        }
    }

    fn commit(self, images: &mut [Image]) -> Result<(), String> {
        let marked = self
            .pairs
            .iter()
            .any(|(key, value)| key == "grana" && value == "1");
        if !marked {
            return Ok(());
        }
        let index = self
            .pairs
            .iter()
            .find(|(key, _)| key == "image")
            .and_then(|(_, value)| value.parse::<usize>().ok())
            .ok_or_else(|| "a grana annotation names no image".to_string())?;
        let image = images
            .get_mut(index)
            .ok_or_else(|| format!("a grana annotation names the missing image {index}"))?;
        for (key, value) in self.pairs {
            if key != "grana" && key != "image" {
                image.annotations.push((key, value));
            }
        }
        Ok(())
    }
}

fn series(start: &BytesStart) -> Result<Image, String> {
    Ok(Image {
        name: attribute(start, b"Name")?,
        ..Image::default()
    })
}

fn element(
    start: &BytesStart,
    ome: &mut Image,
    pending: &mut Option<TiffData>,
) -> Result<(), String> {
    match start.name().local_name().as_ref() {
        b"BinaryOnly" => {
            ome.binary_only = true;
            Ok(())
        }
        b"Pixels" => pixels(start, ome),
        b"Channel" => channel(start, ome),
        b"Plane" => plane(start, ome),
        b"TiffData" => {
            let ifd = integer_attribute(start, b"IFD")?;
            *pending = Some(TiffData {
                ifd: ifd.unwrap_or(0),
                z: integer_attribute(start, b"FirstZ")?.unwrap_or(0),
                c: integer_attribute(start, b"FirstC")?.unwrap_or(0),
                t: integer_attribute(start, b"FirstT")?.unwrap_or(0),
                planes: integer_attribute(start, b"PlaneCount")?
                    .unwrap_or(u32::from(ifd.is_some())),
                file: attribute(start, b"FileName")?,
            });
            Ok(())
        }
        b"UUID" => {
            let Some(name) = attribute(start, b"FileName")? else {
                return Ok(());
            };
            if let Some(block) = pending.as_mut() {
                block.file.get_or_insert(name);
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn pixels(start: &BytesStart, ome: &mut Image) -> Result<(), String> {
    ome.dimension_order = attribute(start, b"DimensionOrder")?;
    ome.size_x = integer_attribute(start, b"SizeX")?;
    ome.size_y = integer_attribute(start, b"SizeY")?;
    ome.size_z = integer_attribute(start, b"SizeZ")?;
    ome.size_c = integer_attribute(start, b"SizeC")?;
    ome.size_t = integer_attribute(start, b"SizeT")?;
    ome.physical[0] = length_attribute(start, b"PhysicalSizeX", b"PhysicalSizeXUnit")?;
    ome.physical[1] = length_attribute(start, b"PhysicalSizeY", b"PhysicalSizeYUnit")?;
    ome.physical[2] = length_attribute(start, b"PhysicalSizeZ", b"PhysicalSizeZUnit")?;
    ome.interval = time_attribute(start)?;
    Ok(())
}

fn channel(start: &BytesStart, ome: &mut Image) -> Result<(), String> {
    let name = attribute(start, b"Name")?;
    let tint = match attribute(start, b"Color")? {
        Some(color) => {
            let value = color
                .parse::<i32>()
                .map_err(|error| format!("the channel color `{color}` is invalid: {error}"))?
                as u32;
            Some([
                f32::from((value >> 16) as u8) / 255.0,
                f32::from((value >> 8) as u8) / 255.0,
                f32::from(value as u8) / 255.0,
            ])
            .filter(|tint| tint.iter().any(|component| *component > 0.0))
        }
        None => None,
    };
    ome.channels.push(Channel { name, tint });
    Ok(())
}

fn plane(start: &BytesStart, ome: &mut Image) -> Result<(), String> {
    let mut position = [None; 3];
    for (axis, (name, unit)) in [
        (b"PositionX", b"PositionXUnit"),
        (b"PositionY", b"PositionYUnit"),
        (b"PositionZ", b"PositionZUnit"),
    ]
    .into_iter()
    .enumerate()
    {
        position[axis] = point_attribute(start, name, unit)?;
    }
    ome.planes.push(Plane {
        z: integer_attribute(start, b"TheZ")?.unwrap_or(0),
        c: integer_attribute(start, b"TheC")?.unwrap_or(0),
        t: integer_attribute(start, b"TheT")?.unwrap_or(0),
        position,
    });
    Ok(())
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

fn integer_attribute(start: &BytesStart, name: &[u8]) -> Result<Option<u32>, String> {
    let Some(text) = attribute(start, name)? else {
        return Ok(None);
    };
    let value = text
        .parse::<u32>()
        .map_err(|error| format!("the OME attribute `{}` is invalid: {error}", label(name)))?;
    Ok(Some(value))
}

fn length_attribute(start: &BytesStart, name: &[u8], unit: &[u8]) -> Result<Option<f32>, String> {
    let Some(text) = attribute(start, name)? else {
        return Ok(None);
    };
    let value = text
        .parse::<f32>()
        .map_err(|error| format!("the OME attribute `{}` is invalid: {error}", label(name)))?;
    let unit = attribute(start, unit)?;
    space(value, unit.as_deref()).map(Some)
}

fn point_attribute(start: &BytesStart, name: &[u8], unit: &[u8]) -> Result<Option<f32>, String> {
    let Some(text) = attribute(start, name)? else {
        return Ok(None);
    };
    let value = text
        .parse::<f32>()
        .map_err(|error| format!("the OME attribute `{}` is invalid: {error}", label(name)))?;
    let unit = attribute(start, unit)?;
    point(value, unit.as_deref()).map(Some)
}

fn time_attribute(start: &BytesStart) -> Result<Option<f32>, String> {
    let Some(text) = attribute(start, b"TimeIncrement")? else {
        return Ok(None);
    };
    let value = text
        .parse::<f32>()
        .map_err(|error| format!("the OME time increment is invalid: {error}"))?;
    let unit = attribute(start, b"TimeIncrementUnit")?;
    duration(value, unit.as_deref()).map(Some)
}

fn space(value: f32, unit: Option<&str>) -> Result<f32, String> {
    positive(value * length_unit(unit)?, "physical size")
}

fn point(value: f32, unit: Option<&str>) -> Result<f32, String> {
    let scaled = value * length_unit(unit)?;
    if scaled.is_finite() {
        Ok(scaled)
    } else {
        Err("the OME plane position is not a finite number".to_string())
    }
}

fn length_unit(unit: Option<&str>) -> Result<f32, String> {
    Ok(match unit.unwrap_or("µm") {
        "µm" | "μm" | "um" => 1.0,
        "nm" => 1e-3,
        "pm" => 1e-6,
        "mm" => 1e3,
        "cm" => 1e4,
        "m" => 1e6,
        "Å" | "angstrom" => 1e-4,
        "in" | "inch" => 25_400.0,
        other => return Err(format!("the physical unit `{other}` is not supported")),
    })
}

fn duration(value: f32, unit: Option<&str>) -> Result<f32, String> {
    let factor = match unit.unwrap_or("s") {
        "s" => 1.0,
        "ms" => 1e-3,
        "µs" | "μs" | "us" => 1e-6,
        "ns" => 1e-9,
        "min" => 60.0,
        "h" => 3600.0,
        other => return Err(format!("the time unit `{other}` is not supported")),
    };
    positive(value * factor, "time increment")
}

fn positive(value: f32, label: &str) -> Result<f32, String> {
    if value.is_finite() && value > 0.0 {
        Ok(value)
    } else {
        Err(format!("the OME {label} is not a positive number"))
    }
}

fn label(name: &[u8]) -> String {
    String::from_utf8_lossy(name).into_owned()
}
