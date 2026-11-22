use std::collections::VecDeque;

use serde_json::Value;

use crate::Error;
use crate::image::GroupImage;
use crate::metadata::{self, Axis};
use crate::transform::{self, Affine};

use super::grid::{EPSILON, basis_matrix, determinant, invert, metadata_error};

pub(super) struct Spatial {
    pub(super) linear: [[f64; 3]; 3],
    pub(super) offset: [f64; 3],
}

pub(super) struct Endpoint {
    pub(super) path: String,
    pub(super) name: String,
}

pub(super) struct Node {
    pub(super) path: String,
    pub(super) name: String,
    pub(super) axes: Vec<Axis>,
}

pub(super) struct Link {
    from: usize,
    to: usize,
    affine: Affine,
}

pub(super) fn endpoints(
    path: &str,
    group: &str,
    transforms: &[Value],
) -> Result<Vec<Endpoint>, Error> {
    let mut found = Vec::new();
    for transform in transforms {
        for key in ["input", "output"] {
            let endpoint = transform
                .get(key)
                .and_then(Value::as_object)
                .ok_or_else(|| {
                    metadata_error(
                        path,
                        format!("a scene transform names no `{key}` coordinate system"),
                    )
                })?;
            let name = endpoint
                .get("name")
                .and_then(Value::as_str)
                .filter(|name| !name.is_empty())
                .ok_or_else(|| {
                    metadata_error(
                        path,
                        format!("a scene transform names no `{key}` coordinate system"),
                    )
                })?;
            let member = match endpoint.get("path") {
                None | Some(Value::Null) => String::new(),
                Some(Value::String(stored)) if stored.is_empty() => String::new(),
                Some(Value::String(stored)) => crate::hcs::join(group, stored.trim_matches('/')),
                Some(_) => {
                    return Err(metadata_error(
                        path,
                        "a scene transform endpoint path is not a string",
                    ));
                }
            };
            found.push(Endpoint {
                path: member,
                name: name.to_string(),
            });
        }
    }
    Ok(found)
}

pub(super) fn groups_of(endpoints: &[Endpoint]) -> Vec<String> {
    let mut groups: Vec<String> = Vec::new();
    for endpoint in endpoints {
        if endpoint.path.is_empty() {
            continue;
        }
        if !groups.contains(&endpoint.path) {
            groups.push(endpoint.path.clone());
        }
    }
    groups
}

pub(super) fn nodes(
    systems: &[metadata::System],
    groups: &[String],
    images: &[GroupImage],
) -> Vec<Node> {
    let mut nodes = systems
        .iter()
        .map(|system| Node {
            path: String::new(),
            name: system.name().to_string(),
            axes: system.axes().to_vec(),
        })
        .collect::<Vec<_>>();
    for (group, image) in groups.iter().zip(images) {
        for system in image.systems() {
            nodes.push(Node {
                path: group.clone(),
                name: system.name().to_string(),
                axes: system.axes().to_vec(),
            });
        }
        if image.systems().is_empty() {
            nodes.push(Node {
                path: group.clone(),
                name: String::new(),
                axes: image.axes().to_vec(),
            });
        }
    }
    nodes
}

fn find(nodes: &[Node], endpoint: &Endpoint) -> Option<usize> {
    nodes
        .iter()
        .position(|node| node.path == endpoint.path && node.name == endpoint.name)
        .or_else(|| {
            nodes.iter().position(|node| {
                node.path == endpoint.path && node.name.is_empty() && !node.path.is_empty()
            })
        })
}

pub(super) fn links(
    path: &str,
    transforms: &[Value],
    endpoints: &[Endpoint],
    nodes: &[Node],
) -> Result<Vec<Link>, Error> {
    let mut links = Vec::with_capacity(transforms.len());
    for (transform, pair) in transforms.iter().zip(endpoints.chunks(2)) {
        let from = find(nodes, &pair[0]).ok_or_else(|| {
            metadata_error(
                path,
                format!("the scene names no coordinate system `{}`", pair[0].name),
            )
        })?;
        let to = find(nodes, &pair[1]).ok_or_else(|| {
            metadata_error(
                path,
                format!("the scene names no coordinate system `{}`", pair[1].name),
            )
        })?;
        let affine = transform::parse(
            path,
            transform,
            nodes[from].axes.len(),
            nodes[to].axes.len(),
        )?;
        links.push(Link { from, to, affine });
    }
    Ok(links)
}

