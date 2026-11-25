use grana_volume::{Basis, Calibration, ColorTable, Extent, Level, Spacing};
use serde_json::{Map, Value, json};

pub const ANNOTATION: &str = "grana";

pub const COLOR_TABLES: &str = "channels";

const MARKER: &str = "1";

const CHANNEL: &str = "channel";

const COLORS: &str = "colors";

const LABELS: [&str; 5] = [
    "recorded",
    "inferred",
    "assumed",
    "declared",
    "uncalibrated",
];
const BASIS_LENGTH: usize = 9;
const ALIGNMENT_TOLERANCE: f32 = 1.0e-6;

pub fn calibration_name(calibration: Calibration) -> &'static str {
    LABELS[calibration.index() as usize]
}

pub fn calibration_of(name: &str) -> Option<Calibration> {
    LABELS
        .iter()
        .position(|label| *label == name)
        .and_then(|index| Calibration::from_index(index as u8))
}

pub fn annotation_values(calibration: Calibration) -> Map<String, Value> {
    let mut values = Map::new();
    values.insert(
        "calibration".to_string(),
        Value::String(calibration_name(calibration).to_string()),
    );
    values
}

pub fn color_table_values(tables: &[Option<&ColorTable>]) -> Value {
    let channels = tables
        .iter()
        .enumerate()
        .filter_map(|(channel, table)| {
            let table = (*table)?;
            let colors = table
                .colors()
                .iter()
                .map(|color| Value::Array(color.iter().map(|component| srgb(*component)).collect()))
                .collect::<Vec<_>>();
            Some(json!({ CHANNEL: channel, COLORS: colors }))
        })
        .collect::<Vec<_>>();
    Value::Array(channels)
}

pub fn color_tables_from(value: Option<&Value>) -> Result<Vec<(usize, ColorTable)>, String> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let entries = value
        .as_array()
        .ok_or_else(|| "the archive colour tables are not a list".to_string())?;
    let mut tables: Vec<(usize, ColorTable)> = Vec::with_capacity(entries.len());
    for entry in entries {
        let channel = entry
            .get(CHANNEL)
            .and_then(Value::as_u64)
            .and_then(|channel| usize::try_from(channel).ok())
            .ok_or_else(|| "a colour table names no channel".to_string())?;
        if tables.iter().any(|(held, _)| *held == channel) {
            return Err(format!(
                "the archive names the colour table of channel {channel} twice"
            ));
        }
        let colors = entry
            .get(COLORS)
            .and_then(Value::as_array)
            .ok_or_else(|| format!("the colour table of channel {channel} holds no colours"))?;
        let mut table = Vec::with_capacity(colors.len());
        for color in colors {
            let components = color
                .as_array()
                .filter(|components| components.len() == 3)
                .ok_or_else(|| format!("a colour of channel {channel} is not an srgb triple"))?;
            let mut held = [0.0f32; 3];
            for (value, component) in held.iter_mut().zip(components) {
                *value = srgb_of(component).ok_or_else(|| {
                    format!("a colour of channel {channel} is not a sixteen bit srgb triple")
                })?;
            }
            table.push(held);
        }
        if !(2..=ColorTable::LIMIT).contains(&table.len()) {
            return Err(format!(
                "the colour table of channel {channel} holds {} colours",
                table.len()
            ));
        }
        tables.push((channel, ColorTable::new(table)));
    }
    tables.sort_by_key(|(channel, _)| *channel);
    Ok(tables)
}

fn srgb(component: f32) -> Value {
    let scaled = (component.clamp(0.0, 1.0) * f32::from(u16::MAX)).round() as u16;
    Value::from(scaled)
}

fn srgb_of(value: &Value) -> Option<f32> {
    let scaled = value
        .as_u64()
        .filter(|value| *value <= u64::from(u16::MAX))?;
    Some(scaled as f32 / f32::from(u16::MAX))
}

pub fn calibration_from<'a>(
    lookup: impl Fn(&str) -> Option<&'a str>,
) -> Result<Option<Calibration>, String> {
    let Some(name) = lookup("calibration") else {
        return Ok(None);
    };
    calibration_of(name)
        .map(Some)
        .ok_or_else(|| format!("the archive names the unknown calibration `{name}`"))
}

