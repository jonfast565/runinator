//! A global immutable sorted disk index. Binary search needs O(1) user-space
//! memory and never scans historical packs. External pairwise merge-sort keeps
//! index construction bounded, at the cost of rewriting the index on commit.
use crate::{
    Id,
    error::{Result, corrupt},
    io_util::read_at,
};
use rayon::prelude::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::Arc,
};
use tempfile::{NamedTempFile, TempDir};
const MAGIC: &[u8; 8] = b"PWINDEX3";
pub const HEADER_BYTES: u64 = 24;
pub const ENTRY_BYTES: u64 = 52;
pub const STANDALONE: u32 = u32::MAX;
const MAX_PACKS: usize = 65536;
const RAW_ENTRY: usize = 84;
const SORT_RUN_ENTRIES: usize = 65_536;

fn decode_compact(bytes: [u8; ENTRY_BYTES as usize], packs: &[Id]) -> Result<Location> {
    let slot = u32::from_le_bytes(bytes[32..36].try_into().unwrap()) as usize;
    let pack = *packs
        .get(slot)
        .ok_or_else(|| corrupt("unknown compact pack slot"))?;
    Ok(Location {
        id: Id(bytes[..32].try_into().unwrap()),
        pack,
        offset: u64::from_le_bytes(bytes[36..44].try_into().unwrap()),
        length: u32::from_le_bytes(bytes[44..48].try_into().unwrap()) as u64,
        member: u32::from_le_bytes(bytes[48..52].try_into().unwrap()),
    })
}

/// Spools entries on disk while collecting only the bounded pack dictionary.

/// Both inputs are sorted; equal IDs reuse the old location.
pub fn merge(old: &DiskIndex, new: &DiskIndex, dir: &Path) -> Result<NamedTempFile> {
    let mut out = IndexWriter::new(dir)?;
    let mut old_entries = old.entries()?;
    let mut new_entries = new.entries()?;
    let mut x = old_entries.next()?;
    let mut y = new_entries.next()?;
    while x.is_some() || y.is_some() {
        match (x.take(), y.take()) {
            (Some(left), Some(right)) if left.id == right.id => {
                out.push(left)?;
                x = old_entries.next()?;
                y = new_entries.next()?;
            }
            (Some(left), Some(right)) if left.id < right.id => {
                out.push(left)?;
                x = old_entries.next()?;
                y = Some(right);
            }
            (Some(left), Some(right)) => {
                out.push(right)?;
                x = Some(left);
                y = new_entries.next()?;
            }
            (Some(left), None) => {
                out.push(left)?;
                x = old_entries.next()?;
            }
            (None, Some(right)) => {
                out.push(right)?;
                y = new_entries.next()?;
            }
            (None, None) => break,
        }
    }
    out.finish()
}
/// Spill bounded runs at a time. Pairwise merging has constant merge memory and
/// keeps at most two input runs open, even for very large transactions.

fn next_raw(file: &mut impl Read) -> Result<Option<Location>> {
    let mut b = [0; RAW_ENTRY];
    let mut used = 0;
    while used < b.len() {
        match file.read(&mut b[used..]) {
            Ok(0) if used == 0 => return Ok(None),
            Ok(0) => return Err(corrupt("truncated sort run")),
            Ok(n) => used += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Ok(Some(Location::decode(b)))
}
fn merge_raw(a: &Path, b: &Path, out: &Path) -> Result<()> {
    let mut a = BufReader::new(File::open(a)?);
    let mut b = BufReader::new(File::open(b)?);
    let mut out = BufWriter::new(File::create(out)?);
    let mut x = next_raw(&mut a)?;
    let mut y = next_raw(&mut b)?;
    while x.is_some() || y.is_some() {
        if y.is_none() || x.is_some_and(|x| x.id <= y.unwrap().id) {
            let e = x.take().unwrap();
            out.write_all(&e.bytes())?;
            x = next_raw(&mut a)?;
        } else {
            let e = y.take().unwrap();
            out.write_all(&e.bytes())?;
            y = next_raw(&mut b)?;
        }
    }
    Ok(())
}

mod location;
pub use location::Location;

mod disk_index;
pub use disk_index::DiskIndex;

mod disk_entries;
use disk_entries::DiskEntries;

mod index_writer;
pub use index_writer::IndexWriter;

mod external_sorter;
pub use external_sorter::ExternalSorter;
