use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::Error;

const END: u32 = 0x0605_4b50;
const ZIP64_END: u32 = 0x0606_4b50;
const ZIP64_LOCATOR: u32 = 0x0706_4b50;
const CENTRAL: u32 = 0x0201_4b50;
const END_LENGTH: usize = 22;
const LOCATOR_LENGTH: usize = 20;
const ZIP64_LENGTH: usize = 56;
const CENTRAL_HEADER_LENGTH: usize = 46;
const SEARCH_LENGTH: u64 = 0xFFFF + END_LENGTH as u64;
const SENTINEL: u32 = u32::MAX;
const SENTINEL_ENTRIES: u16 = u16::MAX;
const CENTRAL_LIMIT: u64 = 512 << 20;

pub(crate) fn normalize(path: &Path) -> Result<(), Error> {
    let mut file = File::open(path)
        .map_err(|error| Error::storage(format!("`{}`: {error}", path.display())))?;
    let length = file
        .metadata()
        .map_err(|error| Error::storage(format!("`{}`: {error}", path.display())))?
        .len();
    let tail_length = length.min(SEARCH_LENGTH) as usize;
    let mut tail = vec![0u8; tail_length];
    file.seek(SeekFrom::Start(length - tail_length as u64))
        .and_then(|_| file.read_exact(&mut tail))
        .map_err(|error| Error::storage(format!("`{}`: {error}", path.display())))?;
    let position = find_end(&tail)?;
    if position < LOCATOR_LENGTH {
        return Ok(());
    }
    let locator = Record::new(&tail[position - LOCATOR_LENGTH..position]);
    if locator.word(0) != ZIP64_LOCATOR {
        return Ok(());
    }
    let end = Record::new(&tail[position..position + END_LENGTH]);
    if locator.word(16) != 1 {
        return Err(Error::archive("the archive spans multiple disks"));
    }
    let zip64_offset = locator.long(8);
    let locator_offset = length - tail_length as u64 + (position as u64 - LOCATOR_LENGTH as u64);
    if zip64_offset
        .checked_add(ZIP64_LENGTH as u64)
        .is_none_or(|end| end != locator_offset)
    {
        return Ok(());
    }
    let mut header = vec![0u8; ZIP64_LENGTH];
    file.seek(SeekFrom::Start(zip64_offset))
        .and_then(|_| file.read_exact(&mut header))
        .map_err(|error| Error::storage(format!("`{}`: {error}", path.display())))?;
    let zip64 = Record::new(&header);
    if zip64.word(0) != ZIP64_END || zip64.long(4) != 44 {
        return Err(Error::archive("the zip64 record is malformed"));
    }
    if zip64.word(16) != 0 || zip64.word(20) != 0 {
        return Err(Error::archive("the archive spans multiple disks"));
    }
    let entries = zip64.long(24);
    let total = zip64.long(32);
    let central_size = zip64.long(40);
    let central_offset = zip64.long(48);
    if entries != total {
        return Err(Error::archive("the zip64 record counts disagree"));
    }
    agree(
        end.half(10),
        zip64_entries(entries),
        end.half(10) == SENTINEL_ENTRIES,
    )?;
    agree(
        end.word(12),
        zip64_word(central_size),
        end.word(12) == SENTINEL,
    )?;
    agree(
        end.word(16),
        zip64_word(central_offset),
        end.word(16) == SENTINEL,
    )?;
    let stored_entries = u16::try_from(entries).map_err(|_| {
        Error::archive("the checkpoint holds more entries than a classic archive names")
    })?;
    let stored_size = u32::try_from(central_size).map_err(|_| {
        Error::archive("the checkpoint central directory exceeds a classic archive")
    })?;
    let stored_offset = u32::try_from(central_offset).map_err(|_| {
        Error::archive("the checkpoint central directory lies beyond a classic archive")
    })?;
    if central_offset
        .checked_add(central_size)
        .is_none_or(|end| end > zip64_offset)
    {
        return Err(Error::archive(
            "the central directory overruns its zip64 record",
        ));
    }
    check_central(&mut file, central_offset, central_size, entries, path)?;
    drop(file);
    let mut file = OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(|error| Error::storage(format!("`{}`: {error}", path.display())))?;
    let target = central_offset + central_size;
    let mut record = vec![0u8; END_LENGTH];
    record[0..4].copy_from_slice(&END.to_le_bytes());
    record[8..10].copy_from_slice(&stored_entries.to_le_bytes());
    record[10..12].copy_from_slice(&stored_entries.to_le_bytes());
    record[12..16].copy_from_slice(&stored_size.to_le_bytes());
    record[16..20].copy_from_slice(&stored_offset.to_le_bytes());
    file.seek(SeekFrom::Start(target))
        .and_then(|_| file.write_all(&record))
        .and_then(|_| file.set_len(target + END_LENGTH as u64))
        .and_then(|_| file.flush())
        .map_err(|error| Error::storage(format!("`{}`: {error}", path.display())))?;
    Ok(())
}

