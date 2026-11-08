use std::path::Path;

use serde_json::{Map, Value};

use crate::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AxisKind {
    Space,
    Time,
    Channel,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AxisUnit {
    Space(f64),
    Time(f64),
}

#[derive(Clone, Debug)]
pub struct Axis {
    name: String,
    kind: AxisKind,
    unit: Option<AxisUnit>,
}

impl Axis {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn kind(&self) -> AxisKind {
        self.kind
    }

    pub fn factor(&self) -> f64 {
        match self.unit {
            Some(AxisUnit::Space(factor)) | Some(AxisUnit::Time(factor)) => factor,
            None => 1.0,
        }
    }

    pub fn json(&self) -> Value {
        let kind = match self.kind {
            AxisKind::Space => "space",
            AxisKind::Time => "time",
            AxisKind::Channel => "channel",
        };
        let unit = match self.unit {
            Some(AxisUnit::Space(_)) => Some("micrometer"),
            Some(AxisUnit::Time(_)) => Some("second"),
            None => None,
        };
        let mut axis = Map::new();
        axis.insert("name".to_string(), Value::String(self.name.clone()));
        axis.insert("type".to_string(), Value::String(kind.to_string()));
        if let Some(unit) = unit {
            axis.insert("unit".to_string(), Value::String(unit.to_string()));
        }
        Value::Object(axis)
    }
}

#[derive(Clone, Debug)]
pub struct Metadata {
    pub name: Option<String>,
    pub version: String,
    pub minor: u32,
    pub axes: Vec<Axis>,
    pub datasets: Vec<Dataset>,
    pub channels: Vec<ChannelMetadata>,
}

#[derive(Clone, Debug)]
pub struct Dataset {
    pub path: String,
    pub scale: Vec<f64>,
    pub translation: Vec<f64>,
}

#[derive(Clone, Debug)]
pub struct ChannelMetadata {
    pub label: Option<String>,
    pub tint: [f32; 3],
    pub data: Option<[f32; 2]>,
    pub window: Option<[f32; 2]>,
}

#[derive(Clone, Debug)]
struct Transform {
    scale: Vec<f64>,
    translation: Vec<f64>,
}

impl Transform {
    fn identity(axes: usize) -> Self {
        Self {
            scale: vec![1.0; axes],
            translation: vec![0.0; axes],
        }
    }

    fn compose(outer: &Transform, inner: &Transform) -> Self {
        Self {
            scale: outer
                .scale
                .iter()
                .zip(&inner.scale)
                .map(|(outer, inner)| outer * inner)
                .collect(),
            translation: outer
                .scale
                .iter()
                .zip(&inner.translation)
                .zip(&outer.translation)
                .map(|((scale, inner), outer)| scale * inner + outer)
                .collect(),
        }
    }
}

pub fn parse(path: &Path, attributes: &Map<String, Value>) -> Result<Metadata, Error> {
    let (version, fields) = match attributes.get("ome") {
        Some(Value::Object(ome)) => {
            let version = ome
                .get("version")
                .and_then(Value::as_str)
                .ok_or_else(|| metadata_error(path, "the ome metadata names no version"))?;
            (version.to_string(), ome)
        }
        Some(_) => {
            return Err(metadata_error(
                path,
                "the ome metadata is not a json object",
            ));
        }
        None => ("0.4".to_string(), attributes),
    };
    let minor = minor_version(path, &version)?;
    let multiscales = fields
        .get("multiscales")
        .and_then(Value::as_array)
        .and_then(|multiscales| multiscales.first())
        .ok_or_else(|| absent(path, attributes, fields))?;
    let (axes, mut datasets) = if minor >= 6 {
        named(path, multiscales)?
    } else {
        direct(path, multiscales)?
    };
    for dataset in &mut datasets {
        for (index, axis) in axes.iter().enumerate() {
            if let Some(value) = dataset.scale.get_mut(index) {
                *value *= axis.factor();
            }
            if let Some(value) = dataset.translation.get_mut(index) {
                *value *= axis.factor();
            }
        }
    }
    let channels = channels(path, fields.get("omero"))?;
    Ok(Metadata {
        name: multiscales
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty())
            .map(str::to_owned),
        version,
        minor,
        axes,
        datasets,
        channels,
    })
}

