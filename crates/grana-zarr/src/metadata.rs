use serde_json::{Map, Value};

use crate::Error;
use crate::transform::{self, Affine};

const DEPENDENCE_TOLERANCE: f64 = 1.0e-9;

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
    pub columns: [[f64; 3]; 3],
    pub origin: [f64; 3],
}

#[derive(Clone, Debug)]
pub struct ChannelMetadata {
    pub label: Option<String>,
    pub tint: [f32; 3],
    pub data: Option<[f32; 2]>,
    pub window: Option<[f32; 2]>,
}

#[derive(Clone, Debug)]
struct System {
    name: String,
    axes: Vec<Axis>,
}

pub fn parse(path: &str, attributes: &Map<String, Value>) -> Result<Metadata, Error> {
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
    let (axes, datasets) = if minor >= 6 {
        named(path, multiscales)?
    } else {
        direct(path, multiscales)?
    };
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

fn minor_version(path: &str, version: &str) -> Result<u32, Error> {
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

fn absent(path: &str, attributes: &Map<String, Value>, fields: &Map<String, Value>) -> Error {
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

fn direct(path: &str, multiscale: &Value) -> Result<(Vec<Axis>, Vec<Dataset>), Error> {
    let axes = axes_of(path, multiscale.get("axes"))?;
    let dimensions = axes.len();
    let global = multiscale
        .get("coordinateTransformations")
        .map(|value| transforms(path, value, dimensions, dimensions))
        .transpose()?
        .unwrap_or_else(|| Affine::identity(dimensions));
    let datasets = entries(path, multiscale)?
        .iter()
        .map(|entry| {
            let name = path_of(path, entry)?;
            let local = entry
                .get("coordinateTransformations")
                .map(|value| transforms(path, value, dimensions, dimensions))
                .transpose()?
                .unwrap_or_else(|| Affine::identity(dimensions));
            let total = global.composite(&local).map_err(|error| {
                metadata_error(
                    path,
                    format!("the transforms of `{name}` do not compose: {error}"),
                )
            })?;
            compose(path, name, &total, &axes, &axes, false, None)
        })
        .collect::<Result<Vec<_>, Error>>()?;
    Ok((axes, datasets))
}

fn named(path: &str, multiscale: &Value) -> Result<(Vec<Axis>, Vec<Dataset>), Error> {
    let systems = systems(path, multiscale)?;
    let entries = entries(path, multiscale)?;
    let intrinsic = entries
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
    let source = systems
        .iter()
        .find(|system| system.name == intrinsic)
        .ok_or_else(|| {
            metadata_error(
                path,
                format!("the coordinate system `{intrinsic}` is not declared"),
            )
        })?;
    let axes = source.axes.clone();
    let dimensions = axes.len();
    let display = display(path, multiscale, &systems, intrinsic)?;
    let datasets = entries
        .iter()
        .map(|entry| {
            let name = path_of(path, entry)?;
            let local = transforms(
                path,
                entry.get("coordinateTransformations").ok_or_else(|| {
                    metadata_error(
                        path,
                        format!("the dataset `{name}` names no coordinate transformation"),
                    )
                })?,
                dimensions,
                dimensions,
            )?;
            match &display {
                Some((affine, target)) => {
                    let total = affine.composite(&local).map_err(|error| {
                        metadata_error(
                            path,
                            format!("the transforms of `{name}` do not compose: {error}"),
                        )
                    })?;
                    compose(path, name, &total, &axes, &target.axes, true, Some(entry))
                }
                None => compose(path, name, &local, &axes, &axes, false, Some(entry)),
            }
        })
        .collect::<Result<Vec<_>, Error>>()?;
    Ok((axes, datasets))
}

fn compose(
    path: &str,
    name: &str,
    total: &Affine,
    source: &[Axis],
    target: &[Axis],
    display: bool,
    entry: Option<&Value>,
) -> Result<Dataset, Error> {
    if total.columns() != source.len() {
        return Err(metadata_error(
            path,
            format!(
                "the transform of `{name}` covers {} of {} axes",
                total.columns(),
                source.len()
            ),
        ));
    }
    let source_axes = space_axes(path, source)?;
    let target_axes = space_axes(path, target)?;
    let stored = source_axes.iter().filter(|axis| axis.is_some()).count();
    let shown = target_axes.iter().filter(|axis| axis.is_some()).count();
    if !(2..=3).contains(&stored) || shown < stored {
        return Err(Error::Unsupported {
            path: path.to_string(),
            detail: format!("the coordinate system of `{name}` does not carry its spatial axes"),
        });
    }
    for axis in 0..total.columns() {
        if source_axes.iter().flatten().any(|held| *held == axis) {
            continue;
        }
        for row in target_axes.iter().flatten() {
            if total.linear(*row, axis).abs() > DEPENDENCE_TOLERANCE {
                return Err(Error::Unsupported {
                    path: path.to_string(),
                    detail: format!("the placement of `{name}` depends on a time or channel axis"),
                });
            }
        }
    }
    let mut columns = [[0.0; 3]; 3];
    let mut origin = [0.0; 3];
    for component in 0..3 {
        let Some(row) = target_axes[component] else {
            continue;
        };
        let factor = if display {
            target[row].factor()
        } else {
            source[row].factor()
        };
        origin[component] = total.offset(row) * factor;
        for axis in 0..3 {
            let Some(input) = source_axes[axis] else {
                continue;
            };
            columns[axis][component] = total.linear(row, input) * factor * source[input].factor();
        }
    }
    if stored == 2 {
        let spacing = (0..3)
            .map(|component| columns[0][component].abs().max(columns[1][component].abs()))
            .filter(|value| *value > 0.0)
            .fold(f64::INFINITY, f64::min);
        let spacing = if spacing.is_finite() { spacing } else { 1.0 };
        columns[2] = [0.0, 0.0, spacing];
        origin[2] = 0.0;
    }
    let determinant = determinant(&columns);
    let volume = length(&columns[0]) * length(&columns[1]) * length(&columns[2]);
    if !determinant.is_finite() || determinant.abs() <= 1.0e-9 * volume || volume <= 0.0 {
        return Err(Error::Unsupported {
            path: path.to_string(),
            detail: format!("the placement of `{name}` is not invertible"),
        });
    }
    let (scale, translation) = match entry {
        Some(entry) => components(entry, source.len(), total),
        None => (
            (0..source.len())
                .map(|axis| total.linear(axis, axis))
                .collect(),
            (0..source.len()).map(|axis| total.offset(axis)).collect(),
        ),
    };
    Ok(Dataset {
        path: name.to_string(),
        scale,
        translation,
        columns,
        origin,
    })
}

fn space_axes(path: &str, axes: &[Axis]) -> Result<[Option<usize>; 3], Error> {
    let mut found = [None; 3];
    for (slot, axis) in axes.iter().enumerate() {
        if axis.kind() != AxisKind::Space {
            continue;
        }
        let index = match axis.name().to_ascii_lowercase().as_str() {
            "x" => 0,
            "y" => 1,
            "z" => 2,
            other => {
                return Err(Error::Unsupported {
                    path: path.to_string(),
                    detail: format!("the spatial axis `{other}` is not named x, y or z"),
                });
            }
        };
        if found[index].is_some() {
            return Err(metadata_error(
                path,
                format!("the coordinate system names the axis `{index}` twice"),
            ));
        }
        found[index] = Some(slot);
    }
    Ok(found)
}

fn components(entry: &Value, dimensions: usize, total: &Affine) -> (Vec<f64>, Vec<f64>) {
    let mut scale = None;
    let mut translation = None;
    if let Some(transforms) = entry
        .get("coordinateTransformations")
        .and_then(Value::as_array)
    {
        for transform in transforms {
            collect(transform, &mut scale, &mut translation);
        }
    }
    let scale = scale
        .filter(|values| values.len() == dimensions)
        .unwrap_or_else(|| {
            (0..dimensions)
                .map(|axis| total.linear(axis, axis))
                .collect()
        });
    let translation = translation
        .filter(|values| values.len() == dimensions)
        .unwrap_or_else(|| (0..dimensions).map(|axis| total.offset(axis)).collect());
    (scale, translation)
}

fn collect(transform: &Value, scale: &mut Option<Vec<f64>>, translation: &mut Option<Vec<f64>>) {
    match transform.get("type").and_then(Value::as_str) {
        Some("scale") => {
            if let Some(values) = vector(transform, "scale") {
                *scale = Some(values);
            }
        }
        Some("translation") => {
            if let Some(values) = vector(transform, "translation") {
                *translation = Some(values);
            }
        }
        Some("sequence") => {
            if let Some(entries) = transform.get("transformations").and_then(Value::as_array) {
                for entry in entries {
                    collect(entry, scale, translation);
                }
            }
        }
        _ => {}
    }
}

fn vector(value: &Value, key: &str) -> Option<Vec<f64>> {
    let values = value.get(key)?.as_array()?;
    values
        .iter()
        .map(Value::as_f64)
        .collect::<Option<Vec<f64>>>()
        .filter(|values| values.iter().all(|value| value.is_finite()))
}

fn determinant(columns: &[[f64; 3]; 3]) -> f64 {
    let [x, y, z] = columns;
    x[0] * (y[1] * z[2] - y[2] * z[1]) - y[0] * (x[1] * z[2] - x[2] * z[1])
        + z[0] * (x[1] * y[2] - x[2] * y[1])
}

fn length(vector: &[f64; 3]) -> f64 {
    vector.iter().map(|value| value * value).sum::<f64>().sqrt()
}

fn systems(path: &str, multiscale: &Value) -> Result<Vec<System>, Error> {
    let systems = multiscale
        .get("coordinateSystems")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            metadata_error(path, "the multiscales metadata names no coordinate system")
        })?;
    systems
        .iter()
        .map(|system| {
            let name = system
                .get("name")
                .and_then(Value::as_str)
                .filter(|name| !name.is_empty())
                .ok_or_else(|| metadata_error(path, "a coordinate system names no name"))?;
            let axes = axes_of(path, system.get("axes"))?;
            Ok(System {
                name: name.to_string(),
                axes,
            })
        })
        .collect()
}

fn entries<'a>(path: &str, multiscale: &'a Value) -> Result<&'a Vec<Value>, Error> {
    let datasets = multiscale
        .get("datasets")
        .and_then(Value::as_array)
        .ok_or_else(|| metadata_error(path, "the multiscales metadata lists no dataset"))?;
    if datasets.is_empty() {
        return Err(metadata_error(path, "the multiscales list has no datasets"));
    }
    Ok(datasets)
}

