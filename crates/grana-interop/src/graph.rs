use grana_object::SampleStatistics;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Points,
    Surfaces,
    Filaments,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Points => "points",
            Self::Surfaces => "surfaces",
            Self::Filaments => "filaments",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "points" => Some(Self::Points),
            "surfaces" => Some(Self::Surfaces),
            "filaments" => Some(Self::Filaments),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Mesh {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    indices: Vec<u32>,
}

impl Mesh {
    pub fn new(positions: Vec<[f32; 3]>, normals: Vec<[f32; 3]>, indices: Vec<u32>) -> Self {
        assert!(
            !positions.is_empty(),
            "an object mesh holds at least one vertex"
        );
        assert!(
            positions
                .iter()
                .all(|point| point.iter().all(|axis| axis.is_finite())),
            "an object mesh lies at finite positions"
        );
        assert_eq!(
            normals.len(),
            positions.len(),
            "an object mesh names one normal per vertex"
        );
        assert!(
            normals
                .iter()
                .all(|normal| normal.iter().all(|axis| axis.is_finite())),
            "an object mesh carries finite normals"
        );
        assert!(
            !indices.is_empty() && indices.len().is_multiple_of(3),
            "an object mesh holds whole triangles"
        );
        assert!(
            indices
                .iter()
                .all(|index| (*index as usize) < positions.len()),
            "an object mesh names the vertices of its triangles"
        );
        Self {
            positions,
            normals,
            indices,
        }
    }

    pub fn positions(&self) -> &[[f32; 3]] {
        &self.positions
    }

    pub fn normals(&self) -> &[[f32; 3]] {
        &self.normals
    }

    pub fn indices(&self) -> &[u32] {
        &self.indices
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    Point,
    Surface {
        mesh: Mesh,
        voxels: u64,
        volume: f64,
        area: f64,
    },
    Filament {
        mesh: Mesh,
        length: f64,
        diameter: f64,
        volume: f64,
        branches: u32,
        terminals: u32,
        junctions: u32,
        nodes: u32,
    },
}

impl Shape {
    pub fn kind(&self) -> Kind {
        match self {
            Self::Point => Kind::Points,
            Self::Surface { .. } => Kind::Surfaces,
            Self::Filament { .. } => Kind::Filaments,
        }
    }

    pub fn mesh(&self) -> Option<&Mesh> {
        match self {
            Self::Point => None,
            Self::Surface { mesh, .. } | Self::Filament { mesh, .. } => Some(mesh),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Related {
    object_type: String,
    path: String,
    node_prop: Option<String>,
}

impl Related {
    pub fn new(object_type: &str, path: &str, node_prop: Option<&str>) -> Self {
        assert!(
            !object_type.trim().is_empty(),
            "a related object names its type"
        );
        assert!(!path.trim().is_empty(), "a related object names its path");
        assert!(
            node_prop.is_none_or(|name| !name.trim().is_empty()),
            "a related object names its node property"
        );
        Self {
            object_type: object_type.to_string(),
            path: path.to_string(),
            node_prop: node_prop.map(str::to_string),
        }
    }

    pub fn object_type(&self) -> &str {
        &self.object_type
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn node_prop(&self) -> Option<&str> {
        self.node_prop.as_deref()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Objects {
    kind: Kind,
    shapes: Vec<Shape>,
    labels: Vec<u64>,
    intensities: Option<Vec<Vec<SampleStatistics>>>,
    related: Vec<Related>,
}

impl Objects {
    pub fn new(
        kind: Kind,
        shapes: Vec<Shape>,
        labels: Vec<u64>,
        intensities: Option<Vec<Vec<SampleStatistics>>>,
        related: Vec<Related>,
    ) -> Self {
        assert!(
            !shapes.is_empty(),
            "a graph carries a shape for at least one detection"
        );
        assert_eq!(
            labels.len(),
            shapes.len(),
            "every graph shape carries its segmentation label"
        );
        assert!(
            labels.iter().all(|label| *label > 0),
            "a graph shape carries a positive segmentation label"
        );
        assert!(
            shapes.iter().all(|shape| shape.kind() == kind),
            "every graph shape matches the kind of its graph"
        );
        if let Some(intensities) = &intensities {
            assert_eq!(
                intensities.len(),
                shapes.len(),
                "every graph shape carries its intensity samples"
            );
            let channels = intensities.first().map_or(0, Vec::len);
            assert!(
                channels > 0 && intensities.iter().all(|samples| samples.len() == channels),
                "every graph shape measures the same channels"
            );
        }
        Self {
            kind,
            shapes,
            labels,
            intensities,
            related,
        }
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }

    pub fn shapes(&self) -> &[Shape] {
        &self.shapes
    }

    pub fn labels(&self) -> &[u64] {
        &self.labels
    }

    pub fn intensities(&self) -> Option<&[Vec<SampleStatistics>]> {
        self.intensities.as_deref()
    }

    pub fn channels(&self) -> usize {
        self.intensities
            .as_ref()
            .and_then(|intensities| intensities.first().map(Vec::len))
            .unwrap_or(0)
    }

    pub fn related(&self) -> &[Related] {
        &self.related
    }
}