fn minor_version(path: &Path, version: &str) -> Result<u32, Error> {
    let mut parts = version.split('.');
    let major = parts
        .next()
        .filter(|major| major.chars().all(|character| character.is_ascii_digit()))
        .and_then(|major| major.parse::<u32>().ok())
        .ok_or_else(|| {
            metadata_error(path, format!("the OME-Zarr version `{version}` is invalid"))
        })?;
    let minor = parts
        .next()
        .filter(|minor| minor.chars().all(|character| character.is_ascii_digit()))
        .and_then(|minor| minor.parse::<u32>().ok())
        .unwrap_or(0);
    if major != 0 {
        return Err(metadata_error(
            path,
            format!("the OME-Zarr version `{version}` is not supported"),
        ));
    }
    Ok(minor)
}

fn absent(path: &Path, attributes: &Map<String, Value>, fields: &Map<String, Value>) -> Error {
    let collection = fields
        .get("bioformats2raw.layout")
        .or_else(|| attributes.get("bioformats2raw.layout"))
        .is_some();
    let plate = fields
        .get("plate")
        .or_else(|| attributes.get("plate"))
        .is_some();
    let detail = if collection {
        "the dataset is a bioformats2raw image collection; open one of its image groups instead"
    } else if plate {
        "high content screening plates are not supported; open a field of view instead"
    } else {
        "the group has no multiscales metadata"
    };
    metadata_error(path, detail)
}

fn direct(path: &Path, multiscale: &Value) -> Result<(Vec<Axis>, Vec<Dataset>), Error> {
    let axes = axes_of(path, multiscale.get("axes"))?;
    let global = multiscale
        .get("coordinateTransformations")
        .map(|value| transform(path, value, axes.len()))
        .transpose()?
        .unwrap_or_else(|| Transform::identity(axes.len()));
    let datasets = datasets_of(path, multiscale, axes.len(), Some(&global))?;
    Ok((axes, datasets))
}

fn named(path: &Path, multiscale: &Value) -> Result<(Vec<Axis>, Vec<Dataset>), Error> {
    let systems = multiscale
        .get("coordinateSystems")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            metadata_error(path, "the multiscales metadata names no coordinate system")
        })?;
    let datasets = multiscale
        .get("datasets")
        .and_then(Value::as_array)
        .ok_or_else(|| metadata_error(path, "the multiscales metadata lists no dataset"))?;
    let intrinsic = datasets
        .first()
        .and_then(|dataset| dataset.get("coordinateTransformations"))
        .and_then(Value::as_array)
        .and_then(|transforms| transforms.last())
        .and_then(|transform| transform.get("output"))
        .and_then(|output| output.get("name"))
        .and_then(Value::as_str)
        .ok_or_else(|| {
            metadata_error(
                path,
                "the dataset transforms name no intrinsic coordinate system",
            )
        })?;
    let system = systems
        .iter()
        .find(|system| system.get("name").and_then(Value::as_str) == Some(intrinsic))
        .ok_or_else(|| {
            metadata_error(
                path,
                format!("the coordinate system `{intrinsic}` is not declared"),
            )
        })?;
    let axes = axes_of(path, system.get("axes"))?;
    let datasets = datasets_of(path, multiscale, axes.len(), None)?;
    Ok((axes, datasets))
}

fn datasets_of(
    path: &Path,
    multiscale: &Value,
    axes: usize,
    global: Option<&Transform>,
) -> Result<Vec<Dataset>, Error> {
    let datasets = multiscale
        .get("datasets")
        .and_then(Value::as_array)
        .ok_or_else(|| metadata_error(path, "the multiscales metadata lists no dataset"))?;
    if datasets.is_empty() {
        return Err(metadata_error(path, "the multiscales list has no datasets"));
    }
    datasets
        .iter()
        .map(|dataset| {
            let name = dataset
                .get("path")
                .and_then(Value::as_str)
                .ok_or_else(|| metadata_error(path, "a dataset names no array path"))?;
            let transforms = dataset
                .get("coordinateTransformations")
                .map(|value| transform(path, value, axes))
                .transpose()?;
            let local = transforms.unwrap_or_else(|| Transform::identity(axes));
            let total = match global {
                Some(global) => Transform::compose(global, &local),
                None => local,
            };
            if total.scale.len() != axes {
                return Err(metadata_error(
                    path,
                    format!(
                        "the scale of the array `{name}` has {} components for {axes} axes",
                        total.scale.len()
                    ),
                ));
            }
            Ok(Dataset {
                path: name.to_string(),
                scale: total.scale,
                translation: total.translation,
            })
        })
        .collect()
}