fn path_of<'a>(path: &str, entry: &'a Value) -> Result<&'a str, Error> {
    entry
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| metadata_error(path, "a dataset names no array path"))
}

fn transforms(path: &str, value: &Value, input: usize, output: usize) -> Result<Affine, Error> {
    match value {
        Value::Array(transforms) => {
            let mut total = Affine::identity(input);
            for entry in transforms {
                let inner = transform::parse(path, entry, input, output)?;
                total = inner
                    .composite(&total)
                    .map_err(|error| metadata_error(path, error))?;
            }
            Ok(total)
        }
        Value::Object(_) => transform::parse(path, value, input, output),
        _ => Err(metadata_error(
            path,
            "a coordinate transformation is not a json object or array",
        )),
    }
}

fn display(
    path: &str,
    multiscale: &Value,
    systems: &[System],
    intrinsic: &str,
) -> Result<Option<(Affine, System)>, Error> {
    let Some(transforms) = multiscale
        .get("coordinateTransformations")
        .and_then(Value::as_array)
    else {
        return Ok(None);
    };
    let source = systems
        .iter()
        .find(|system| system.name == intrinsic)
        .expect("the intrinsic coordinate system is declared");
    for transform in transforms {
        let input = endpoint(path, transform.get("input"))?;
        let output = endpoint(path, transform.get("output"))?;
        let (name, invert) = match (&input, &output) {
            (Some(input), Some(output)) if input == intrinsic && output != intrinsic => {
                (output, false)
            }
            (Some(input), Some(output)) if output == intrinsic && input != intrinsic => {
                (input, true)
            }
            _ => continue,
        };
        let target = systems
            .iter()
            .find(|system| &system.name == name)
            .ok_or_else(|| {
                metadata_error(
                    path,
                    format!("the coordinate system `{name}` is not declared"),
                )
            })?;
        let spaces = target
            .axes
            .iter()
            .filter(|axis| axis.kind() == AxisKind::Space)
            .count();
        if !(2..=3).contains(&spaces) || target.axes.len() != spaces {
            continue;
        }
        let (input_dimensions, output_dimensions) = if invert {
            (target.axes.len(), source.axes.len())
        } else {
            (source.axes.len(), target.axes.len())
        };
        let affine = transform::parse(path, transform, input_dimensions, output_dimensions)?;
        let affine = if invert {
            affine.inverse().map_err(|error| Error::Unsupported {
                path: path.to_string(),
                detail: format!("the display transform is not invertible: {error}"),
            })?
        } else {
            affine
        };
        return Ok(Some((affine, target.clone())));
    }
    Ok(None)
}

