use std::path::Path;

use quick_xml::Reader;
use quick_xml::Writer;
use quick_xml::events::attributes::Attribute;
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, Event};

use crate::error::Error;
use crate::lineage::{Detection, Edge, Lineage};

const VERSION: &str = "8.0.0";
const DEFAULT_RADIUS: f32 = 1.0;

pub struct Header {
    pub name: String,
    pub folder: String,
    pub extent: [u32; 3],
    pub frames: u32,
    pub spacing: [f32; 3],
    pub interval: f32,
}

impl Header {
    pub fn new(
        name: impl Into<String>,
        folder: impl Into<String>,
        extent: [u32; 3],
        frames: u32,
        spacing: [f32; 3],
        interval: f32,
    ) -> Self {
        let name = name.into();
        assert!(
            !name.trim().is_empty(),
            "an exported track file names its image"
        );
        assert!(
            extent.iter().all(|dimension| *dimension > 0),
            "an exported track file names a non empty image"
        );
        assert!(
            spacing.iter().all(|step| step.is_finite() && *step > 0.0),
            "an exported track file names a positive voxel size"
        );
        assert!(
            interval.is_finite() && interval > 0.0,
            "an exported track file names a positive frame interval"
        );
        Self {
            name,
            folder: folder.into(),
            extent,
            frames: frames.max(1),
            spacing,
            interval,
        }
    }
}

struct Metadata {
    scale: f32,
    seconds: bool,
    interval: Option<f32>,
}

pub fn read(path: &Path) -> Result<Lineage, Error> {
    let text = std::fs::read_to_string(path).map_err(|error| Error::Read {
        detail: format!("{}: {error}", path.display()),
    })?;
    parse(&text)
}

pub fn parse(text: &str) -> Result<Lineage, Error> {
    let metadata = metadata(text)?;
    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(true);
    let mut frame = None;
    let mut detections = Vec::new();
    let mut edges = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(start)) | Ok(Event::Empty(start)) => {
                match start.local_name().as_ref() {
                    b"SpotsInFrame" => frame = number(&start, b"frame")?,
                    b"Spot" => detections.push(detection(&start, frame, &metadata)?),
                    b"Edge" => edges.push(edge(&start)?),
                    _ => {}
                }
            }
            Ok(Event::End(end)) => {
                if end.local_name().as_ref() == b"SpotsInFrame" {
                    frame = None;
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(Error::Format {
                    detail: error.to_string(),
                });
            }
        }
    }
    if detections.is_empty() {
        return Err(Error::Format {
            detail: "the file holds no spot".to_string(),
        });
    }
    Lineage::new(detections, edges)
}

