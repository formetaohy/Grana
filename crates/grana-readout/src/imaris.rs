use std::collections::BTreeMap;
use std::path::Path;

use hdf5_pure::{AttrValue, Dataset, Datatype, DatatypeByteOrder, File};
use openreadout_hdf5::ims_scene::{self, SceneTable, SceneTableKind};

use crate::error::Error;

const CONTENT: [&str; 2] = ["Scene8/Content", "Scene/Content"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Surfaces,
    Spots,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    mean: f32,
    minimum: f32,
    maximum: f32,
    deviation: f32,
    sum: f64,
}

impl Sample {
    pub fn new(mean: f64, minimum: f64, maximum: f64, deviation: f64, sum: f64) -> Option<Self> {
        let ordered = [mean, minimum, maximum, deviation]
            .iter()
            .all(|value| value.is_finite())
            && minimum <= mean
            && mean <= maximum
            && deviation >= 0.0
            && sum.is_finite()
            && sum >= 0.0;
        ordered.then_some(Self {
            mean: mean as f32,
            minimum: minimum as f32,
            maximum: maximum as f32,
            deviation: deviation as f32,
            sum,
        })
    }

    pub fn mean(self) -> f32 {
        self.mean
    }

    pub fn minimum(self) -> f32 {
        self.minimum
    }

    pub fn maximum(self) -> f32 {
        self.maximum
    }

    pub fn deviation(self) -> f32 {
        self.deviation
    }