fn endpoint(path: &str, value: Option<&Value>) -> Result<Option<String>, Error> {
    let Some(value) = value else {
        return Ok(None);
    };
    let Some(object) = value.as_object() else {
        return Err(metadata_error(
            path,
            "a transform endpoint is not an object",
        ));
    };
    if object
        .get("path")
        .is_some_and(|value| !value.is_null() && value.as_str() != Some(""))
    {
        return Ok(None);
    }
    Ok(object
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_owned))
}

fn axes_of(path: &str, value: Option<&Value>) -> Result<Vec<Axis>, Error> {
    let axes = value
        .and_then(Value::as_array)
        .ok_or_else(|| metadata_error(path, "the metadata lists no axes"))?;
    axes.iter()
        .map(|axis| axis_of(path, axis))
        .collect::<Result<Vec<_>, Error>>()
}

fn axis_of(path: &str, value: &Value) -> Result<Axis, Error> {
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

fn unit_of(path: &str, kind: AxisKind, unit: &str) -> Result<AxisUnit, Error> {
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

fn channels(path: &str, omero: Option<&Value>) -> Result<Vec<ChannelMetadata>, Error> {
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

fn color_of(path: &str, color: &str) -> Result<[f32; 3], Error> {
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

fn metadata_error(path: &str, detail: impl Into<String>) -> Error {
    Error::Metadata {
        path: path.to_string(),
        detail: detail.into(),
    }
}