pub fn annotations(
    levels: &[Level],
    placement: [f32; 3],
    calibration: Calibration,
) -> Vec<(String, String)> {
    let mut entries = vec![
        (ANNOTATION.to_string(), MARKER.to_string()),
        (
            "calibration".to_string(),
            calibration_name(calibration).to_string(),
        ),
        ("levels".to_string(), levels.len().to_string()),
        (
            "placement".to_string(),
            placement
                .iter()
                .map(f32::to_string)
                .collect::<Vec<_>>()
                .join(","),
        ),
    ];
    for (index, level) in levels.iter().enumerate() {
        let extent = level.extent();
        entries.push((
            format!("level.{index}.extent"),
            format!("{},{},{}", extent.x(), extent.y(), extent.z()),
        ));
        if calibration.measured() {
            let spacing = level.spacing();
            entries.push((
                format!("level.{index}.spacing"),
                format!("{},{},{}", spacing.x(), spacing.y(), spacing.z()),
            ));
        } else {
            entries.push((format!("level.{index}.relative"), factor(levels[0], *level)));
        }
        let basis = level.basis();
        let columns = [0, 1, 2].map(|axis| basis.column(axis));
        entries.push((
            format!("level.{index}.basis"),
            columns
                .iter()
                .flatten()
                .map(f32::to_string)
                .collect::<Vec<_>>()
                .join(","),
        ));
        entries.push((
            format!("level.{index}.origin"),
            level
                .origin()
                .iter()
                .map(f32::to_string)
                .collect::<Vec<_>>()
                .join(","),
        ));
    }
    entries
}

fn factor(finest: Level, level: Level) -> String {
    let base = finest.voxel();
    let held = level.voxel();
    [0, 1, 2]
        .map(|axis| held[axis] / base[axis])
        .iter()
        .map(f32::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

pub struct Geometry {
    levels: Vec<Level>,
    placement: [f32; 3],
}

impl Geometry {
    pub fn levels(&self) -> &[Level] {
        &self.levels
    }

    pub fn placement(&self) -> [f32; 3] {
        self.placement
    }
}

pub fn levels_from<'a>(
    lookup: impl Fn(&str) -> Option<&'a str>,
    resolved: &[Extent],
) -> Result<Option<Geometry>, String> {
    let Some(count) = level_count(&lookup, resolved.len())? else {
        return Ok(None);
    };
    if lookup("level.0.basis").is_none() {
        return Ok(None);
    }
    let placement = values(&lookup, "placement", 3)?;
    let placement = [placement[0], placement[1], placement[2]];
    let mut levels = Vec::with_capacity(count);
    for (index, held) in resolved.iter().enumerate() {
        let extent = counts(&lookup, &format!("level.{index}.extent"))?;
        let extent = Extent::new(extent[0], extent[1], extent[2]);
        if extent != *held {
            return Err(format!(
                "the archive level {index} holds {}x{}x{} and its pages hold {}x{}x{}",
                extent.x(),
                extent.y(),
                extent.z(),
                held.x(),
                held.y(),
                held.z()
            ));
        }
        let basis = values(&lookup, &format!("level.{index}.basis"), BASIS_LENGTH)?;
        let origin = values(&lookup, &format!("level.{index}.origin"), 3)?;
        let columns = [
            [basis[0], basis[1], basis[2]],
            [basis[3], basis[4], basis[5]],
            [basis[6], basis[7], basis[8]],
        ];
        check_basis(index, &columns)?;
        levels.push(Level::placed(
            extent,
            Basis::new(columns),
            [origin[0], origin[1], origin[2]],
        ));
    }
    Ok(Some(Geometry { levels, placement }))
}

pub fn spacings_from<'a>(
    lookup: impl Fn(&str) -> Option<&'a str>,
    resolved: &[Extent],
) -> Result<Option<Vec<Spacing>>, String> {
    let Some(count) = level_count(&lookup, resolved.len())? else {
        return Ok(None);
    };
    let mut spacings = Vec::with_capacity(count);
    for (index, held) in resolved.iter().enumerate() {
        let extent = counts(&lookup, &format!("level.{index}.extent"))?;
        let extent = Extent::new(extent[0], extent[1], extent[2]);
        if extent != *held {
            return Err(format!(
                "the archive level {index} holds {}x{}x{} and its pages hold {}x{}x{}",
                extent.x(),
                extent.y(),
                extent.z(),
                held.x(),
                held.y(),
                held.z()
            ));
        }
        let spacing = values(&lookup, &format!("level.{index}.spacing"), 3)?;
        if !spacing.iter().all(|value| *value > 0.0) {
            return Err(format!(
                "the archive level {index} carries a spacing that is not positive"
            ));
        }
        spacings.push(Spacing::new(spacing[0], spacing[1], spacing[2]));
    }
    Ok(Some(spacings))
}

