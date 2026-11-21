use std::collections::BTreeMap;

use grana_volume::Extent;
use serde_json::{Map, Value};
use zarrs::storage::{Bytes, StoreKey, StorePrefix};

use crate::Error;
use crate::hcs;
use crate::outline::Outline;
use crate::root::Root;

const LAYOUT: &str = "bioformats2raw.layout";
const LAYOUT_VERSION: u64 = 3;
const OME_GROUP: &str = "/OME";
const OME_METADATA: &str = "OME/METADATA.ome.xml";
const MARKERS: [&str; 2] = ["zarr.json", ".zgroup"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Series {
    path: String,
    name: Option<String>,
    outline: Option<Outline>,
}

impl Series {
    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    pub fn outline(&self) -> Option<Outline> {
        self.outline
    }
}

#[derive(Clone, Debug)]
pub struct Collection {
    name: String,
    series: Vec<Series>,
}

impl Collection {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn series(&self) -> &[Series] {
        &self.series
    }

    pub fn series_at(&self, index: usize) -> Option<&Series> {
        self.series.get(index)
    }
}

pub fn collection(root: &Root) -> Result<Option<Collection>, Error> {
    let attributes = hcs::attributes(root, "/")?;
    let fields = hcs::fields(&attributes);
    if hcs::Kind::of(fields) != hcs::Kind::Collection {
        return Ok(None);
    }
    if fields.get(LAYOUT).and_then(Value::as_u64) != Some(LAYOUT_VERSION) {
        return Ok(None);
    }
    let origin = root.tag("/");
    let images = images(root)?;
    let paths = match declared(root)? {
        Some(paths) => {
            if let Some(images) = &images
                && images.len() != paths.len()
            {
                return Err(Error::Metadata {
                    path: origin,
                    detail: format!(
                        "the collection lists {} series and its OME metadata describes {} images",
                        paths.len(),
                        images.len()
                    ),
                });
            }
            paths
        }
        None => match &images {
            Some(images) => numbered(images.len()),
            None => listed(root)?,
        },
    };
    let series = paths
        .into_iter()
        .enumerate()
        .map(|(index, path)| {
            let image = images.as_ref().and_then(|images| images.get(index));
            let name = image
                .and_then(|image| image.name.clone())
                .filter(|name| !name.is_empty());
            let outline = image.and_then(outline_of);
            Series {
                path,
                name,
                outline,
            }
        })
        .collect();
    Ok(Some(Collection {
        name: root.name(),
        series,
    }))
}

fn declared(root: &Root) -> Result<Option<Vec<String>>, Error> {
    let Some(attributes) = ome_attributes(root)? else {
        return Ok(None);
    };
    let Some(series) = hcs::fields(&attributes).get("series") else {
        return Ok(None);
    };
    let origin = root.tag(OME_GROUP);
    let entries = series.as_array().ok_or_else(|| Error::Metadata {
        path: origin.clone(),
        detail: "the series metadata is not a list of image paths".to_string(),
    })?;
    if entries.is_empty() {
        return Err(Error::Metadata {
            path: origin,
            detail: "the series metadata lists no image path".to_string(),
        });
    }
    let paths = entries
        .iter()
        .map(|entry| {
            let path = entry.as_str().ok_or_else(|| Error::Metadata {
                path: origin.clone(),
                detail: "a series image path is not a string".to_string(),
            })?;
            image_group(path).ok_or_else(|| Error::Metadata {
                path: origin.clone(),
                detail: format!("the series image path `{path}` is not a relative group path"),
            })
        })
        .collect::<Result<Vec<String>, Error>>()?;
    Ok(Some(paths))
}

fn image_group(path: &str) -> Option<String> {
    let trimmed = path.trim_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    let segments = trimmed.split('/').collect::<Vec<_>>();
    if segments
        .iter()
        .any(|segment| segment.is_empty() || *segment == "." || *segment == "..")
    {
        return None;
    }
    Some(format!("/{trimmed}"))
}

fn numbered(count: usize) -> Vec<String> {
    (0..count).map(|index| format!("/{index}")).collect()
}

fn listed(root: &Root) -> Result<Vec<String>, Error> {
    let listing = root
        .listable()
        .list_dir(&StorePrefix::root())
        .map_err(|error| Error::Open {
            path: root.tag("/"),
            detail: format!("failed to list the image groups of the collection: {error}"),
        })?;
    let mut groups = BTreeMap::new();
    for prefix in listing.prefixes() {
        let name = prefix.as_str().trim_end_matches('/');
        if let Ok(index) = name.parse::<usize>()
            && name == index.to_string()
        {
            groups.insert(index, format!("/{name}"));
        }
    }
    let paths = groups.into_values().collect::<Vec<_>>();
    if paths.is_empty() {
        return Err(Error::Metadata {
            path: root.tag("/"),
            detail: "the collection holds no OME metadata and no numbered image group".to_string(),
        });
    }
    let consecutive = paths
        .iter()
        .enumerate()
        .all(|(index, path)| path == &format!("/{index}"));
    if !consecutive {
        return Err(Error::Metadata {
            path: root.tag("/"),
            detail: "the image groups of the collection are not numbered consecutively from zero"
                .to_string(),
        });
    }
    Ok(paths)
}

fn images(root: &Root) -> Result<Option<Vec<grana_ome::Image>>, Error> {
    let Some(bytes) = read(root, OME_METADATA)? else {
        return Ok(None);
    };
    let text = std::str::from_utf8(&bytes).map_err(|error| Error::Metadata {
        path: root.tag(OME_METADATA),
        detail: format!("the OME metadata is not valid UTF-8: {error}"),
    })?;
    let images = grana_ome::parse(text).map_err(|detail| Error::Metadata {
        path: root.tag(OME_METADATA),
        detail,
    })?;
    if images.is_empty() {
        return Err(Error::Metadata {
            path: root.tag(OME_METADATA),
            detail: "the OME metadata describes no image".to_string(),
        });
    }
    Ok(Some(images))
}

fn outline_of(image: &grana_ome::Image) -> Option<Outline> {
    let extent = Extent::new(
        positive(image.size_x?)?,
        positive(image.size_y?)?,
        image.size_z.and_then(positive).unwrap_or(1),
    );
    let channels = image
        .size_c
        .and_then(positive)
        .unwrap_or_else(|| u32::try_from(image.channels.len()).unwrap_or(1))
        .max(1);
    let frames = image.size_t.and_then(positive).unwrap_or(1);
    Some(Outline::new(extent, channels, frames))
}

fn positive(value: u32) -> Option<u32> {
    (value > 0).then_some(value)
}

fn ome_attributes(root: &Root) -> Result<Option<Map<String, Value>>, Error> {
    for marker in MARKERS {
        if read(root, &joined(OME_GROUP, marker))?.is_some() {
            return hcs::attributes(root, OME_GROUP).map(Some);
        }
    }
    Ok(None)
}

fn joined(group: &str, name: &str) -> String {
    hcs::join(group, name).trim_start_matches('/').to_string()
}

fn read(root: &Root, key: &str) -> Result<Option<Bytes>, Error> {
    let key = StoreKey::new(key.trim_start_matches('/')).map_err(|error| Error::Open {
        path: root.tag("/"),
        detail: error.to_string(),
    })?;
    root.readable().get(&key).map_err(|error| Error::Read {
        array: key.as_str().to_string(),
        detail: error.to_string(),
    })
}
