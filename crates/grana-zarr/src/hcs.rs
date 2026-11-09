use std::path::{Path, PathBuf};

use serde_json::{Map, Value};
use zarrs::group::Group;

use crate::Error;
use crate::root::Root;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    path: String,
    name: String,
    series: usize,
    row: usize,
    column: usize,
    index: usize,
}

impl Field {
    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn series(&self) -> usize {
        self.series
    }

    pub fn row(&self) -> usize {
        self.row
    }

    pub fn column(&self) -> usize {
        self.column
    }

    pub fn index(&self) -> usize {
        self.index
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Well {
    label: String,
    row: usize,
    column: usize,
    fields: Vec<Field>,
}

impl Well {
    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn row(&self) -> usize {
        self.row
    }

    pub fn column(&self) -> usize {
        self.column
    }

    pub fn fields(&self) -> &[Field] {
        &self.fields
    }

    pub fn field(&self, index: usize) -> Option<&Field> {
        self.fields.get(index)
    }
}

#[derive(Clone, Debug)]
pub struct Plate {
    name: String,
    rows: Vec<String>,
    columns: Vec<String>,
    wells: Vec<Well>,
    cells: Vec<Option<usize>>,
    fields: Vec<Field>,
}

impl Plate {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn rows(&self) -> usize {
        self.rows.len()
    }

    pub fn columns(&self) -> usize {
        self.columns.len()
    }

    pub fn row_name(&self, row: usize) -> Option<&str> {
        self.rows.get(row).map(String::as_str)
    }

    pub fn column_name(&self, column: usize) -> Option<&str> {
        self.columns.get(column).map(String::as_str)
    }

    pub fn wells(&self) -> &[Well] {
        &self.wells
    }

    pub fn well(&self, row: usize, column: usize) -> Option<&Well> {
        if row >= self.rows.len() || column >= self.columns.len() {
            return None;
        }
        self.cells[row * self.columns.len() + column].map(|index| &self.wells[index])
    }

    pub fn fields(&self) -> &[Field] {
        &self.fields
    }

    pub fn field(&self, index: usize) -> Option<&Field> {
        self.fields.get(index)
    }

    pub fn well_of(&self, series: usize) -> Option<&Well> {
        let field = self.fields.get(series)?;
        self.well(field.row, field.column)
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

pub fn kind(root: &Root) -> Result<Kind, Error> {
    let attributes = attributes(root, "/")?;
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

pub fn open(root: &Root) -> Result<Plate, Error> {
    let origin = root.tag("/");
    let attributes = attributes(root, "/")?;
    let fields = fields(&attributes);
    let plate = fields
        .get("plate")
        .and_then(Value::as_object)
        .ok_or_else(|| Error::Metadata {
            path: origin.clone(),
            detail: "the group carries no plate metadata".to_string(),
        })?;
    let rows = names(plate, "rows", &origin)?;
    let columns = names(plate, "columns", &origin)?;
    let mut wells = plate
        .get("wells")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Metadata {
            path: origin.clone(),
            detail: "the plate metadata lists no wells".to_string(),
        })?
        .iter()
        .map(|well| {
            let detail = || Error::Metadata {
                path: origin.clone(),
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
        if well.row >= rows.len() || well.column >= columns.len() {
            return Err(Error::Metadata {
                path: origin.clone(),
                detail: format!(
                    "the well `{}` names the row {} and the column {} of a plate holding {} rows and {} columns",
                    well.path.display(),
                    well.row,
                    well.column,
                    rows.len(),
                    columns.len()
                ),
            });
        }
    }
    wells.sort_by(|left, right| {
        (left.row, left.column, &left.path).cmp(&(right.row, right.column, &right.path))
    });
    let mut collected = Vec::with_capacity(wells.len());
    let mut flat = Vec::new();
    for spec in &wells {
        let label = format!("{}{}", rows[spec.row], columns[spec.column]);
        let mut fields = well_fields(root, spec, &label)?;
        fields.sort_by(|left, right| natural(&left.path, &right.path));
        for (index, field) in fields.iter_mut().enumerate() {
            field.series = flat.len();
            field.index = index;
            flat.push(field.clone());
        }
        collected.push(Well {
            label,
            row: spec.row,
            column: spec.column,
            fields,
        });
    }
    let mut cells = vec![None; rows.len() * columns.len()];
    for (index, well) in collected.iter().enumerate() {
        cells[well.row * columns.len() + well.column].get_or_insert(index);
    }
    Ok(Plate {
        name: plate
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| root.name()),
        rows,
        columns,
        wells: collected,
        cells,
        fields: flat,
    })
}

struct WellSpec {
    path: PathBuf,
    row: usize,
    column: usize,
}

fn well_fields(root: &Root, well: &WellSpec, label: &str) -> Result<Vec<Field>, Error> {
    let group = group_path(&well.path);
    let attributes = attributes(root, &group)?;
    let fields = fields(&attributes);
    let images = fields
        .get("well")
        .and_then(|well| well.get("images"))
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Metadata {
            path: root.tag(&group),
            detail: "the well group carries no well metadata".to_string(),
        })?
        .iter()
        .map(|image| {
            image
                .get("path")
                .and_then(Value::as_str)
                .map(PathBuf::from)
                .ok_or_else(|| Error::Metadata {
                    path: root.tag(&group),
                    detail: "a field of view names no path".to_string(),
                })
        })
        .collect::<Result<Vec<_>, Error>>()?;
    Ok(images
        .into_iter()
        .map(|field| {
            let joined = well.path.join(&field);
            Field {
                name: field_name(label, &field),
                path: group_path(&joined),
                series: 0,
                row: well.row,
                column: well.column,
                index: 0,
            }
        })
        .collect())
}

fn natural(left: &str, right: &str) -> std::cmp::Ordering {
    let left = left.as_bytes();
    let right = right.as_bytes();
    let mut left_at = 0;
    let mut right_at = 0;
    while left_at < left.len() && right_at < right.len() {
        if left[left_at].is_ascii_digit() && right[right_at].is_ascii_digit() {
            let left_end = (left_at..left.len())
                .find(|index| !left[*index].is_ascii_digit())
                .unwrap_or(left.len());
            let right_end = (right_at..right.len())
                .find(|index| !right[*index].is_ascii_digit())
                .unwrap_or(right.len());
            let left_number = &left[left_at..left_end];
            let right_number = &right[right_at..right_end];
            let left_digits = left_number
                .iter()
                .position(|digit| *digit != b'0')
                .map_or(left_number, |offset| &left_number[offset..]);
            let right_digits = right_number
                .iter()
                .position(|digit| *digit != b'0')
                .map_or(right_number, |offset| &right_number[offset..]);
            let order = left_digits
                .len()
                .cmp(&right_digits.len())
                .then_with(|| left_digits.cmp(right_digits));
            if order != std::cmp::Ordering::Equal {
                return order;
            }
            left_at = left_end;
            right_at = right_end;
            continue;
        }
        match left[left_at].cmp(&right[right_at]) {
            std::cmp::Ordering::Equal => {
                left_at += 1;
                right_at += 1;
            }
            order => return order,
        }
    }
    (left.len() - left_at).cmp(&(right.len() - right_at))
}

fn names(plate: &Map<String, Value>, key: &str, origin: &str) -> Result<Vec<String>, Error> {
    let entries = plate
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Metadata {
            path: origin.to_string(),
            detail: format!("the plate metadata lists no {key}"),
        })?;
    Ok(entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            entry
                .get("name")
                .and_then(Value::as_str)
                .filter(|name| !name.is_empty())
                .map_or_else(
                    || match key {
                        "rows" => row_name(index),
                        _ => column_name(index),
                    },
                    str::to_owned,
                )
        })
        .collect())
}

fn index(well: &Value, key: &str) -> Option<usize> {
    usize::try_from(well.get(key).and_then(Value::as_u64)?).ok()
}

fn field_name(well: &str, field: &Path) -> String {
    match relative(field) {
        Some(path) => format!("{well}/{path}"),
        None => well.to_string(),
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

fn attributes(root: &Root, group: &str) -> Result<Map<String, Value>, Error> {
    let opened = Group::open(root.listable(), group).map_err(|error| Error::Open {
        path: root.tag(group),
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