pub fn relatives_from<'a>(
    lookup: impl Fn(&str) -> Option<&'a str>,
    resolved: &[Extent],
) -> Result<Option<Vec<[f32; 3]>>, String> {
    let Some(count) = level_count(&lookup, resolved.len())? else {
        return Ok(None);
    };
    let mut factors = Vec::with_capacity(count);
    for index in 0..count {
        let relative = values(&lookup, &format!("level.{index}.relative"), 3)?;
        if index == 0 && relative != [1.0, 1.0, 1.0] {
            return Err(
                "the archive scales its finest level away from one against itself".to_string(),
            );
        }
        if !relative.iter().all(|value| *value > 0.0) {
            return Err(format!(
                "the archive level {index} carries a downsample factor that is not positive"
            ));
        }
        factors.push([relative[0], relative[1], relative[2]]);
    }
    Ok(Some(factors))
}

fn level_count<'a>(
    lookup: &impl Fn(&str) -> Option<&'a str>,
    levels: usize,
) -> Result<Option<usize>, String> {
    let Some(count) = lookup("levels") else {
        return Ok(None);
    };
    let count = count
        .parse::<usize>()
        .map_err(|error| format!("the archive levels are invalid: {error}"))?;
    if count != levels {
        return Err(format!(
            "the archive describes {count} levels and its pages hold {levels}"
        ));
    }
    Ok(Some(count))
}

fn check_basis(index: usize, columns: &[[f32; 3]; 3]) -> Result<(), String> {
    if columns.iter().flatten().any(|value| !value.is_finite()) {
        return Err(format!(
            "the archive level {index} carries a basis that is not finite"
        ));
    }
    let cross = |left: [f32; 3], right: [f32; 3]| {
        [
            left[1] * right[2] - left[2] * right[1],
            left[2] * right[0] - left[0] * right[2],
            left[0] * right[1] - left[1] * right[0],
        ]
    };
    let normal = cross(columns[1], columns[2]);
    let determinant =
        columns[0][0] * normal[0] + columns[0][1] * normal[1] + columns[0][2] * normal[2];
    let length = |column: [f32; 3]| {
        (column[0] * column[0] + column[1] * column[1] + column[2] * column[2]).sqrt()
    };
    let volume = length(columns[0]) * length(columns[1]) * length(columns[2]);
    if !(determinant.is_finite()
        && volume.is_finite()
        && volume > 0.0
        && determinant.abs() > ALIGNMENT_TOLERANCE * volume)
    {
        return Err(format!(
            "the archive level {index} carries a basis that does not map voxels bijectively"
        ));
    }
    Ok(())
}

fn counts<'a>(lookup: &impl Fn(&str) -> Option<&'a str>, key: &str) -> Result<[u32; 3], String> {
    let text = lookup(key).ok_or_else(|| format!("the archive holds no {key}"))?;
    let values = text
        .split(',')
        .map(|value| {
            value
                .trim()
                .parse::<u32>()
                .map_err(|error| format!("the archive {key} is invalid: {error}"))
        })
        .collect::<Result<Vec<_>, String>>()?;
    values
        .try_into()
        .map_err(|_| format!("the archive {key} holds the wrong number of axes"))
}

fn values<'a>(
    lookup: &impl Fn(&str) -> Option<&'a str>,
    key: &str,
    count: usize,
) -> Result<Vec<f32>, String> {
    let text = lookup(key).ok_or_else(|| format!("the archive holds no {key}"))?;
    let values = text
        .split(',')
        .map(|value| {
            value
                .trim()
                .parse::<f32>()
                .map_err(|error| format!("the archive {key} is invalid: {error}"))
        })
        .collect::<Result<Vec<_>, String>>()?;
    if values.len() != count {
        return Err(format!(
            "the archive {key} holds the wrong number of values"
        ));
    }
    if values.iter().any(|value| !value.is_finite()) {
        return Err(format!("the archive {key} is not finite"));
    }
    Ok(values)
}