fn metadata(text: &str) -> Result<Metadata, Error> {
    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(true);
    let mut units = String::from("µm");
    let mut time = String::from("frame");
    let mut pixel = None;
    let mut interval = None;
    loop {
        match reader.read_event() {
            Ok(Event::Start(start)) | Ok(Event::Empty(start)) => {
                match start.local_name().as_ref() {
                    b"Model" => {
                        if let Some(value) = attribute(&start, b"spatialunits")? {
                            units = value;
                        }
                        if let Some(value) = attribute(&start, b"timeunits")? {
                            time = value;
                        }
                    }
                    b"ImageData" => {
                        pixel = float(&start, b"pixelwidth")?.filter(|value| *value > 0.0);
                        interval = float(&start, b"timeinterval")?.filter(|value| *value > 0.0);
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(Error::Format {
                    detail: error.to_string(),
                });
            }
        }
    }
    let scale = match units.as_str() {
        "pixel" | "pixels" => pixel
            .ok_or_else(|| invalid("the file measures positions in pixels without a pixel size"))?,
        other => length_scale(other)?,
    };
    let seconds = match time.to_ascii_lowercase().as_str() {
        "frame" | "frames" => false,
        "s" | "sec" | "second" | "seconds" => true,
        other => {
            return Err(Error::Format {
                detail: format!("the file measures time in `{other}`"),
            });
        }
    };
    Ok(Metadata {
        scale,
        seconds,
        interval,
    })
}

fn length_scale(units: &str) -> Result<f32, Error> {
    match units {
        "" | "µm" | "μm" | "um" | "micron" | "microns" | "micrometer" | "micrometers" => Ok(1.0),
        "nm" | "nanometer" | "nanometre" | "nanometers" => Ok(1.0e-3),
        "mm" | "millimeter" | "millimetre" => Ok(1.0e3),
        "cm" | "centimeter" | "centimetre" => Ok(1.0e4),
        "m" | "meter" | "metre" | "meters" => Ok(1.0e6),
        other => Err(Error::Format {
            detail: format!("the file measures positions in `{other}`"),
        }),
    }
}

fn detection(
    start: &BytesStart,
    frame: Option<u32>,
    metadata: &Metadata,
) -> Result<Detection, Error> {
    let id = unsigned(start, b"ID")?.ok_or_else(|| invalid("a spot carries no identifier"))?;
    let time = match (number(start, b"FRAME")?, frame) {
        (Some(frame), _) => frame,
        (None, Some(frame)) => frame,
        (None, None) => {
            let position = float(start, b"POSITION_T")?
                .ok_or_else(|| invalid("a spot names neither its frame nor its time"))?;
            if metadata.seconds {
                let interval = metadata.interval.ok_or_else(|| {
                    invalid("the file measures time in seconds without a frame interval")
                })?;
                (position / interval).round().max(0.0) as u32
            } else {
                position.round().max(0.0) as u32
            }
        }
    };
    let mut position = [0.0f32; 3];
    for (axis, key) in [&b"POSITION_X"[..], &b"POSITION_Y"[..], &b"POSITION_Z"[..]]
        .into_iter()
        .enumerate()
    {
        let value = float(start, key)?
            .ok_or_else(|| invalid("a spot carries no position on every axis"))?;
        position[axis] = value * metadata.scale;
    }
    let radius = float(start, b"RADIUS")?
        .filter(|radius| radius.is_finite() && *radius > 0.0)
        .map_or(DEFAULT_RADIUS * metadata.scale, |radius| {
            radius * metadata.scale
        });
    let quality = float(start, b"QUALITY")?.unwrap_or(0.0);
    Ok(Detection::new(id, time, position, radius, quality))
}

fn edge(start: &BytesStart) -> Result<Edge, Error> {
    let source = unsigned(start, b"SPOT_SOURCE_ID")?
        .ok_or_else(|| invalid("a link carries no source spot"))?;
    let target = unsigned(start, b"SPOT_TARGET_ID")?
        .ok_or_else(|| invalid("a link carries no target spot"))?;
    Ok(Edge::new(source, target))
}

fn invalid(detail: &str) -> Error {
    Error::Format {
        detail: detail.to_string(),
    }
}

fn attribute(start: &BytesStart, name: &[u8]) -> Result<Option<String>, Error> {
    for attribute in start.attributes() {
        let attribute = attribute.map_err(|error| Error::Format {
            detail: error.to_string(),
        })?;
        if attribute.key.local_name().as_ref() == name {
            return Ok(Some(
                attribute
                    .normalized_value(Default::default())
                    .map_err(|error| Error::Format {
                        detail: error.to_string(),
                    })?
                    .into_owned(),
            ));
        }
    }
    Ok(None)
}

fn float(start: &BytesStart, name: &[u8]) -> Result<Option<f32>, Error> {
    let Some(text) = attribute(start, name)? else {
        return Ok(None);
    };
    let value = text.parse::<f32>().map_err(|error| Error::Format {
        detail: format!("the attribute `{}` is not a number: {error}", lossy(name)),
    })?;
    if !value.is_finite() {
        return Err(Error::Format {
            detail: format!("the attribute `{}` is not a number: `{text}`", lossy(name)),
        });
    }
    Ok(Some(value))
}

fn number(start: &BytesStart, name: &[u8]) -> Result<Option<u32>, Error> {
    let Some(text) = attribute(start, name)? else {
        return Ok(None);
    };
    text.parse::<u32>()
        .map(Some)
        .map_err(|error| Error::Format {
            detail: format!("the attribute `{}` is not a frame: {error}", lossy(name)),
        })
}

fn unsigned(start: &BytesStart, name: &[u8]) -> Result<Option<u64>, Error> {
    let Some(text) = attribute(start, name)? else {
        return Ok(None);
    };
    text.parse::<u64>()
        .map(Some)
        .map_err(|error| Error::Format {
            detail: format!("the attribute `{}` is not an id: {error}", lossy(name)),
        })
}

fn lossy(name: &[u8]) -> String {
    String::from_utf8_lossy(name).into_owned()
}

pub fn write(path: &Path, lineage: &Lineage, header: &Header) -> Result<(), Error> {
    let paths = lineage.track_paths(header.interval)?;
    let mut writer = Writer::new_with_indent(Vec::new(), b' ', 2);
    let mut write = |event: Event<'_>| -> Result<(), Error> {
        writer.write_event(event).map_err(|error| Error::Write {
            detail: error.to_string(),
        })
    };
    write(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))?;
    write(Event::Start(element("TrackMate", &[("version", VERSION)])))?;
    write(Event::Start(element(
        "Model",
        &[("spatialunits", "µm"), ("timeunits", "frame")],
    )))?;
    write(Event::Start(element("FeatureDeclarations", &[])))?;
    write(Event::Start(element("SpotFeatures", &[])))?;
    for feature in [
        ("QUALITY", "Quality", "QUALITY", false),
        ("POSITION_X", "Position X", "POSITION", false),
        ("POSITION_Y", "Position Y", "POSITION", false),
        ("POSITION_Z", "Position Z", "POSITION", false),
        ("POSITION_T", "Time", "TIME", false),
        ("FRAME", "Frame", "FRAME", true),
        ("RADIUS", "Radius", "RADIUS", false),
    ] {
        write(Event::Empty(feature_declaration(feature)))?;
    }
    write(Event::End(BytesEnd::new("SpotFeatures")))?;
    write(Event::Empty(BytesStart::new("EdgeFeatures")))?;
    write(Event::Start(element("TrackFeatures", &[])))?;
    for feature in [
        ("TRACK_DURATION", "Duration", "TIME", false),
        ("TRACK_LENGTH", "Length", "LENGTH", false),
        ("TRACK_DISPLACEMENT", "Displacement", "LENGTH", false),
        ("TRACK_SPEED", "Speed", "VELOCITY", false),
        ("TRACK_STRAIGHTNESS", "Straightness", "NONE", false),
    ] {
        write(Event::Empty(feature_declaration(feature)))?;
    }
    write(Event::End(BytesEnd::new("TrackFeatures")))?;
    write(Event::End(BytesEnd::new("FeatureDeclarations")))?;
    let set = lineage.tracks(header.interval)?;
    let spots = lineage.len().to_string();
    write(Event::Start(element("AllSpots", &[("nspots", &spots)])))?;
    let mut index = 0usize;
    for frame in lineage.frames() {
        let time = frame.to_string();
        write(Event::Start(element("SpotsInFrame", &[("frame", &time)])))?;
        while index < lineage.len() && lineage.detections()[index].time() == frame {
            let detection = lineage.detections()[index];
            write(Event::Empty(spot(detection, frame)))?;
            index += 1;
        }
        write(Event::End(BytesEnd::new("SpotsInFrame")))?;
    }
    write(Event::End(BytesEnd::new("AllSpots")))?;
    write(Event::Start(element("AllTracks", &[])))?;
    for (track, path) in paths.iter().enumerate() {
        let id = track.to_string();
        let name = format!("Track_{track}");
        let count = path.nodes().len().to_string();
        let statistics = set
            .track(track)
            .expect("an exported path names a track of the set")
            .statistics(header.interval);
        let duration = statistics.duration().to_string();
        let length = statistics.length().to_string();
        let displacement = statistics.displacement().to_string();
        let speed = statistics.speed().to_string();
        let straightness = statistics.straightness().to_string();
        write(Event::Start(element(
            "Track",
            &[
                ("TRACK_ID", &id),
                ("name", &name),
                ("NUMBER_SPOTS", &count),
                ("NUMBER_GAPS", "0"),
                ("TRACK_DURATION", &duration),
                ("TRACK_LENGTH", &length),
                ("TRACK_DISPLACEMENT", &displacement),
                ("TRACK_SPEED", &speed),
                ("TRACK_STRAIGHTNESS", &straightness),
            ],
        )))?;
        if let Some(parent) = path.parent() {
            let previous = paths[parent]
                .nodes()
                .last()
                .expect("a parent track holds nodes");
            write(Event::Empty(link(*previous, path.nodes()[0])))?;
        }
        for pair in path.nodes().windows(2) {
            write(Event::Empty(link(pair[0], pair[1])))?;
        }
        write(Event::End(BytesEnd::new("Track")))?;
    }
    write(Event::End(BytesEnd::new("AllTracks")))?;
    write(Event::Start(element("FilteredTracks", &[])))?;
    for track in 0..paths.len() {
        let id = track.to_string();
        write(Event::Empty(element("TrackID", &[("TRACK_ID", &id)])))?;
    }
    write(Event::End(BytesEnd::new("FilteredTracks")))?;
    write(Event::End(BytesEnd::new("Model")))?;
    write(Event::Start(element("Settings", &[])))?;
    let width = header.extent[0].to_string();
    let height = header.extent[1].to_string();
    let slices = header.extent[2].to_string();
    let frames = header.frames.to_string();
    let pixel = header.spacing[0].to_string();
    let line = header.spacing[1].to_string();
    let voxel = header.spacing[2].to_string();
    let interval = header.interval.to_string();
    write(Event::Empty(element(
        "ImageData",
        &[
            ("filename", &header.name),
            ("folder", &header.folder),
            ("width", &width),
            ("height", &height),
            ("nslices", &slices),
            ("nframes", &frames),
            ("pixelwidth", &pixel),
            ("pixelheight", &line),
            ("voxeldepth", &voxel),
            ("timeinterval", &interval),
        ],
    )))?;
    let end = header.extent[0].saturating_sub(1).to_string();
    let bottom = header.extent[1].saturating_sub(1).to_string();
    let deep = header.extent[2].saturating_sub(1).to_string();
    let last = header.frames.saturating_sub(1).to_string();
    write(Event::Empty(element(
        "BasicSettings",
        &[
            ("xstart", "0"),
            ("xend", &end),
            ("ystart", "0"),
            ("yend", &bottom),
            ("zstart", "0"),
            ("zend", &deep),
            ("tstart", "0"),
            ("tend", &last),
        ],
    )))?;
    write(Event::End(BytesEnd::new("Settings")))?;
    write(Event::End(BytesEnd::new("TrackMate")))?;
    let bytes = writer.into_inner();
    std::fs::write(path, bytes).map_err(|error| Error::Write {
        detail: format!("{}: {error}", path.display()),
    })
}