fn transform(path: &Path, value: &Value, axes: usize) -> Result<Transform, Error> {
    match value {
        Value::Array(transforms) => {
            let mut total = Transform::identity(axes);
            for transform in transforms {
                total = Transform::compose(&single(path, transform, axes)?, &total);
            }
            Ok(total)
        }
        Value::Object(_) => single(path, value, axes),
        _ => Err(metadata_error(
            path,
            "a coordinate transformation is not a json object or array",
        )),
    }
}

fn single(path: &Path, value: &Value, axes: usize) -> Result<Transform, Error> {
    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| metadata_error(path, "a coordinate transformation names no type"))?;
    let mut transform = Transform::identity(axes);
    match kind {
        "identity" => {}
        "scale" => transform.scale = vector(path, value, "scale", axes)?,
        "translation" => transform.translation = vector(path, value, "translation", axes)?,
        "sequence" => {
            let inner = value
                .get("transformations")
                .and_then(Value::as_array)
                .ok_or_else(|| metadata_error(path, "a sequence transform lists no transforms"))?;
            for entry in inner {
                transform = Transform::compose(&single(path, entry, axes)?, &transform);
            }
        }
        other => {
            return Err(metadata_error(
                path,
                format!("the coordinate transformation `{other}` is not supported"),
            ));
        }
    }
    Ok(transform)
}

fn vector(path: &Path, value: &Value, key: &str, axes: usize) -> Result<Vec<f64>, Error> {
    let values = value
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| metadata_error(path, format!("the `{key}` transform holds no vector")))?;
    if values.len() != axes {
        return Err(metadata_error(
            path,
            format!(
                "the `{key}` transform has {} components for {axes} axes",
                values.len()
            ),
        ));
    }
    values
        .iter()
        .map(|value| {
            value
                .as_f64()
                .filter(|value| value.is_finite())
                .ok_or_else(|| metadata_error(path, format!("the `{key}` transform is not finite")))
        })
        .collect()
}

fn axes_of(path: &Path, value: Option<&Value>) -> Result<Vec<Axis>, Error> {
    let axes = value
        .and_then(Value::as_array)
        .ok_or_else(|| metadata_error(path, "the metadata lists no axes"))?;
    axes.iter()
        .map(|axis| axis_of(path, axis))
        .collect::<Result<Vec<_>, Error>>()
}

fn axis_of(path: &Path, value: &Value) -> Result<Axis, Error> {
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| metadata_error(path, "an axis names no dimension"))?;
    let kind = match value.get("type").and_then(Value::as_str) {
        Some("space") => AxisKind::Space,
        Some("time") => AxisKind::Time,
        Some("channel") => AxisKind::Channel,
        Some(other) => {
            return Err(metadata_error(
                path,
                format!("the custom axis type `{other}` is not supported"),
            ));
        }
        None => match name {
            "x" | "y" | "z" => AxisKind::Space,
            "t" => AxisKind::Time,
            "c" => AxisKind::Channel,
            other => {
                return Err(metadata_error(
                    path,
                    format!("the axis `{other}` has no recognisable type"),
                ));
            }
        },
    };
    let unit = match value.get("unit").and_then(Value::as_str) {
        Some(unit) => Some(unit_of(path, kind, unit)?),
        None => None,
    };
    Ok(Axis {
        name: name.to_string(),
        kind,
        unit,
    })
}

fn unit_of(path: &Path, kind: AxisKind, unit: &str) -> Result<AxisUnit, Error> {
    match kind {
        AxisKind::Space => space_factor(unit)
            .map(AxisUnit::Space)
            .ok_or_else(|| metadata_error(path, format!("the unit `{unit}` is not supported"))),
        AxisKind::Time => time_factor(unit)
            .map(AxisUnit::Time)
            .ok_or_else(|| metadata_error(path, format!("the unit `{unit}` is not supported"))),
        AxisKind::Channel => Err(metadata_error(
            path,
            format!("the channel axis carries the unit `{unit}`"),
        )),
    }
}

