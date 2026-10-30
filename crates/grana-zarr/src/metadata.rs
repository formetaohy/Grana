use std::path::Path;

use ome_zarr_metadata::AnyOmeFields;
use ome_zarr_metadata::v0_4::{
    Axis, Channel, CoordinateTransform, CoordinateTransformScale, MultiscaleImage,
    MultiscaleImageDataset, Omero,
};
use ome_zarr_metadata::v0_5::MultiscaleImage as MultiscaleImageV5;
use serde_json::{Map, Value};

use crate::Error;

#[derive(Clone, Debug)]
pub struct Metadata {
    pub name: Option<String>,
    pub axes: Vec<Axis>,
    pub datasets: Vec<Dataset>,
    pub channels: Vec<ChannelMetadata>,
}

#[derive(Clone, Debug)]
pub struct Dataset {
    pub path: String,
    pub scale: Vec<f32>,
}

#[derive(Clone, Debug)]
pub struct ChannelMetadata {
    pub label: Option<String>,
    pub tint: [f32; 3],
    pub window: Option<[f32; 2]>,
}

pub fn parse(path: &Path, attributes: &Map<String, Value>) -> Result<Metadata, Error> {
    let fields: AnyOmeFields = serde_json::from_value(Value::Object(attributes.clone()))
        .map_err(|error| metadata_error(path, error.to_string()))?;
    match fields {
        AnyOmeFields::V0_4(attributes) => {
            let collection = attributes.bioformats2raw.is_some();
            let plate = attributes.plate.is_some();
            let omero = attributes.omero;
            let multiscales = attributes.multiscales;
            let image = image(path, multiscales, collection, plate)?;
            assemble(
                path,
                image.name.clone(),
                image.axes.clone(),
                image.datasets.clone(),
                image.coordinate_transformations.clone(),
                omero,
            )
        }
        AnyOmeFields::V0_5(attributes) => {
            let collection = attributes.bioformats2raw.is_some();
            let plate = attributes.plate.is_some();
            let omero = attributes.omero;
            let multiscales = attributes.multiscales;
            let image = image_v5(path, multiscales, collection, plate)?;
            assemble(
                path,
                image.name.clone(),
                image.axes.clone(),
                image.datasets.clone(),
                image.coordinate_transformations.clone(),
                omero,
            )
        }
    }
}

fn image(
    path: &Path,
    multiscales: Option<Vec<MultiscaleImage>>,
    collection: bool,
    plate: bool,
) -> Result<MultiscaleImage, Error> {
    multiscales
        .and_then(|multiscales| multiscales.into_iter().next())
        .ok_or_else(|| absent(path, collection, plate))
}

fn image_v5(
    path: &Path,
    multiscales: Option<Vec<MultiscaleImageV5>>,
    collection: bool,
    plate: bool,
) -> Result<MultiscaleImageV5, Error> {
    multiscales
        .and_then(|multiscales| multiscales.into_iter().next())
        .ok_or_else(|| absent(path, collection, plate))
}

fn absent(path: &Path, collection: bool, plate: bool) -> Error {
    let detail = if collection {
        "the dataset is a bioformats2raw image collection; open one of its image groups instead"
    } else if plate {
        "high content screening plates are not supported; open a field of view instead"
    } else {
        "the group has no multiscales metadata"
    };
    metadata_error(path, detail)
}

fn assemble(
    path: &Path,
    name: Option<String>,
    axes: Vec<Axis>,
    datasets: Vec<MultiscaleImageDataset>,
    transforms: Option<Vec<CoordinateTransform>>,
    omero: Option<Omero>,
) -> Result<Metadata, Error> {
    if datasets.is_empty() {
        return Err(metadata_error(path, "the multiscales list has no datasets"));
    }
    let image_scale = scale(path, transforms.as_deref().unwrap_or_default())?;
    let datasets = datasets
        .into_iter()
        .map(|dataset| {
            let dataset_scale =
                scale(path, &dataset.coordinate_transformations)?.ok_or_else(|| {
                    metadata_error(
                        path,
                        format!("the array `{}` has no scale transformation", dataset.path),
                    )
                })?;
            let combined = match &image_scale {
                Some(image_scale) => multiply(path, image_scale, &dataset_scale)?,
                None => dataset_scale,
            };
            if combined.len() != axes.len() {
                return Err(metadata_error(
                    path,
                    format!(
                        "the scale of the array `{}` has {} components for {} axes",
                        dataset.path,
                        combined.len(),
                        axes.len()
                    ),
                ));
            }
            Ok(Dataset {
                path: dataset.path,
                scale: combined,
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;
    let channels = omero
        .map(|omero| omero.channels.into_iter().map(channel).collect())
        .unwrap_or_default();
    Ok(Metadata {
        name,
        axes,
        datasets,
        channels,
    })
}

fn channel(channel: Channel) -> ChannelMetadata {
    let window = [channel.window.start, channel.window.end];
    let window = (window[0].is_finite() && window[1].is_finite() && window[0] < window[1])
        .then_some([window[0] as f32, window[1] as f32]);
    ChannelMetadata {
        label: channel
            .other
            .get("label")
            .and_then(Value::as_str)
            .map(str::to_owned),
        tint: [
            f32::from(channel.color.r) / 255.0,
            f32::from(channel.color.g) / 255.0,
            f32::from(channel.color.b) / 255.0,
        ],
        window,
    }
}

fn scale(path: &Path, transforms: &[CoordinateTransform]) -> Result<Option<Vec<f32>>, Error> {
    let mut scale: Option<Vec<f32>> = None;
    for transform in transforms {
        match transform {
            CoordinateTransform::Scale(CoordinateTransformScale::List { scale: values }) => {
                scale = Some(match scale {
                    Some(current) => multiply(path, &current, values)?,
                    None => values.clone(),
                });
            }
            CoordinateTransform::Scale(CoordinateTransformScale::Path { .. }) => {
                return Err(metadata_error(
                    path,
                    "scale transformations stored in array paths are not supported",
                ));
            }
            _ => {}
        }
    }
    Ok(scale)
}

fn multiply(path: &Path, left: &[f32], right: &[f32]) -> Result<Vec<f32>, Error> {
    if left.len() != right.len() {
        return Err(metadata_error(
            path,
            format!(
                "scale transformations of {} and {} components cannot be combined",
                left.len(),
                right.len()
            ),
        ));
    }
    Ok(left
        .iter()
        .zip(right)
        .map(|(left, right)| left * right)
        .collect())
}

fn metadata_error(path: &Path, detail: impl Into<String>) -> Error {
    Error::Metadata {
        path: path.to_path_buf(),
        detail: detail.into(),
    }
}
