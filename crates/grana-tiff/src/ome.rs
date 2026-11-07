use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

#[derive(Default)]
pub(crate) struct Ome {
    pub name: Option<String>,
    pub dimension_order: Option<String>,
    pub size_x: Option<u32>,
    pub size_y: Option<u32>,
    pub size_z: Option<u32>,
    pub size_c: Option<u32>,
    pub size_t: Option<u32>,
    pub physical: [Option<f32>; 3],
    pub interval: Option<f32>,
    pub channels: Vec<Channel>,
    pub tiff_data: Vec<TiffData>,
}

pub(crate) struct Channel {
    pub name: Option<String>,
    pub tint: Option<[f32; 3]>,
}

#[derive(Default, Clone)]
pub(crate) struct TiffData {
    pub ifd: u32,
    pub z: u32,
    pub c: u32,
    pub t: u32,
    pub planes: u32,
    pub file: Option<String>,
}

pub(crate) fn parse(xml: &str) -> Result<Option<Ome>, String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut ome = Ome::default();
    let mut depth = 0usize;
    let mut image_depth = None;
    let mut pending: Option<TiffData> = None;
    loop {
        let event = reader.read_event().map_err(|error| error.to_string())?;
        match event {
            Event::Start(start) => {
                depth += 1;
                if start.name().local_name().as_ref() == b"Image" {
                    if image_depth.is_none() {
                        image_depth = Some(depth);
                        ome.name = attribute(&start, b"Name")?;
                    }
                } else if image_depth.is_some() {
                    element(&start, &mut ome, &mut pending)?;
                }
            }
            Event::Empty(start) => {
                if image_depth.is_some() {
                    element(&start, &mut ome, &mut pending)?;
                    if start.name().local_name().as_ref() == b"TiffData"
                        && let Some(block) = pending.take()
                    {
                        ome.tiff_data.push(block);
                    }
                }
            }
            Event::End(end) => {
                if image_depth == Some(depth) && end.name().local_name().as_ref() == b"Image" {
                    break;
                }
                if depth == 0 {
                    break;
                }
                if end.name().local_name().as_ref() == b"TiffData"
                    && let Some(block) = pending.take()
                {
                    ome.tiff_data.push(block);
                }
                depth -= 1;
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if let Some(block) = pending {
        ome.tiff_data.push(block);
    }
    Ok(image_depth.map(|_| ome))
}

fn element(
    start: &BytesStart,
    ome: &mut Ome,
    pending: &mut Option<TiffData>,
) -> Result<(), String> {
    match start.name().local_name().as_ref() {
        b"Pixels" => pixels(start, ome),
        b"Channel" => channel(start, ome),
        b"TiffData" => {
            *pending = Some(TiffData {
                ifd: integer_attribute(start, b"IFD")?.unwrap_or(0),
                z: integer_attribute(start, b"FirstZ")?.unwrap_or(0),
                c: integer_attribute(start, b"FirstC")?.unwrap_or(0),
                t: integer_attribute(start, b"FirstT")?.unwrap_or(0),
                planes: integer_attribute(start, b"PlaneCount")?.unwrap_or(1),
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

fn pixels(start: &BytesStart, ome: &mut Ome) -> Result<(), String> {
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

fn channel(start: &BytesStart, ome: &mut Ome) -> Result<(), String> {
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
    let factor = match unit.unwrap_or("µm") {
        "µm" | "μm" | "um" => 1.0,
        "nm" => 1e-3,
        "pm" => 1e-6,
        "mm" => 1e3,
        "cm" => 1e4,
        "m" => 1e6,
        "Å" | "angstrom" => 1e-4,
        "in" | "inch" => 25_400.0,
        other => return Err(format!("the physical unit `{other}` is not supported")),
    };
    positive(value * factor, "physical size")
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