fn space_factor(unit: &str) -> Option<f64> {
    Some(match unit {
        "angstrom" => 1e-4,
        "attometer" => 1e-12,
        "centimeter" => 1e4,
        "decimeter" => 1e5,
        "exameter" => 1e24,
        "femtometer" => 1e-9,
        "foot" => 304_800.0,
        "gigameter" => 1e15,
        "hectometer" => 1e8,
        "inch" => 25_400.0,
        "kilometer" => 1e9,
        "megameter" => 1e12,
        "meter" => 1e6,
        "micrometer" => 1.0,
        "mile" => 1_609_344_000.0,
        "millimeter" => 1e3,
        "nanometer" => 1e-3,
        "parsec" => 3.085_677_581_491_367e22,
        "petameter" => 1e21,
        "picometer" => 1e-6,
        "terameter" => 1e18,
        "yard" => 914_400.0,
        "yoctometer" => 1e-18,
        "yottameter" => 1e30,
        "zeptometer" => 1e-15,
        "zettameter" => 1e27,
        _ => return None,
    })
}

fn time_factor(unit: &str) -> Option<f64> {
    Some(match unit {
        "attosecond" => 1e-18,
        "centisecond" => 1e-2,
        "day" => 86_400.0,
        "decisecond" => 1e-1,
        "exasecond" => 1e18,
        "femtosecond" => 1e-15,
        "gigasecond" => 1e9,
        "hectosecond" => 1e2,
        "hour" => 3_600.0,
        "kilosecond" => 1e3,
        "megasecond" => 1e6,
        "microsecond" => 1e-6,
        "millisecond" => 1e-3,
        "minute" => 60.0,
        "nanosecond" => 1e-9,
        "petasecond" => 1e15,
        "picosecond" => 1e-12,
        "second" => 1.0,
        "terasecond" => 1e12,
        "yoctosecond" => 1e-24,
        "yottasecond" => 1e24,
        "zeptosecond" => 1e-21,
        "zettasecond" => 1e21,
        _ => return None,
    })
}

fn channels(path: &Path, omero: Option<&Value>) -> Result<Vec<ChannelMetadata>, Error> {
    let Some(channels) = omero
        .and_then(|omero| omero.get("channels"))
        .and_then(Value::as_array)
    else {
        return Ok(Vec::new());
    };
    channels
        .iter()
        .map(|channel| {
            let color = channel
                .get("color")
                .and_then(Value::as_str)
                .unwrap_or("FFFFFF");
            let tint = color_of(path, color)?;
            let window = channel
                .get("window")
                .and_then(Value::as_object)
                .map(|window| {
                    let value = |key: &str| {
                        window
                            .get(key)
                            .and_then(Value::as_f64)
                            .map(|value| value as f32)
                    };
                    (
                        valid_range(value("min"), value("max")),
                        valid_range(value("start"), value("end")),
                    )
                })
                .unwrap_or((None, None));
            Ok(ChannelMetadata {
                label: channel
                    .get("label")
                    .and_then(Value::as_str)
                    .filter(|label| !label.is_empty())
                    .map(str::to_owned),
                tint,
                data: window.0,
                window: window.1,
            })
        })
        .collect()
}

fn color_of(path: &Path, color: &str) -> Result<[f32; 3], Error> {
    let digits = color.trim_start_matches('#');
    if digits.len() != 6 || !digits.chars().all(|digit| digit.is_ascii_hexdigit()) {
        return Err(metadata_error(
            path,
            format!("the channel color `{color}` is not an rgb hex triple"),
        ));
    }
    let channel = |index: usize| {
        u8::from_str_radix(&digits[index..index + 2], 16).expect("the digits are hexadecimal")
            as f32
            / 255.0
    };
    Ok([channel(0), channel(2), channel(4)])
}

fn valid_range(min: Option<f32>, max: Option<f32>) -> Option<[f32; 2]> {
    let range = [min?, max?];
    (range[0].is_finite() && range[1].is_finite() && range[0] < range[1]).then_some(range)
}

fn metadata_error(path: &Path, detail: impl Into<String>) -> Error {
    Error::Metadata {
        path: path.to_path_buf(),
        detail: detail.into(),
    }
}