pub(super) fn route(links: &[Link], nodes: &[Node], from: usize, to: usize) -> Option<Affine> {
    let dimensions = nodes[from].axes.len();
    let mut visited = vec![false; nodes.len()];
    let mut pending = VecDeque::new();
    visited[from] = true;
    pending.push_back((from, Affine::identity(dimensions)));
    while let Some((node, affine)) = pending.pop_front() {
        if node == to {
            return Some(affine);
        }
        for link in links {
            let (next, step) = if link.from == node {
                (link.to, Some(link.affine.clone()))
            } else if link.to == node {
                (link.from, link.affine.inverse().ok())
            } else {
                continue;
            };
            let Some(step) = step else {
                continue;
            };
            if visited[next] {
                continue;
            }
            let Some(composite) = step.composite(&affine).ok() else {
                continue;
            };
            visited[next] = true;
            pending.push_back((next, composite));
        }
    }
    None
}

pub(super) fn reference(images: &[GroupImage]) -> Result<usize, Error> {
    let mut chosen = 0;
    let mut volume = f64::INFINITY;
    for (index, image) in images.iter().enumerate() {
        let level = image.pyramid().level(0);
        let basis = basis_matrix(level.basis());
        let determinant = determinant(basis).abs();
        if determinant > 0.0 && determinant < volume {
            volume = determinant;
            chosen = index;
        }
    }
    Ok(chosen)
}

pub(super) fn display(
    systems: &[metadata::System],
    nodes: &[Node],
    groups: &[String],
    images: &[GroupImage],
    reference: usize,
) -> Result<usize, Error> {
    if let Some(system) = systems.first() {
        return nodes
            .iter()
            .position(|node| node.path.is_empty() && node.name == system.name())
            .ok_or_else(|| {
                metadata_error(
                    "the scene",
                    format!("the scene does not declare `{}`", system.name()),
                )
            });
    }
    let group = &groups[reference];
    let image = &images[reference];
    nodes
        .iter()
        .position(|node| {
            node.path == *group
                && match image.intrinsic() {
                    Some(name) => node.name == name,
                    None => node.name.is_empty(),
                }
        })
        .ok_or_else(|| {
            metadata_error(
                "the scene",
                format!("the group `{group}` names no coordinate system"),
            )
        })
}

pub(super) fn intrinsic(nodes: &[Node], group: &str, image: &GroupImage) -> Result<usize, Error> {
    let found = nodes.iter().position(|node| {
        node.path == group
            && match image.intrinsic() {
                Some(name) => node.name == name,
                None => node.name.is_empty(),
            }
    });
    found.ok_or_else(|| {
        metadata_error(
            "the scene",
            format!("the group `{group}` names no intrinsic coordinate system"),
        )
    })
}

pub(super) fn spatial(
    path: &str,
    display: &Node,
    source: &Node,
    route: &Affine,
) -> Result<Spatial, Error> {
    let displayed = metadata::space_axes(path, &display.axes)?;
    let stored = metadata::space_axes(path, &source.axes)?;
    for component in 0..2 {
        if displayed[component].is_none() || stored[component].is_none() {
            return Err(Error::Unsupported {
                path: path.to_string(),
                detail: format!(
                    "the coordinate systems of `{}` do not carry the spatial axes",
                    display.name
                ),
            });
        }
    }
    let mut linear = [[0.0; 3]; 3];
    let mut offset = [0.0; 3];
    for component in 0..3 {
        let Some(row) = displayed[component] else {
            let axis = stored[component].unwrap_or(component);
            linear[component][axis] = 1.0;
            continue;
        };
        let factor = display.axes[row].factor();
        offset[component] = route.offset(row) * factor;
        for axis in 0..3 {
            let Some(column) = stored[axis] else {
                continue;
            };
            linear[component][axis] =
                route.linear(row, column) * factor / source.axes[column].factor();
        }
        for column in 0..source.axes.len() {
            if stored.iter().flatten().any(|held| *held == column) {
                continue;
            }
            if route.linear(row, column).abs() > EPSILON {
                return Err(Error::Unsupported {
                    path: path.to_string(),
                    detail: format!(
                        "the placement of `{}` depends on a time or channel axis",
                        source.name
                    ),
                });
            }
        }
    }
    if stored[2].is_none()
        && let Some(row) = displayed[2]
    {
        for column in stored.iter().take(2).flatten() {
            if route.linear(row, *column).abs() > EPSILON {
                return Err(Error::Unsupported {
                    path: path.to_string(),
                    detail: format!(
                        "the placement of `{}` tilts its plane out of the plane",
                        source.name
                    ),
                });
            }
        }
        linear[2] = [0.0, 0.0, plane_spacing(linear)];
    }
    if invert(linear).is_none() {
        return Err(Error::Unsupported {
            path: path.to_string(),
            detail: format!("the placement of `{}` is not invertible", source.name),
        });
    }
    Ok(Spatial { linear, offset })
}

fn plane_spacing(linear: [[f64; 3]; 3]) -> f64 {
    let spacing = (0..3)
        .map(|component| linear[0][component].abs().max(linear[1][component].abs()))
        .filter(|value| *value > 0.0)
        .fold(f64::INFINITY, f64::min);
    if spacing.is_finite() { spacing } else { 1.0 }
}
