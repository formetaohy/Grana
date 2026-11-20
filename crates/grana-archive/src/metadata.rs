use grana_volume::{Calibration, Extent, Level, Spacing};

pub const ANNOTATION: &str = "grana";
pub const MARKER: &str = "1";

const LABELS: [&str; 4] = ["recorded", "inferred", "assumed", "uncalibrated"];

pub fn calibration_name(calibration: Calibration) -> &'static str {
    LABELS[calibration.index() as usize]
}

pub fn calibration_of(name: &str) -> Option<Calibration> {
    LABELS
        .iter()
        .position(|label| *label == name)
        .and_then(|index| Calibration::from_index(index as u8))
}

pub fn annotations(levels: &[Level], calibration: Calibration) -> Vec<(String, String)> {
    let mut entries = vec![
        (ANNOTATION.to_string(), MARKER.to_string()),
        (
            "calibration".to_string(),
            calibration_name(calibration).to_string(),
        ),
        ("levels".to_string(), levels.len().to_string()),
    ];
    for (index, level) in levels.iter().enumerate() {
        let extent = level.extent();
        let spacing = level.spacing();
        entries.push((
            format!("level.{index}.extent"),
            format!("{},{},{}", extent.x(), extent.y(), extent.z()),
        ));
        entries.push((
            format!("level.{index}.spacing"),
            format!("{},{},{}", spacing.x(), spacing.y(), spacing.z()),
        ));
    }
    entries
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

pub fn spacings_from<'a>(
    lookup: impl Fn(&str) -> Option<&'a str>,
    resolved: &[Extent],
) -> Result<Option<Vec<Spacing>>, String> {
    let Some(count) = lookup("levels") else {
        return Ok(None);
    };
    let count = count
        .parse::<usize>()
        .map_err(|error| format!("the archive levels are invalid: {error}"))?;
    if count != resolved.len() {
        return Err(format!(
            "the archive describes {count} levels and its pages hold {}",
            resolved.len()
        ));
    }
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
        let spacing = numbers(&lookup, &format!("level.{index}.spacing"))?;
        if !spacing.iter().all(|value| *value > 0.0) {
            return Err(format!(
                "the archive level {index} carries a spacing that is not positive"
            ));
        }
        spacings.push(Spacing::new(spacing[0], spacing[1], spacing[2]));
    }
    Ok(Some(spacings))
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

fn numbers<'a>(lookup: &impl Fn(&str) -> Option<&'a str>, key: &str) -> Result<[f32; 3], String> {
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
    let values: [f32; 3] = values
        .try_into()
        .map_err(|_| format!("the archive {key} holds the wrong number of axes"))?;
    if !values.iter().all(|value| value.is_finite()) {
        return Err(format!("the archive {key} is not finite"));
    }
    Ok(values)
}
