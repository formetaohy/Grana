use std::collections::BTreeMap;
use std::path::Path;

use hdf5_pure::{AttrValue, Dataset, Datatype, DatatypeByteOrder, File};
use openreadout_hdf5::ims_scene::{self, FieldValue, Records, SceneTable, SceneTableKind};

use crate::error::Error;

const CONTENT: [&str; 2] = ["Scene8/Content", "Scene/Content"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Surfaces,
    Spots,
    Filaments,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    name: String,
    detail: String,
}

impl Diagnostic {
    pub fn new(name: &str, detail: &str) -> Self {
        assert!(!name.trim().is_empty(), "a diagnostic names its object");
        assert!(!detail.trim().is_empty(), "a diagnostic names its reason");
        Self {
            name: name.to_string(),
            detail: detail.to_string(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn detail(&self) -> &str {
        &self.detail
    }
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
pub struct Filament {
    positions: Vec<[f32; 3]>,
    radii: Vec<f32>,
    edges: Vec<[u32; 2]>,
    intensities: Option<Vec<Sample>>,
}

impl Filament {
    pub fn positions(&self) -> &[[f32; 3]] {
        &self.positions
    }

    pub fn radii(&self) -> &[f32] {
        &self.radii
    }

    pub fn edges(&self) -> &[[u32; 2]] {
        &self.edges
    }

    pub fn intensities(&self) -> Option<&[Sample]> {
        self.intensities.as_deref()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Frame {
    Surfaces(Vec<Surface>),
    Spots(Vec<Spot>),
    Filaments(Vec<Filament>),
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
    diagnostics: Vec<Diagnostic>,
}

impl Imported {
    pub fn center(&self) -> [f32; 3] {
        self.center
    }

    pub fn objects(&self) -> &[Object] {
        &self.objects
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
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
    let mut diagnostics = Vec::new();
    for candidate in &candidates {
        let label = candidate.label(&file);
        match read(
            &file,
            path,
            candidate,
            statistics.get(&candidate.key),
            center,
        ) {
            Ok(Some(object)) => objects.push(object),
            Ok(None) => diagnostics.push(Diagnostic::new(
                &label,
                "the object holds no readable geometry",
            )),
            Err(detail) => diagnostics.push(Diagnostic::new(&label, &detail)),
        }
    }
    objects.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(Imported {
        center,
        objects,
        diagnostics,
    })
}

fn read(
    file: &File,
    path: &Path,
    candidate: &Group,
    statistics: Option<&Statistics>,
    center: [f32; 3],
) -> Result<Option<Object>, String> {
    let Some(source) = geometry(file, path, candidate)? else {
        return Ok(None);
    };
    let kind = source.kind();
    let ids = identifiers(file, candidate, kind);
    let frames = match &source {
        Source::Surfaces(dir) => {
            let normals = candidate
                .paths
                .iter()
                .find_map(|group| normals(file, path, group));
            surfaces(file, path, dir, normals, center).map_err(|error| error.to_string())
        }
        Source::Graphs(dir) => graphs(file, path, dir, center),
        Source::Filaments(dir) => filaments(file, dir, center),
        Source::Spots(dir) => spots(file, path, dir, center).map_err(|error| error.to_string()),
        Source::Points(dir) => points(file, dir, center),
    }?;
    let frames = attach(frames, statistics, ids.as_deref());
    if frames.is_empty() {
        return Err("the object holds no frame with geometry".to_string());
    }
    Ok(Some(Object {
        name: candidate.label(file),
        kind,
        frames,
    }))
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

fn text(attribute: &AttrValue) -> Option<String> {
    let joined = |parts: &[String]| parts.concat().trim_end_matches('\0').trim().to_string();
    let value = match attribute {
        AttrValue::String(value)
        | AttrValue::AsciiString(value)
        | AttrValue::VarLenString(value)
        | AttrValue::VarLenAsciiString(value) => value.trim().to_string(),
        AttrValue::StringSized { value, .. } | AttrValue::AsciiStringSized { value, .. } => {
            value.trim().to_string()
        }
        AttrValue::StringArray(parts)
        | AttrValue::AsciiStringArray(parts)
        | AttrValue::VarLenStringArray(parts)
        | AttrValue::VarLenAsciiStringArray(parts)
        | AttrValue::VarLenAsciiCharArray(parts) => joined(parts),
        AttrValue::StringArraySized { values, .. }
        | AttrValue::AsciiStringArraySized { values, .. } => joined(values),
        _ => return None,
    };
    (!value.is_empty()).then_some(value)
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
        _ => text(attribute)?.parse().ok(),
    }
}

struct Group {
    key: String,
    id: Option<i64>,
    names: Vec<String>,
    paths: Vec<String>,
}

impl Group {
    fn new(key: String, id: Option<i64>, name: Option<String>, path: String) -> Self {
        Self {
            key,
            id,
            names: name.into_iter().collect(),
            paths: vec![path],
        }
    }

    fn absorb(&mut self, path: String, name: Option<String>) {
        self.paths.push(path);
        if let Some(name) = name
            && !self.names.contains(&name)
        {
            self.names.push(name);
        }
    }

    fn matches(&self, id: Option<i64>, folder: &str, name: Option<&str>) -> bool {
        match (self.id, id) {
            (Some(held), Some(other)) => held == other,
            (None, _) => {
                self.key == folder
                    || name.is_some_and(|name| self.names.iter().any(|held| held == name))
            }
            (Some(_), None) => self.key == folder,
        }
    }

    fn label(&self, file: &File) -> String {
        self.paths
            .iter()
            .find_map(|path| label(file, path))
            .or_else(|| self.names.first().cloned())
            .unwrap_or_else(|| self.key.clone())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Source {
    Surfaces(String),
    Filaments(String),
    Graphs(String),
    Spots(String),
    Points(String),
}

impl Source {
    fn kind(&self) -> Kind {
        match self {
            Self::Surfaces(_) => Kind::Surfaces,
            Self::Filaments(_) | Self::Graphs(_) => Kind::Filaments,
            Self::Spots(_) | Self::Points(_) => Kind::Spots,
        }
    }
}

fn content(file: &File, path: &Path) -> Result<Vec<Group>, Error> {
    let mut found: Vec<Group> = Vec::new();
    for root in CONTENT {
        let Ok(group) = file.group(root) else {
            continue;
        };
        let mut names = group
            .groups()
            .map_err(|error| Error::read(path, error.to_string()))?;
        names.sort();
        for folder in names {
            let full = format!("{root}/{folder}");
            let id = identity(file, &full);
            let name = label(file, &full);
            match found
                .iter_mut()
                .find(|held| held.matches(id, &folder, name.as_deref()))
            {
                Some(held) => held.absorb(full, name),
                None => found.push(Group::new(folder, id, name, full)),
            }
        }
    }
    Ok(found)
}

fn identity(file: &File, group: &str) -> Option<i64> {
    let attrs = file.group(group).ok()?.attrs().ok()?;
    attrs
        .get("Id")
        .and_then(value)
        .filter(|id| id.is_finite())
        .map(|id| id as i64)
}

fn datasets(file: &File, path: &Path, group: &str) -> Result<Vec<String>, Error> {
    file.group(group)
        .map_err(|error| Error::read(path, error.to_string()))?
        .datasets()
        .map_err(|error| Error::read(path, error.to_string()))
}

fn holds(file: &File, path: &Path, group: &str, dataset: &str) -> bool {
    datasets(file, path, group).is_ok_and(|held| held.iter().any(|name| name == dataset))
}

fn geometry(file: &File, path: &Path, candidate: &Group) -> Result<Option<Source>, String> {
    let mut blocked = None;
    for group in &candidate.paths {
        let Ok(held) = datasets(file, path, group) else {
            continue;
        };
        let holds_here = |wanted: &str| held.iter().any(|name| name == wanted);
        if ["TimeNVerticesNTriangles", "Vertices", "Triangles"]
            .iter()
            .all(|wanted| holds_here(wanted))
        {
            return Ok(Some(Source::Surfaces(group.clone())));
        }
        let surfaces = format!("{group}/Surfaces");
        if ["TimeNVerticesNTriangles", "Vertices", "Triangles"]
            .iter()
            .all(|wanted| holds(file, path, &surfaces, wanted))
        {
            return Ok(Some(Source::Surfaces(surfaces)));
        }
        if ["Vertex", "Edge", "Filament"]
            .iter()
            .all(|wanted| holds_here(wanted))
        {
            return Ok(Some(Source::Filaments(group.clone())));
        }
        let graphs = format!("{group}/Graphs");
        if ["TimesNVerticesNEdgesRoots", "Vertices", "Segments"]
            .iter()
            .all(|wanted| holds(file, path, &graphs, wanted))
        {
            return Ok(Some(Source::Graphs(graphs)));
        }
        if ["CoordsXYZR", "Time"]
            .iter()
            .all(|wanted| holds_here(wanted))
        {
            return Ok(Some(Source::Spots(group.clone())));
        }
        let points = format!("{group}/Points");
        if ["CoordsXYZR", "Time"]
            .iter()
            .all(|wanted| holds(file, path, &points, wanted))
        {
            return Ok(Some(Source::Spots(points)));
        }
        if holds_here("Vertex") && holds(file, path, &format!("{group}/Points"), "Point") {
            return Ok(Some(Source::Points(group.clone())));
        }
        if holds_here("Vertex") && holds_here("Point") {
            return Ok(Some(Source::Points(group.clone())));
        }
        if holds_here("SurfaceModel") || holds_here("BlockData") {
            blocked =
                Some("the object stores only an Imaris block model, which is not read".to_string());
        }
    }
    match blocked {
        Some(detail) => Err(detail),
        None => Ok(None),
    }
}

fn label(file: &File, group: &str) -> Option<String> {
    let attrs = file.group(group).ok()?.attrs().ok()?;
    let name = text(attrs.get("Name")?)?;
    (!name.trim().is_empty()).then(|| name.trim().to_string())
}

fn identifiers(file: &File, candidate: &Group, kind: Kind) -> Option<Vec<i64>> {
    let names: &[&str] = match kind {
        Kind::Surfaces => &["Surface"],
        Kind::Spots => &["Point", "Spot"],
        Kind::Filaments => &["Filament"],
    };
    for group in &candidate.paths {
        for name in names {
            for path in [
                format!("{group}/{name}"),
                format!("{group}/Surfaces/{name}"),
                format!("{group}/Points/{name}"),
            ] {
                if let Some(ids) = identifiers_at(file, &path) {
                    return Some(ids);
                }
            }
        }
    }
    None
}

fn identifiers_at(file: &File, dataset: &str) -> Option<Vec<i64>> {
    let held = file.dataset(dataset).ok()?;
    if let Some(ids) = records(file, dataset).and_then(|held| column(&held, "ID")) {
        return Some(ids.iter().map(|value| *value as i64).collect());
    }
    let rows = held.shape().ok()?.first().copied()? as usize;
    let raw = held.read_raw().ok()?;
    if rows == 0 || raw.len() < rows * 8 {
        return None;
    }
    Some(
        raw.as_chunks::<8>()
            .0
            .iter()
            .take(rows)
            .map(|record| i64::from_le_bytes(*record))
            .collect(),
    )
}

fn records(file: &File, dataset: &str) -> Option<Records> {
    ims_scene::read_records(file, dataset).ok().flatten()
}

fn column(records: &Records, name: &str) -> Option<Vec<f64>> {
    let index = records.fields.iter().position(|field| field == name)?;
    records
        .rows
        .iter()
        .map(|row| match row.get(index)? {
            FieldValue::Number(value) => Some(*value),
            FieldValue::Text(_) => None,
        })
        .collect()
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

fn graphs(
    file: &File,
    path: &Path,
    directory: &str,
    center: [f32; 3],
) -> Result<Vec<(u32, Frame)>, String> {
    let counts = table(
        file,
        path,
        &format!("{directory}/TimesNVerticesNEdgesRoots"),
        4,
    )?;
    let vertices = table(file, path, &format!("{directory}/Vertices"), 5)?;
    let segments = table(file, path, &format!("{directory}/Segments"), 2)?;
    let mut order = (0..counts.rows).collect::<Vec<_>>();
    order.sort_by(|left, right| counts.row(*left)[0].total_cmp(&counts.row(*right)[0]));
    let mut frames: BTreeMap<u32, Vec<Filament>> = BTreeMap::new();
    let mut vertex_start = 0usize;
    let mut edge_start = 0usize;
    for row in order {
        let values = counts.row(row);
        let held_vertices = values[1].max(0.0) as usize;
        let held_edges = values[2].max(0.0) as usize;
        let vertex_end = (vertex_start + held_vertices).min(vertices.rows);
        let edge_end = (edge_start + held_edges).min(segments.rows);
        if let Some(filament) = graph(
            &vertices,
            vertex_start..vertex_end,
            &segments,
            edge_start..edge_end,
            center,
        ) {
            frames.entry(time_of(values[0])).or_default().push(filament);
        }
        vertex_start += held_vertices;
        edge_start += held_edges;
    }
    Ok(frames
        .into_iter()
        .map(|(time, filaments)| (time, Frame::Filaments(filaments)))
        .collect())
}

fn graph(
    vertices: &Table,
    nodes: std::ops::Range<usize>,
    segments: &Table,
    edges: std::ops::Range<usize>,
    center: [f32; 3],
) -> Option<Filament> {
    if nodes.len() < 2 || edges.is_empty() {
        return None;
    }
    let positions = nodes
        .clone()
        .map(|index| {
            let value = vertices.row(index);
            [
                value[0] as f32 - center[0],
                value[1] as f32 - center[1],
                value[2] as f32 - center[2],
            ]
        })
        .collect::<Vec<_>>();
    let radii = nodes
        .clone()
        .map(|index| (0.5 * vertices.row(index)[3].abs()) as f32)
        .collect::<Vec<_>>();
    let mut held = Vec::with_capacity(edges.len());
    for index in edges {
        let value = segments.row(index);
        let (a, b) = (value[0].max(0.0) as usize, value[1].max(0.0) as usize);
        if !(nodes.contains(&a) && nodes.contains(&b)) || a == b {
            continue;
        }
        held.push([(a - nodes.start) as u32, (b - nodes.start) as u32]);
    }
    if held.is_empty() {
        return None;
    }
    let radii = positive(radii, &positions)?;
    build(positions, radii, held)
}

fn filaments(file: &File, group: &str, center: [f32; 3]) -> Result<Vec<(u32, Frame)>, String> {
    let vertices = records(file, &format!("{group}/Vertex"))
        .ok_or_else(|| "the filament object holds no vertex records".to_string())?;
    let edges = records(file, &format!("{group}/Edge"))
        .ok_or_else(|| "the filament object holds no edge records".to_string())?;
    let filaments = records(file, &format!("{group}/Filament"))
        .ok_or_else(|| "the filament object holds no filament records".to_string())?;
    let positions = coordinates(&vertices, center)
        .ok_or_else(|| "the filament vertices do not hold finite positions".to_string())?;
    let radii = column(&vertices, "Radius")
        .ok_or_else(|| "the filament vertices hold no radius".to_string())?;
    let first = column(&edges, "VertexA")
        .ok_or_else(|| "the filament edges hold no first vertex".to_string())?;
    let second = column(&edges, "VertexB")
        .ok_or_else(|| "the filament edges hold no second vertex".to_string())?;
    let times = column(&filaments, "ID_Time")
        .ok_or_else(|| "the filaments hold no frame index".to_string())?;
    let node_begin = column(&filaments, "IndexVertexBegin")
        .ok_or_else(|| "the filaments hold no vertex range".to_string())?;
    let node_end = column(&filaments, "IndexVertexEnd")
        .ok_or_else(|| "the filaments hold no vertex range".to_string())?;
    let edge_begin = column(&filaments, "IndexEdgeBegin")
        .ok_or_else(|| "the filaments hold no edge range".to_string())?;
    let edge_end = column(&filaments, "IndexEdgeEnd")
        .ok_or_else(|| "the filaments hold no edge range".to_string())?;
    let mut frames: BTreeMap<u32, Vec<Filament>> = BTreeMap::new();
    for row in 0..filaments.rows.len() {
        let nodes = range(node_begin[row], node_end[row], positions.len());
        let held = range(edge_begin[row], edge_end[row], first.len());
        let (Some(nodes), Some(held)) = (nodes, held) else {
            continue;
        };
        if nodes.is_empty() || held.is_empty() {
            continue;
        }
        let slice = positions[nodes.clone()].to_vec();
        let slice_radii = radii[nodes.clone()]
            .iter()
            .map(|radius| *radius as f32)
            .collect::<Vec<_>>();
        let mut links = Vec::new();
        for edge in held {
            let (a, b) = (
                first[edge].max(0.0) as usize,
                second[edge].max(0.0) as usize,
            );
            if !(nodes.contains(&a) && nodes.contains(&b)) || a == b {
                continue;
            }
            links.push([(a - nodes.start) as u32, (b - nodes.start) as u32]);
        }
        if links.is_empty() {
            continue;
        }
        let Some(radii) = positive(slice_radii, &slice) else {
            continue;
        };
        if let Some(filament) = build(slice, radii, links) {
            frames
                .entry(time_of(times[row]))
                .or_default()
                .push(filament);
        }
    }
    Ok(frames
        .into_iter()
        .map(|(time, filaments)| (time, Frame::Filaments(filaments)))
        .collect())
}

fn points(file: &File, group: &str, center: [f32; 3]) -> Result<Vec<(u32, Frame)>, String> {
    let vertices = records(file, &format!("{group}/Vertex"))
        .ok_or_else(|| "the spot object holds no vertex records".to_string())?;
    let points = records(file, &format!("{group}/Points/Point"))
        .or_else(|| records(file, &format!("{group}/Point")))
        .ok_or_else(|| "the spot object holds no point records".to_string())?;
    let positions = coordinates(&vertices, center)
        .ok_or_else(|| "the spot vertices do not hold finite positions".to_string())?;
    let radii = column(&vertices, "Radius")
        .ok_or_else(|| "the spot vertices hold no radius".to_string())?;
    let nodes = column(&points, "IndexVertex")
        .ok_or_else(|| "the spot points name no vertex".to_string())?;
    let times = column(&points, "ID_Time")
        .ok_or_else(|| "the spot points hold no frame index".to_string())?;
    let mut frames: BTreeMap<u32, Vec<Spot>> = BTreeMap::new();
    for row in 0..points.rows.len() {
        let node = nodes[row];
        let radius = radii.get(node.max(0.0) as usize).copied();
        let center = positions.get(node.max(0.0) as usize).copied();
        let (Some(center), Some(radius)) = (center, radius) else {
            continue;
        };
        let radius = radius.abs();
        if !center.iter().all(|value| value.is_finite()) || !radius.is_finite() || radius <= 0.0 {
            continue;
        }
        frames.entry(time_of(times[row])).or_default().push(Spot {
            center,
            radius: radius as f32,
            intensities: None,
        });
    }
    Ok(frames
        .into_iter()
        .map(|(time, spots)| (time, Frame::Spots(spots)))
        .collect())
}

fn coordinates(records: &Records, center: [f32; 3]) -> Option<Vec<[f32; 3]>> {
    let x = column(records, "PositionX")?;
    let y = column(records, "PositionY")?;
    let z = column(records, "PositionZ")?;
    x.iter()
        .zip(y.iter())
        .zip(z.iter())
        .map(|((x, y), z)| {
            let point = [
                *x as f32 - center[0],
                *y as f32 - center[1],
                *z as f32 - center[2],
            ];
            point.iter().all(|value| value.is_finite()).then_some(point)
        })
        .collect()
}

fn build(positions: Vec<[f32; 3]>, radii: Vec<f32>, edges: Vec<[u32; 2]>) -> Option<Filament> {
    if positions.len() < 2 || radii.len() != positions.len() || edges.is_empty() {
        return None;
    }
    if !positions.iter().flatten().all(|value| value.is_finite()) {
        return None;
    }
    let count = positions.len() as u32;
    let mut kept = Vec::with_capacity(edges.len());
    for edge in edges {
        if edge[0] >= count || edge[1] >= count || edge[0] == edge[1] {
            continue;
        }
        kept.push(edge);
    }
    if kept.is_empty() {
        return None;
    }
    Some(Filament {
        positions,
        radii,
        edges: kept,
        intensities: None,
    })
}

fn positive(radii: Vec<f32>, positions: &[[f32; 3]]) -> Option<Vec<f32>> {
    let valid = |radius: &f32| radius.is_finite() && *radius > 0.0;
    let fallback = if radii.iter().all(valid) {
        0.0
    } else {
        let mut sorted = radii
            .iter()
            .copied()
            .filter(|radius| valid(radius))
            .collect::<Vec<_>>();
        sorted.sort_by(f32::total_cmp);
        sorted
            .get(sorted.len() / 2)
            .copied()
            .or_else(|| nearest(positions))?
    };
    radii
        .iter()
        .map(|radius| {
            let value = if valid(radius) { *radius } else { fallback };
            (value > 0.0).then_some(value)
        })
        .collect()
}

fn nearest(positions: &[[f32; 3]]) -> Option<f32> {
    let mut nearest = f32::INFINITY;
    for pair in positions.windows(2) {
        let distance = ((pair[1][0] - pair[0][0]).powi(2)
            + (pair[1][1] - pair[0][1]).powi(2)
            + (pair[1][2] - pair[0][2]).powi(2))
        .sqrt();
        if distance.is_finite() && distance > 0.0 {
            nearest = nearest.min(distance);
        }
    }
    nearest.is_finite().then_some(0.5 * nearest)
}

fn range(begin: f64, end: f64, limit: usize) -> Option<std::ops::Range<usize>> {
    let begin = begin.max(0.0) as usize;
    let end = (end.max(0.0) as usize).min(limit);
    (begin < end && begin < limit).then_some(begin..end)
}

fn table(file: &File, path: &Path, dataset: &str, columns: usize) -> Result<Table, String> {
    read_table(file, path, dataset, columns).map_err(|error| error.to_string())
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
            Frame::Filaments(mut filaments) => {
                for filament in &mut filaments {
                    filament.intensities =
                        statistics.and_then(|held| held.samples(time, global, ids));
                    global += 1;
                }
                Frame::Filaments(filaments)
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
