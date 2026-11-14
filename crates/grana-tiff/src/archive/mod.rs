mod layout;
mod tile;

use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use grana_archive::{Fill, ImageSpec, Written};

use crate::Error;
use tile::{LevelInfo, Request, Tile};

const ALIGN: u64 = 8;
const ZEROS: [u8; 8] = [0; 8];
const ALIGNMENT: f32 = 1.0e-5;

pub fn write_archive(path: &Path, spec: &mut ImageSpec<'_>) -> Result<Written, crate::Error> {
    spec.check();
    validate(path, spec)?;
    let levels = spec
        .levels
        .iter()
        .copied()
        .map(LevelInfo::new)
        .collect::<Vec<_>>();
    let base = levels[0];
    let planes = u64::from(spec.frames) * spec.channels.len() as u64 * u64::from(base.depth());
    let per_plane = levels.iter().map(|level| level.tiles() as u64).sum::<u64>();
    let total = usize::try_from(planes * per_plane)
        .expect("an archive writes at most this platform's tiles");
    let description = layout::description(spec);
    let atomic =
        grana_file::Atomic::file(path).map_err(|error| Error::write(path, error.to_string()))?;
    let file =
        File::create(atomic.staging()).map_err(|error| Error::write(path, error.to_string()))?;
    let mut sink = Sink {
        file: BufWriter::new(file),
        path: path.to_path_buf(),
        cursor: 0,
        done: 0,
        total,
        tile: Tile::default(),
    };
    sink.write(&layout::header())?;
    let mut pointer: Option<u64> = None;
    for frame in 0..spec.frames {
        for channel in 0..spec.channels.len() {
            for z in 0..base.depth() {
                let at = align(sink.cursor);
                sink.pad(at)?;
                if let Some(held) = pointer {
                    sink.patch(held, &at.to_le_bytes())?;
                }
                let first = frame == 0 && channel == 0 && z == 0;
                let (block, spots) = layout::page(
                    at,
                    base.width(),
                    base.height(),
                    base.tiles(),
                    levels.len() - 1,
                    first.then_some(description.as_str()),
                );
                sink.write(&block)?;
                let request = Request {
                    level: 0,
                    frame,
                    channel,
                    z,
                };
                let (offsets, counts) = sink.tiles(&request, base, spec.fill, spec.progress)?;
                sink.patch(spots.offsets, &offsets)?;
                sink.patch(spots.counts, &counts)?;
                let mut sub_ifds = Vec::with_capacity(levels.len() - 1);
                for (index, level) in levels.iter().enumerate().skip(1) {
                    let at = align(sink.cursor);
                    sink.pad(at)?;
                    let (block, spots) =
                        layout::sub_page(at, level.width(), level.height(), level.tiles());
                    sink.write(&block)?;
                    let request = Request {
                        level: index,
                        frame,
                        channel,
                        z,
                    };
                    let (offsets, counts) =
                        sink.tiles(&request, *level, spec.fill, spec.progress)?;
                    sink.patch(spots.offsets, &offsets)?;
                    sink.patch(spots.counts, &counts)?;
                    sub_ifds.push(at);
                }
                if let Some(held) = spots.sub_ifds {
                    let mut bytes = Vec::with_capacity(sub_ifds.len() * 8);
                    for at in &sub_ifds {
                        bytes.extend_from_slice(&at.to_le_bytes());
                    }
                    sink.patch(held, &bytes)?;
                }
                assert_eq!(
                    spots.sub_ifds.is_some(),
                    !sub_ifds.is_empty(),
                    "a TIFF page points at every reduced resolution level"
                );
                pointer = Some(spots.next);
            }
        }
    }
    let bytes = sink.finish()?;
    atomic
        .commit()
        .map_err(|error| Error::write(path, error.to_string()))?;
    Ok(Written::new(
        path.to_path_buf(),
        spec.name.to_string(),
        spec.channels.len(),
        spec.frames,
        spec.levels.len(),
        bytes,
    ))
}