    pub fn sum(self) -> f64 {
        self.sum
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Surface {
    vertices: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    triangles: Vec<[u32; 3]>,
    intensities: Option<Vec<Sample>>,
}

impl Surface {
    pub fn new(
        vertices: Vec<[f32; 3]>,
        normals: Vec<[f32; 3]>,
        triangles: Vec<[u32; 3]>,
        intensities: Vec<Sample>,
    ) -> Self {
        assert_eq!(
            vertices.len(),
            normals.len(),
            "every surface vertex carries a normal"
        );
        Self {
            vertices,
            normals,
            triangles,
            intensities: Some(intensities),
        }
    }

    pub fn vertices(&self) -> &[[f32; 3]] {
        &self.vertices
    }

    pub fn normals(&self) -> &[[f32; 3]] {
        &self.normals
    }

    pub fn triangles(&self) -> &[[u32; 3]] {
        &self.triangles
    }

    pub fn intensities(&self) -> Option<&[Sample]> {
        self.intensities.as_deref()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Spot {
    center: [f32; 3],
    radius: f32,
    intensities: Option<Vec<Sample>>,
}

impl Spot {
    pub fn center(&self) -> [f32; 3] {
        self.center
    }

    pub fn radius(&self) -> f32 {
        self.radius
    }

    pub fn intensities(&self) -> Option<&[Sample]> {
        self.intensities.as_deref()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Frame {
    Surfaces(Vec<Surface>),
    Spots(Vec<Spot>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Object {
    name: String,
    kind: Kind,
    frames: Vec<(u32, Frame)>,
}

impl Object {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }

    pub fn frames(&self) -> &[(u32, Frame)] {
        &self.frames
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Imported {
    center: [f32; 3],
    objects: Vec<Object>,
}

impl Imported {
    pub fn center(&self) -> [f32; 3] {
        self.center
    }

    pub fn objects(&self) -> &[Object] {
        &self.objects
    }

    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }
}

pub fn imported(path: &Path) -> Result<Imported, Error> {
    let file = File::open(path).map_err(|error| Error::open(path, error.to_string()))?;
    let center = center(&file);
    let statistics = statistics(&file, path).unwrap_or_default();
    let candidates = content(&file, path)?;
    let mut objects = Vec::new();
    for (name, groups) in candidates {
        let mut found = None;
        for group in &groups {
            if let Some(geometry) = geometry(&file, path, group)? {
                found = Some((group, geometry));
                break;
            }
        }
        let Some((_group, (kind, geometry))) = found else {
            continue;
        };
        let label = groups
            .iter()
            .find_map(|candidate| label(&file, candidate))
            .unwrap_or_else(|| name.clone());
        let ids = groups
            .iter()
            .find_map(|candidate| identifiers(&file, candidate, kind));
        let frames = match kind {
            Kind::Surfaces => {
                let normals = groups
                    .iter()
                    .find_map(|candidate| normals(&file, path, candidate));
                surfaces(&file, path, &geometry, normals, center)
            }
            Kind::Spots => spots(&file, path, &geometry, center),
        };
        let Ok(frames) = frames else {
            continue;
        };
        let frames = attach(frames, statistics.get(&name), ids.as_deref());
        if frames.is_empty() {
            continue;
        }
        objects.push(Object {
            name: label,
            kind,
            frames,
        });
    }
    objects.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(Imported { center, objects })
}

fn center(file: &File) -> [f32; 3] {
    let Ok(group) = file.group("DataSetInfo/Image") else {
        return [0.0; 3];
    };
    let Ok(attrs) = group.attrs() else {
        return [0.0; 3];
    };
    [0, 1, 2].map(|axis| {
        let bound = |key: String| {
            attrs
                .get(&key)
                .and_then(value)
                .filter(|bound| bound.is_finite())
        };
        match (
            bound(format!("ExtMin{axis}")),
            bound(format!("ExtMax{axis}")),
        ) {
            (Some(minimum), Some(maximum)) => (0.5 * (minimum + maximum)) as f32,
            _ => 0.0,
        }
    })
}

fn value(attribute: &AttrValue) -> Option<f64> {
    match attribute {
        AttrValue::F32(value) => Some(f64::from(*value)),
        AttrValue::F64(value) => Some(*value),
        AttrValue::I8(value) => Some(f64::from(*value)),
        AttrValue::I16(value) => Some(f64::from(*value)),
        AttrValue::I32(value) => Some(f64::from(*value)),
        AttrValue::I64(value) => Some(*value as f64),
        AttrValue::U8(value) => Some(f64::from(*value)),
        AttrValue::U16(value) => Some(f64::from(*value)),
        AttrValue::U32(value) => Some(f64::from(*value)),
        AttrValue::U64(value) => Some(*value as f64),
        AttrValue::String(value)
        | AttrValue::AsciiString(value)
        | AttrValue::VarLenString(value)
        | AttrValue::VarLenAsciiString(value) => value.trim().parse().ok(),
        _ => None,
    }
}

fn content(file: &File, path: &Path) -> Result<BTreeMap<String, Vec<String>>, Error> {
    let mut found: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for root in CONTENT {
        let Ok(group) = file.group(root) else {
            continue;
        };
        let mut names = group
            .groups()
            .map_err(|error| Error::read(path, error.to_string()))?;
        names.sort();
        for name in names {
            found
                .entry(name.clone())
                .or_default()
                .push(format!("{root}/{name}"));
        }
    }
    Ok(found)
}

fn geometry(file: &File, path: &Path, group: &str) -> Result<Option<(Kind, String)>, Error> {
    let mut candidates = vec![group.to_string()];
    candidates.extend(["Surfaces", "Points"].map(|child| format!("{group}/{child}")));
    for candidate in candidates {
        let Ok(datasets) = datasets(file, path, &candidate) else {
            continue;
        };
        let holds = |wanted: &str| datasets.iter().any(|held| held == wanted);
        if ["TimeNVerticesNTriangles", "Vertices", "Triangles"]
            .iter()
            .all(|wanted| holds(wanted))
        {
            return Ok(Some((Kind::Surfaces, candidate)));
        }
        if ["CoordsXYZR", "Time"].iter().all(|wanted| holds(wanted)) {
            return Ok(Some((Kind::Spots, candidate)));
        }
    }
    Ok(None)
}

fn datasets(file: &File, path: &Path, group: &str) -> Result<Vec<String>, Error> {
    file.group(group)
        .map_err(|error| Error::read(path, error.to_string()))?
        .datasets()
        .map_err(|error| Error::read(path, error.to_string()))
}

fn label(file: &File, group: &str) -> Option<String> {
    let attrs = file.group(group).ok()?.attrs().ok()?;
    let name = match attrs.get("Name")? {
        AttrValue::String(name)
        | AttrValue::AsciiString(name)
        | AttrValue::VarLenString(name)
        | AttrValue::VarLenAsciiString(name) => name.trim(),
        _ => return None,
    };
    (!name.is_empty()).then(|| name.to_string())
}

fn identifiers(file: &File, group: &str, kind: Kind) -> Option<Vec<i64>> {
    let name = match kind {
        Kind::Surfaces => "Surface",
        Kind::Spots => "Spot",
    };
    for candidate in [
        format!("{group}/{name}"),
        format!("{group}/Surfaces/{name}"),
        format!("{group}/Points/{name}"),
    ] {
        let Ok(dataset) = file.dataset(&candidate) else {
            continue;
        };
        let rows = dataset.shape().ok()?.first().copied()? as usize;
        let raw = dataset.read_raw().ok()?;
        if rows == 0 || raw.len() < rows * 8 {
            continue;
        }
        return Some(
            raw.as_chunks::<8>()
                .0
                .iter()
                .take(rows)
                .map(|record| i64::from_le_bytes(*record))
                .collect(),
        );
    }
    None
}

fn surfaces(
    file: &File,
    path: &Path,
    geometry: &str,
    normals: Option<Table>,
    center: [f32; 3],
) -> Result<Vec<(u32, Frame)>, Error> {
    let counts = read_table(
        file,
        path,
        &format!("{geometry}/TimeNVerticesNTriangles"),
        3,
    )?;
    let vertices = read_table(file, path, &format!("{geometry}/Vertices"), 3)?;
    let triangles = read_table(file, path, &format!("{geometry}/Triangles"), 3)?;
    let mut frames: BTreeMap<u32, Vec<Surface>> = BTreeMap::new();
    let mut vertex_start = 0usize;
    let mut triangle_start = 0usize;
    for row in 0..counts.rows {
        let values = counts.row(row);
        let held_vertices = values[1].max(0.0) as usize;
        let held_triangles = values[2].max(0.0) as usize;
        let vertex_end = (vertex_start + held_vertices).min(vertices.rows);
        let triangle_end = (triangle_start + held_triangles).min(triangles.rows);
        let positions = vertices.slab(vertex_start, vertex_end);
        let faces = triangles.slab(triangle_start, triangle_end);
        let surface = Surface {
            vertices: positions
                .iter()
                .map(|value| {
                    [
                        value[0] as f32 - center[0],
                        value[1] as f32 - center[1],
                        value[2] as f32 - center[2],
                    ]
                })
                .collect(),
            normals: normals
                .as_ref()
                .and_then(|table| unit_normals(&table.slab(vertex_start, vertex_end)))
                .unwrap_or_else(|| computed_normals(&positions, &faces)),
            triangles: faces
                .iter()
                .map(|value| {
                    [
                        value[0].max(0.0) as u32,
                        value[1].max(0.0) as u32,
                        value[2].max(0.0) as u32,
                    ]
                })
                .collect(),
            intensities: None,
        };
        if !surface.vertices.is_empty()
            && !surface.triangles.is_empty()
            && surface
                .vertices
                .iter()
                .flatten()
                .all(|value| value.is_finite())
        {
            frames.entry(time_of(values[0])).or_default().push(surface);
        }
        vertex_start += held_vertices;
        triangle_start += held_triangles;
    }
    Ok(frames
        .into_iter()
        .map(|(time, surfaces)| (time, Frame::Surfaces(surfaces)))
        .collect())
}

fn normals(file: &File, path: &Path, group: &str) -> Option<Table> {
    for candidate in [format!("{group}/Vertex"), format!("{group}/Normals")] {
        if let Ok(table) = read_table(file, path, &candidate, 3) {
            return Some(table);
        }
    }
    None
}

fn unit_normals(values: &[Vec<f64>]) -> Option<Vec<[f32; 3]>> {
    values
        .iter()
        .map(|value| {
            let length = (value[0] * value[0] + value[1] * value[1] + value[2] * value[2]).sqrt();
            (length > 1.0e-6 && value.iter().all(|part| part.is_finite())).then(|| {
                [
                    (value[0] / length) as f32,
                    (value[1] / length) as f32,
                    (value[2] / length) as f32,
                ]
            })
        })
        .collect()
}

fn computed_normals(vertices: &[Vec<f64>], triangles: &[Vec<f64>]) -> Vec<[f32; 3]> {
    let mut normals = vec![[0.0f32; 3]; vertices.len()];
    for triangle in triangles {
        let [a, b, c] = [0, 1, 2].map(|slot| triangle[slot].max(0.0) as usize);
        if a >= vertices.len() || b >= vertices.len() || c >= vertices.len() {
            continue;
        }
        let point = |index: usize| {
            [
                vertices[index][0] as f32,
                vertices[index][1] as f32,
                vertices[index][2] as f32,
            ]
        };
        let (first, second, third) = (point(a), point(b), point(c));
        let edge = [
            second[0] - first[0],
            second[1] - first[1],
            second[2] - first[2],
        ];
        let other = [
            third[0] - first[0],
            third[1] - first[1],
            third[2] - first[2],
        ];
        let normal = [
            edge[1] * other[2] - edge[2] * other[1],
            edge[2] * other[0] - edge[0] * other[2],
            edge[0] * other[1] - edge[1] * other[0],
        ];
        for index in [a, b, c] {
            for (axis, value) in normal.iter().enumerate() {
                normals[index][axis] += value;
            }
        }
    }
    for normal in &mut normals {
        let length = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
        if length > 1.0e-6 {
            for value in normal.iter_mut() {
                *value /= length;
            }
        }
    }
    normals
}

fn spots(
    file: &File,
    path: &Path,
    geometry: &str,
    center: [f32; 3],
) -> Result<Vec<(u32, Frame)>, Error> {
    let coords = read_table(file, path, &format!("{geometry}/CoordsXYZR"), 4)?;
    let times = read_table(file, path, &format!("{geometry}/Time"), 1)?;
    if times.rows != coords.rows {
        return Err(Error::read(
            path,
            "the Imaris spot table holds a different number of times than coordinates",
        ));
    }
    let radii = read_table(file, path, &format!("{geometry}/RadiusYZ"), 2).ok();
    let mut frames: BTreeMap<u32, Vec<Spot>> = BTreeMap::new();
    for row in 0..coords.rows {
        let values = coords.row(row);
        let radius = match radii.as_ref() {
            Some(table) if table.columns == 2 => (values[3].abs() + table.row(row)[1].abs()) * 0.5,
            _ => values[3].abs(),
        };
        if !values[..3].iter().all(|value| value.is_finite())
            || !radius.is_finite()
            || radius <= 0.0
        {
            continue;
        }
        frames
            .entry(time_of(times.row(row)[0]))
            .or_default()
            .push(Spot {
                center: [
                    values[0] as f32 - center[0],
                    values[1] as f32 - center[1],
                    values[2] as f32 - center[2],
                ],
                radius: radius as f32,
                intensities: None,
            });
    }
    Ok(frames
        .into_iter()
        .map(|(time, spots)| (time, Frame::Spots(spots)))
        .collect())
}

struct Table {
    rows: usize,
    columns: usize,
    values: Vec<f64>,
}

impl Table {
    fn row(&self, index: usize) -> &[f64] {
        let start = index * self.columns;
        &self.values[start..start + self.columns]
    }

    fn slab(&self, start: usize, end: usize) -> Vec<Vec<f64>> {
        (start..end.min(self.rows))
            .map(|index| self.row(index).to_vec())
            .collect()
    }
}

fn read_table(file: &File, path: &Path, dataset: &str, columns: usize) -> Result<Table, Error> {
    let held = file
        .dataset(dataset)
        .map_err(|error| Error::read(path, error.to_string()))?;
    let shape = held
        .shape()
        .map_err(|error| Error::read(path, error.to_string()))?;
    let rows = shape.first().copied().unwrap_or(0) as usize;
    let stored = shape.get(1).copied().unwrap_or(1) as usize;
    if rows == 0 || (shape.len() > 1 && stored != columns) {
        return Err(Error::read(
            path,
            format!("the Imaris dataset `{dataset}` does not hold {columns} columns"),
        ));
    }
    let values = read_columns(&held, rows, columns, path, dataset)?;
    Ok(Table {
        rows,
        columns,
        values,
    })
}

fn read_columns(
    dataset: &Dataset,
    rows: usize,
    columns: usize,
    path: &Path,
    name: &str,
) -> Result<Vec<f64>, Error> {
    let datatype = dataset
        .datatype()
        .map_err(|error| Error::read(path, error.to_string()))?;
    let raw = dataset
        .read_raw()
        .map_err(|error| Error::read(path, error.to_string()))?;
    let count = rows * columns;
    if let Some(width) = element_size(&datatype) {
        if raw.len() < count * width {
            return Err(Error::read(
                path,
                format!("the Imaris dataset `{name}` does not hold its declared rows"),
            ));
        }
        return raw
            .chunks_exact(width)
            .take(count)
            .map(|element| {
                decode(&datatype, element).ok_or_else(|| {
                    Error::read(
                        path,
                        format!("the Imaris dataset `{name}` holds an unreadable number"),
                    )
                })
            })
            .collect();
    }
    if let Datatype::Compound { size, ref members } = datatype {
        let fields = members
            .iter()
            .filter(|member| element_size(&member.datatype).is_some())
            .collect::<Vec<_>>();
        if fields.len() != columns {
            return Err(Error::read(
                path,
                format!("the Imaris dataset `{name}` does not hold {columns} numeric fields"),
            ));
        }
        let record = size as usize;
        if record == 0 || raw.len() < rows * record {
            return Err(Error::read(
                path,
                format!("the Imaris dataset `{name}` does not hold its declared rows"),
            ));
        }
        let mut values = Vec::with_capacity(count);
        for index in 0..rows {
            let start = index * record;
            for field in &fields {
                let width = element_size(&field.datatype).expect("the field is numeric");
                let offset = start + field.byte_offset as usize;
                let reader = |slice: &[u8]| {
                    decode(&field.datatype, slice).ok_or_else(|| {
                        Error::read(
                            path,
                            format!("the Imaris dataset `{name}` holds an unreadable number"),
                        )
                    })
                };
                let slice = raw.get(offset..offset + width).ok_or_else(|| {
                    Error::read(
                        path,
                        format!("the Imaris dataset `{name}` cuts a numeric record"),
                    )
                })?;
                values.push(reader(slice)?);
            }
        }
        return Ok(values);
    }
    Err(Error::read(
        path,
        format!("the Imaris dataset `{name}` is not numeric"),
    ))
}

fn element_size(datatype: &Datatype) -> Option<usize> {
    match datatype {
        Datatype::FloatingPoint { size, .. } | Datatype::FixedPoint { size, .. } => {
            Some(*size as usize)
        }
        _ => None,
    }
}

fn decode(datatype: &Datatype, bytes: &[u8]) -> Option<f64> {
    if bytes.len() > 8 {
        return None;
    }
    let big = matches!(
        datatype,
        Datatype::FloatingPoint {
            byte_order: DatatypeByteOrder::BigEndian,
            ..
        } | Datatype::FixedPoint {
            byte_order: DatatypeByteOrder::BigEndian,
            ..
        }
    );
    let mut buffer = [0u8; 8];
    if big {
        for (slot, byte) in bytes.iter().rev().enumerate() {
            buffer[slot] = *byte;
        }
    } else {
        buffer[..bytes.len()].copy_from_slice(bytes);
    }
    match datatype {
        Datatype::FloatingPoint { size: 8, .. } => Some(f64::from_le_bytes(buffer)),
        Datatype::FloatingPoint { size: 4, .. } => {
            Some(f64::from(f32::from_le_bytes(buffer[..4].try_into().ok()?)))
        }
        Datatype::FixedPoint { size, layout, .. } => {
            let value = u64::from_le_bytes(buffer);
            if layout.signed {
                let shift = 64 - 8 * *size;
                Some(((value << shift) as i64 >> shift) as f64)
            } else {
                Some(value as f64)
            }
        }
        _ => None,
    }
}

fn time_of(value: f64) -> u32 {
    value.max(0.0) as u32
}

fn attach(
    frames: Vec<(u32, Frame)>,
    statistics: Option<&Statistics>,
    ids: Option<&[i64]>,
) -> Vec<(u32, Frame)> {
    let mut attached = Vec::with_capacity(frames.len());
    let mut global = 0usize;
    for (time, frame) in frames {
        let frame = match frame {
            Frame::Surfaces(mut surfaces) => {
                for surface in &mut surfaces {
                    surface.intensities =
                        statistics.and_then(|held| held.samples(time, global, ids));
                    global += 1;
                }
                Frame::Surfaces(surfaces)
            }
            Frame::Spots(mut spots) => {
                for spot in &mut spots {
                    spot.intensities = statistics.and_then(|held| held.samples(time, global, ids));
                    global += 1;
                }
                Frame::Spots(spots)
            }
        };
        attached.push((time, frame));
    }
    attached
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Metric {
    Mean,
    Minimum,
    Maximum,
    Deviation,
    Sum,
}

#[derive(Default)]
struct Intensity {
    mean: Option<f64>,
    minimum: Option<f64>,
    maximum: Option<f64>,
    deviation: Option<f64>,
    sum: Option<f64>,
}

struct Statistics {
    channels: usize,
    values: BTreeMap<(i64, i64), Vec<Intensity>>,
}

impl Statistics {
    fn samples(&self, time: u32, index: usize, ids: Option<&[i64]>) -> Option<Vec<Sample>> {
        let id = ids
            .and_then(|held| held.get(index).copied())
            .unwrap_or(index as i64 + 1);
        let intensity = self
            .values
            .get(&(id, time as i64))
            .or_else(|| self.values.get(&(id, -1)))?;
        if intensity.len() != self.channels {
            return None;
        }
        intensity
            .iter()
            .map(|held| {
                Sample::new(
                    held.mean?,
                    held.minimum?,
                    held.maximum?,
                    held.deviation.unwrap_or(0.0),
                    held.sum?,
                )
            })
            .collect()
    }

    fn value(&mut self, id: i64, time: i64, channel: usize) -> &mut Intensity {
        let values = self.values.entry((id, time)).or_default();
        if values.len() <= channel {
            values.resize_with(channel + 1, Intensity::default);
        }
        &mut values[channel]
    }
}

fn statistics(file: &File, path: &Path) -> Result<BTreeMap<String, Statistics>, Error> {
    let tables =
        ims_scene::scene_tables(file).map_err(|error| Error::read(path, error.to_string()))?;
    let mut held: BTreeMap<String, Statistics> = BTreeMap::new();
    for table in &tables {
        if !matches!(table.kind, SceneTableKind::Statistics { .. }) {
            continue;
        }
        let columns = metrics(table);
        if columns.is_empty() {
            continue;
        }
        let channels = columns
            .iter()
            .map(|(_, _, channel)| channel + 1)
            .max()
            .unwrap_or(0);
        let Ok(values) = ims_scene::read_scene_table(file, table) else {
            continue;
        };
        if values.len() != table.stat_columns.len() || values.len() < 2 {
            continue;
        }
        let entry = held
            .entry(table.object.clone())
            .or_insert_with(|| Statistics {
                channels,
                values: BTreeMap::new(),
            });
        entry.channels = entry.channels.max(channels);
        for row in 0..values[1].len() {
            let (id, time) = (values[0][row], values[1][row]);
            if !id.is_finite() || !time.is_finite() {
                continue;
            }
            for (column, metric, channel) in &columns {
                let Some(value) = values.get(*column).and_then(|held| held.get(row)).copied()
                else {
                    continue;
                };
                if !value.is_finite() {
                    continue;
                }
                let slot = entry.value(id as i64, time as i64, *channel);
                match metric {
                    Metric::Mean => slot.mean = Some(value),
                    Metric::Minimum => slot.minimum = Some(value),
                    Metric::Maximum => slot.maximum = Some(value),
                    Metric::Deviation => slot.deviation = Some(value),
                    Metric::Sum => slot.sum = Some(value),
                }
            }
        }
    }
    Ok(held)
}

fn metrics(table: &SceneTable) -> Vec<(usize, Metric, usize)> {
    let mut columns = Vec::new();
    for (index, column) in table
        .info
        .columns
        .iter()
        .enumerate()
        .take(table.stat_columns.len())
    {
        let Some(metric) = metric(&column.name) else {
            continue;
        };
        columns.push((index, metric, channel(&column.name).unwrap_or(0)));
    }
    columns
}

fn metric(name: &str) -> Option<Metric> {
    let lower = name.to_ascii_lowercase();
    let rest = lower.strip_prefix("intensity ")?;
    if rest.starts_with("mean") {
        Some(Metric::Mean)
    } else if rest.starts_with("min") {
        Some(Metric::Minimum)
    } else if rest.starts_with("max") {
        Some(Metric::Maximum)
    } else if rest.starts_with("standard deviation") || rest.starts_with("stddev") {
        Some(Metric::Deviation)
    } else if rest.starts_with("sum") {
        Some(Metric::Sum)
    } else {
        None
    }
}

fn channel(name: &str) -> Option<usize> {
    let start = name.find("Channel=")? + "Channel=".len();
    name[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_intensity_column_names() {
        assert!(matches!(
            metric("Intensity Mean [Channel=0]"),
            Some(Metric::Mean)
        ));
        assert!(matches!(
            metric("Intensity StdDev [Channel=1]"),
            Some(Metric::Deviation)
        ));
        assert_eq!(channel("Intensity Sum [Channel=2]"), Some(2));
        assert_eq!(metric("Volume"), None);
        assert_eq!(channel("Intensity Mean"), None);
    }
}