fn agree<T: Copy + PartialEq>(classic: T, zip64: T, sentinel: bool) -> Result<(), Error> {
    if !sentinel && classic != zip64 {
        return Err(Error::archive(
            "the zip64 record disagrees with its trailing record",
        ));
    }
    Ok(())
}

fn zip64_entries(entries: u64) -> u16 {
    u16::try_from(entries).unwrap_or(SENTINEL_ENTRIES)
}

fn zip64_word(value: u64) -> u32 {
    u32::try_from(value).unwrap_or(SENTINEL)
}

fn find_end(tail: &[u8]) -> Result<usize, Error> {
    if tail.len() < END_LENGTH {
        return Err(Error::archive(
            "the checkpoint is shorter than its trailing record",
        ));
    }
    for position in (0..=tail.len() - END_LENGTH).rev() {
        let record = Record::new(&tail[position..position + END_LENGTH]);
        if record.word(0) != END {
            continue;
        }
        let comment = record.half(20) as usize;
        if position + END_LENGTH + comment == tail.len() {
            if record.half(4) != 0 || record.half(6) != 0 {
                return Err(Error::archive("the archive spans multiple disks"));
            }
            return Ok(position);
        }
    }
    Err(Error::archive(
        "the checkpoint has no trailing archive record",
    ))
}

fn check_central(
    file: &mut File,
    offset: u64,
    size: u64,
    entries: u64,
    path: &Path,
) -> Result<(), Error> {
    if size > CENTRAL_LIMIT {
        return Err(Error::archive(
            "the central directory is unreasonably large",
        ));
    }
    let mut directory = vec![0u8; size as usize];
    file.seek(SeekFrom::Start(offset))
        .and_then(|_| file.read_exact(&mut directory))
        .map_err(|error| Error::storage(format!("`{}`: {error}", path.display())))?;
    let mut position = 0usize;
    let mut count = 0u64;
    while position + CENTRAL_HEADER_LENGTH <= directory.len() {
        let record = Record::new(&directory[position..position + CENTRAL_HEADER_LENGTH]);
        if record.word(0) != CENTRAL {
            return Err(Error::archive("the central directory is malformed"));
        }
        if record.word(20) == SENTINEL || record.word(24) == SENTINEL || record.word(42) == SENTINEL
        {
            return Err(Error::archive(
                "the checkpoint holds an entry a classic archive cannot address",
            ));
        }
        let name = record.half(28) as usize;
        let extra = record.half(30) as usize;
        let comment = record.half(32) as usize;
        position += CENTRAL_HEADER_LENGTH + name + extra + comment;
        count += 1;
    }
    if position != directory.len() || count != entries {
        return Err(Error::archive(
            "the central directory does not match its record",
        ));
    }
    Ok(())
}

struct Record<'a> {
    bytes: &'a [u8],
}

impl<'a> Record<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }

    fn half(&self, offset: usize) -> u16 {
        u16::from_le_bytes([self.bytes[offset], self.bytes[offset + 1]])
    }

    fn word(&self, offset: usize) -> u32 {
        u32::from_le_bytes([
            self.bytes[offset],
            self.bytes[offset + 1],
            self.bytes[offset + 2],
            self.bytes[offset + 3],
        ])
    }

    fn long(&self, offset: usize) -> u64 {
        u64::from_le_bytes([
            self.bytes[offset],
            self.bytes[offset + 1],
            self.bytes[offset + 2],
            self.bytes[offset + 3],
            self.bytes[offset + 4],
            self.bytes[offset + 5],
            self.bytes[offset + 6],
            self.bytes[offset + 7],
        ])
    }
}