fn validate(path: &Path, spec: &ImageSpec<'_>) -> Result<(), Error> {
    for (index, level) in spec.levels.iter().enumerate() {
        let basis = level.basis();
        for axis in 0..3 {
            let column = basis.column(axis);
            let norm = column.iter().map(|value| value * value).sum::<f32>().sqrt();
            let world = column
                .iter()
                .filter(|value| value.abs() > ALIGNMENT * norm)
                .count();
            if world > 1 {
                return Err(Error::write(
                    path,
                    format!(
                        "resolution level {index} rotates the {} axis over {world} world axes, which an OME-TIFF cannot represent",
                        ["x", "y", "z"][axis]
                    ),
                ));
            }
        }
    }
    for (index, pair) in spec.levels.windows(2).enumerate() {
        let fine = pair[0].extent();
        let coarse = pair[1].extent();
        if coarse.z() != fine.z() {
            return Err(Error::write(
                path,
                format!(
                    "resolution level {} stores {} planes where level {index} stores {}, and an OME-TIFF keeps one depth for every level",
                    index + 1,
                    coarse.z(),
                    fine.z()
                ),
            ));
        }
        if coarse.x() > fine.x() || coarse.y() > fine.y() || coarse == fine {
            return Err(Error::write(
                path,
                format!(
                    "resolution level {} does not store fewer voxels than level {index}",
                    index + 1
                ),
            ));
        }
    }
    Ok(())
}

fn align(at: u64) -> u64 {
    at.div_ceil(ALIGN) * ALIGN
}

fn pack(values: &[u64]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(values.len() * 8);
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

struct Sink {
    file: BufWriter<File>,
    path: PathBuf,
    cursor: u64,
    done: usize,
    total: usize,
    tile: Tile,
}

impl Sink {
    fn write(&mut self, bytes: &[u8]) -> Result<(), Error> {
        self.file
            .write_all(bytes)
            .map_err(|error| Error::write(&self.path, error.to_string()))?;
        self.cursor += bytes.len() as u64;
        Ok(())
    }

    fn pad(&mut self, at: u64) -> Result<(), Error> {
        assert!(at >= self.cursor, "an archive writes its pages in order");
        let gap = usize::try_from(at - self.cursor)
            .expect("an archive pads less than this platform addresses");
        if gap > 0 {
            self.write(&ZEROS[..gap])?;
        }
        Ok(())
    }

    fn patch(&mut self, at: u64, bytes: &[u8]) -> Result<(), Error> {
        let end = self.cursor;
        self.file
            .seek(SeekFrom::Start(at))
            .map_err(|error| Error::write(&self.path, error.to_string()))?;
        self.file
            .write_all(bytes)
            .map_err(|error| Error::write(&self.path, error.to_string()))?;
        self.file
            .seek(SeekFrom::Start(end))
            .map_err(|error| Error::write(&self.path, error.to_string()))?;
        Ok(())
    }

    fn tiles(
        &mut self,
        request: &Request,
        info: LevelInfo,
        fill: &mut Fill<'_>,
        progress: &mut dyn FnMut(usize, usize),
    ) -> Result<(Vec<u8>, Vec<u8>), Error> {
        let mut offsets = Vec::with_capacity(info.tiles());
        let mut counts = Vec::with_capacity(info.tiles());
        for row in 0..info.down() {
            for column in 0..info.across() {
                let length = self
                    .tile
                    .encode(request, info, column, row, fill)
                    .map_err(|detail| Error::write(&self.path, detail))?;
                offsets.push(self.cursor);
                counts.push(length);
                self.file
                    .write_all(self.tile.bytes())
                    .map_err(|error| Error::write(&self.path, error.to_string()))?;
                self.cursor += length;
                self.done += 1;
                progress(self.done, self.total);
            }
        }
        Ok((pack(&offsets), pack(&counts)))
    }

    fn finish(mut self) -> Result<u64, Error> {
        let end = self
            .file
            .seek(SeekFrom::End(0))
            .map_err(|error| Error::write(&self.path, error.to_string()))?;
        assert_eq!(
            end, self.cursor,
            "the archive length matches the written layout"
        );
        let file = self
            .file
            .into_inner()
            .map_err(|error| Error::write(&self.path, error.to_string()))?;
        file.sync_all()
            .map_err(|error| Error::write(&self.path, error.to_string()))?;
        Ok(end)
    }
}
