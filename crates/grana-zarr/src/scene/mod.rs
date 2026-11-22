mod assemble;
mod graph;
mod grid;
mod member;

use std::ops::Range;

use grana_volume::{Level, Pyramid, VoxelData};
use serde_json::{Map, Value};

use crate::Error;
use crate::hcs;
use crate::image::GroupImage;
use crate::metadata;
use crate::root::Root;
use crate::witness;

use self::assemble::{calibration, channels, frames, interval};
use self::graph::{display, endpoints, groups_of, intrinsic, links, nodes, reference, spatial};
use self::grid::grid;
use self::member::Member;

pub(crate) struct Scene {
    root: Root,
    group: String,
    name: String,
    members: Vec<Member>,
    levels: Vec<Level>,
    pyramid: Pyramid,
}

impl Scene {
    pub(crate) fn open(
        root: Root,
        group: &str,
        attributes: &Map<String, Value>,
    ) -> Result<Self, Error> {
        let tag = root.tag(group);
        let fields = hcs::fields(attributes);
        let declared = fields
            .get("scene")
            .ok_or_else(|| grid::metadata_error(&tag, "the group carries no scene metadata"))?;
        let scene = declared
            .as_object()
            .ok_or_else(|| grid::metadata_error(&tag, "the scene metadata is not a json object"))?;
        let systems = metadata::coordinate_systems(&tag, declared)?;
        let transforms = scene
            .get("coordinateTransformations")
            .and_then(Value::as_array)
            .filter(|transforms| !transforms.is_empty())
            .ok_or_else(|| {
                grid::metadata_error(&tag, "the scene lists no coordinate transformation")
            })?;
        let endpoints = endpoints(&tag, group, transforms)?;
        let groups = groups_of(&endpoints);
        let cache_bytes = crate::cache::per_level(groups.len());
        let mut images = Vec::with_capacity(groups.len());
        for member in &groups {
            images.push(GroupImage::open_member(&root, member, cache_bytes)?);
        }
        let nodes = nodes(&systems, &groups, &images);
        let resolved = links(&tag, transforms, &endpoints, &nodes)?;
        let reference = reference(&images)?;
        let display = display(&systems, &nodes, &groups, &images, reference)?;
        let mut members = Vec::with_capacity(groups.len());
        for (index, image) in images.into_iter().enumerate() {
            let source = intrinsic(&nodes, &groups[index], &image)?;
            let route = graph::route(&resolved, &nodes, source, display).ok_or_else(|| {
                grid::metadata_error(
                    &tag,
                    format!(
                        "the coordinate system `{}` of `{}` is not connected to the scene",
                        nodes[source].name, groups[index]
                    ),
                )
            })?;
            let placement = spatial(&tag, &nodes[display], &nodes[source], &route)?;
            members.push(Member::new(&tag, groups[index].clone(), image, placement)?);
        }
        let stored = members
            .iter()
            .map(|member| member.geometry.len())
            .min()
            .expect("a scene holds at least one member");
        let grid = grid(&tag, &members, reference, stored)?;
        for member in &mut members {
            member.index(&tag, &grid.levels, grid.shift)?;
        }
        let channels = channels(&tag, &members)?;
        let frames = frames(&tag, &members)?;
        let interval = interval(&tag, &members)?;
        let calibration = calibration(&tag, &members, reference)?;
        let name = root.name_at(group);
        let pyramid = Pyramid::placed(
            grid.levels.clone(),
            stored,
            channels,
            frames,
            interval,
            grid.placement,
            calibration,
        );
        Ok(Self {
            root,
            group: group.to_string(),
            name,
            members,
            levels: grid.levels,
            pyramid,
        })
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn pyramid(&self) -> &Pyramid {
        &self.pyramid
    }

    pub(crate) fn transfer(&self) -> Option<std::sync::Arc<grana_net::Transfer>> {
        self.root.transfer()
    }

    pub(crate) fn identity(&self) -> Option<u64> {
        let mut hash = witness::START;
        hash = witness::mix(hash, self.group.as_bytes());
        hash = witness::mix(hash, &(self.levels.len() as u64).to_le_bytes());
        for member in &self.members {
            hash = witness::mix(hash, member.group.as_bytes());
            hash = witness::mix(hash, &member.image.identity()?.to_le_bytes());
            hash = witness::mix(hash, &member.frames.to_le_bytes());
            for level in &member.placements {
                for column in level.linear {
                    for value in column {
                        hash = witness::mix(hash, &value.to_bits().to_le_bytes());
                    }
                }
                for value in level.offset {
                    hash = witness::mix(hash, &value.to_bits().to_le_bytes());
                }
            }
        }
        Some(hash)
    }

    pub(crate) fn stamps(
        &self,
        level: usize,
        time: u32,
        channel: usize,
        x: Range<u32>,
        y: Range<u32>,
        z: Range<u32>,
    ) -> Result<Option<u64>, Error> {
        let mut hash = witness::START;
        hash = witness::mix(hash, &(level as u64).to_le_bytes());
        hash = witness::mix(hash, &u64::from(time).to_le_bytes());
        hash = witness::mix(hash, &(channel as u64).to_le_bytes());
        for member in &self.members {
            let stamp = match member.coverage(level, &[x.clone(), y.clone(), z.clone()]) {
                Some(ranges) => member.image.stamps(
                    level,
                    member.time(time),
                    channel,
                    ranges[0].clone(),
                    ranges[1].clone(),
                    ranges[2].clone(),
                )?,
                None => Some(witness::START),
            };
            let Some(stamp) = stamp else {
                return Ok(None);
            };
            hash = witness::mix(hash, &stamp.to_le_bytes());
        }
        Ok(Some(hash))
    }

    pub(crate) fn read_region(
        &self,
        time: u32,
        level: usize,
        channel: usize,
        x: Range<u32>,
        y: Range<u32>,
        z: Range<u32>,
    ) -> Result<VoxelData, Error> {
        let origin = self.root.tag(&self.group);
        let Some(geometry) = self.levels.get(level) else {
            return Err(Error::Region {
                array: origin,
                detail: "the requested resolution level does not exist".to_string(),
            });
        };
        let extent = geometry.extent();
        if time >= self.pyramid.frame_count() {
            return Err(Error::Region {
                array: origin,
                detail: "the requested frame does not exist".to_string(),
            });
        }
        if channel >= self.pyramid.channel_count() {
            return Err(Error::Region {
                array: origin,
                detail: format!(
                    "the channel {channel} lies outside the {} available channels",
                    self.pyramid.channel_count()
                ),
            });
        }
        if x.start >= x.end
            || x.end > extent.x()
            || y.start >= y.end
            || y.end > extent.y()
            || z.start >= z.end
            || z.end > extent.z()
        {
            return Err(Error::Region {
                array: origin,
                detail: "the region lies outside the array extent".to_string(),
            });
        }
        let format = self.pyramid.channels()[channel].format();
        let count = [&x, &y, &z]
            .iter()
            .map(|range| (range.end - range.start) as usize)
            .product();
        let mut output = VoxelData::blank(format, count);
        let ranges = [x, y, z];
        for member in &self.members {
            member.paint(&mut output, &ranges, level, time, channel)?;
        }
        Ok(output)
    }
}