fn feature_declaration(
    (feature, name, dimension, integer): (&str, &str, &str, bool),
) -> BytesStart<'static> {
    let mut start = BytesStart::new("Feature");
    start.push_attribute(Attribute::from(("feature", feature)));
    start.push_attribute(Attribute::from(("name", name)));
    start.push_attribute(Attribute::from(("shortname", name)));
    start.push_attribute(Attribute::from(("dimension", dimension)));
    start.push_attribute(Attribute::from((
        "isint",
        if integer { "true" } else { "false" },
    )));
    start
}

fn spot(detection: Detection, frame: u32) -> BytesStart<'static> {
    let id = detection.id().to_string();
    let time = frame.to_string();
    let x = detection.position()[0].to_string();
    let y = detection.position()[1].to_string();
    let z = detection.position()[2].to_string();
    let radius = detection.radius().to_string();
    let quality = detection.quality().to_string();
    let mut start = BytesStart::new("Spot");
    start.push_attribute(Attribute::from(("ID", id.as_str())));
    start.push_attribute(Attribute::from(("name", id.as_str())));
    start.push_attribute(Attribute::from(("QUALITY", quality.as_str())));
    start.push_attribute(Attribute::from(("POSITION_X", x.as_str())));
    start.push_attribute(Attribute::from(("POSITION_Y", y.as_str())));
    start.push_attribute(Attribute::from(("POSITION_Z", z.as_str())));
    start.push_attribute(Attribute::from(("POSITION_T", time.as_str())));
    start.push_attribute(Attribute::from(("FRAME", time.as_str())));
    start.push_attribute(Attribute::from(("RADIUS", radius.as_str())));
    start
}

fn link(source: u64, target: u64) -> BytesStart<'static> {
    let source = source.to_string();
    let target = target.to_string();
    let mut start = BytesStart::new("Edge");
    start.push_attribute(Attribute::from(("SPOT_SOURCE_ID", source.as_str())));
    start.push_attribute(Attribute::from(("SPOT_TARGET_ID", target.as_str())));
    start
}

fn element<'a>(name: &'a str, attributes: &[(&'a str, &'a str)]) -> BytesStart<'a> {
    let mut start = BytesStart::new(name);
    for (key, value) in attributes {
        start.push_attribute(Attribute::from((*key, *value)));
    }
    start
}
