use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Map, Value};
use zarrs::filesystem::FilesystemStore;
use zarrs::group::Group;

use crate::Error;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    path: String,
    name: String,
}

impl Field {
    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

#[derive(Clone, Debug)]
pub struct Plate {
    name: String,
    fields: Vec<Field>,
}

impl Plate {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn fields(&self) -> &[Field] {
        &self.fields
    }

    pub fn field(&self, index: usize) -> Option<&Field> {
        self.fields.get(index)
    }

    pub fn len(&self) -> usize {
        self.fields.len()
    }

    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Plate,
    Well,
    Image,
    Collection,
}

pub fn kind(path: &Path) -> Result<Kind, Error> {
    let attributes = attributes(path, Path::new("/"))?;
    let fields = fields(&attributes);
    Ok(if fields.get("plate").is_some() {
        Kind::Plate
    } else if fields.get("well").is_some() {
        Kind::Well
    } else if fields.get("multiscales").is_some() {
        Kind::Image
    } else {
        Kind::Collection
    })
}

pub fn open(path: &Path) -> Result<Plate, Error> {
    let attributes = attributes(path, Path::new("/"))?;
    let fields = fields(&attributes);
    let plate = fields
        .get("plate")
        .and_then(Value::as_object)
        .ok_or_else(|| Error::Metadata {
            path: path.to_path_buf(),
            detail: "the group carries no plate metadata".to_string(),
        })?;
    let rows = count(plate, "rows", path)?;
    let columns = count(plate, "columns", path)?;
    let mut wells = plate
        .get("wells")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Metadata {
            path: path.to_path_buf(),
            detail: "the plate metadata lists no wells".to_string(),
        })?
        .iter()
        .map(|well| {
            let detail = || Error::Metadata {
                path: path.to_path_buf(),
                detail: "a well names no path or index".to_string(),
            };
            Ok(WellSpec {
                path: PathBuf::from(
                    well.get("path")
                        .and_then(Value::as_str)
                        .ok_or_else(detail)?,
                ),
                row: index(well, "rowIndex").ok_or_else(detail)?,
                column: index(well, "columnIndex").ok_or_else(detail)?,
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;
    for well in &wells {
        if well.row >= rows || well.column >= columns {
            return Err(Error::Metadata {
                path: path.to_path_buf(),
                detail: format!(
                    "the well `{}` names the row {} and the column {} of a plate holding {rows} rows and {columns} columns",
                    well.path.display(),
                    well.row,
                    well.column
                ),
            });
        }
    }
    wells.sort_by(|left, right| {
        (left.row, left.column, &left.path).cmp(&(right.row, right.column, &right.path))
    });
    let mut fields = Vec::new();
    for well in &wells {
        for field in well_fields(path, well)? {
            fields.push(field);
        }
    }
    Ok(Plate {
        name: plate
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| dataset_name(path)),
        fields,
    })
}

struct WellSpec {
    path: PathBuf,
    row: usize,
    column: usize,
}

fn well_fields(path: &Path, well: &WellSpec) -> Result<Vec<Field>, Error> {
    let group = group_path(&well.path);
    let directory = path.join(&well.path);
    let attributes = attributes(path, Path::new(&group))?;
    let fields = fields(&attributes);
    let images = fields
        .get("well")
        .and_then(|well| well.get("images"))
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Metadata {
            path: directory.clone(),
            detail: "the well group carries no well metadata".to_string(),
        })?
        .iter()
        .map(|image| {
            image
                .get("path")
                .and_then(Value::as_str)
                .map(PathBuf::from)
                .ok_or_else(|| Error::Metadata {
                    path: directory.clone(),
                    detail: "a field of view names no path".to_string(),
                })
        })
        .collect::<Result<Vec<_>, Error>>()?;
    let mut fields = images
        .into_iter()
        .map(|field| {
            let joined = well.path.join(&field);
            Field {
                path: group_path(&joined),
                name: field_name(well.row, well.column, &field),
            }
        })
        .collect::<Vec<_>>();
    fields.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(fields)
}

fn count(plate: &Map<String, Value>, key: &str, path: &Path) -> Result<usize, Error> {
    plate
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::len)
        .ok_or_else(|| Error::Metadata {
            path: path.to_path_buf(),
            detail: format!("the plate metadata lists no {key}"),
        })
}

fn index(well: &Value, key: &str) -> Option<usize> {
    usize::try_from(well.get(key).and_then(Value::as_u64)?).ok()
}

fn field_name(row: usize, column: usize, field: &Path) -> String {
    let base = format!("{}{}", row_name(row), column_name(column));
    match relative(field) {
        Some(path) => format!("{base}/{path}"),
        None => base,
    }
}

fn relative(path: &Path) -> Option<String> {
    let parts = path
        .components()
        .filter_map(|part| part.as_os_str().to_str())
        .collect::<Vec<_>>();
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn row_name(row: usize) -> String {
    let mut name = String::new();
    let mut value = row;
    loop {
        name.insert(0, (b'A' + (value % 26) as u8) as char);
        if value < 26 {
            break;
        }
        value = value / 26 - 1;
    }
    name
}

fn column_name(column: usize) -> String {
    (column + 1).to_string()
}

fn parts(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|part| match part {
            std::path::Component::Normal(name) => name.to_str().map(str::to_string),
            _ => None,
        })
        .collect()
}

pub fn group_path(path: &Path) -> String {
    let parts = parts(path);
    if parts.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", parts.join("/"))
    }
}

pub fn join(group: &str, path: &str) -> String {
    let path = path.trim_start_matches('/');
    let group = group.trim_end_matches('/');
    if group.is_empty() {
        format!("/{path}")
    } else {
        format!("{group}/{path}")
    }
}

fn attributes(path: &Path, group: &Path) -> Result<Map<String, Value>, Error> {
    let store = Arc::new(FilesystemStore::new(path).map_err(|error| Error::Open {
        path: path.to_path_buf(),
        detail: error.to_string(),
    })?);
    let opened = Group::open(store, &group_path(group)).map_err(|error| Error::Open {
        path: path.join(group),
        detail: error.to_string(),
    })?;
    Ok(opened.attributes().clone())
}

fn fields(attributes: &Map<String, Value>) -> &Map<String, Value> {
    attributes
        .get("ome")
        .and_then(Value::as_object)
        .unwrap_or(attributes)
}

fn dataset_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| {
            name.trim_end_matches(".zarr")
                .trim_end_matches(".ome")
                .to_string()
        })
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "plate".to_string())
}
