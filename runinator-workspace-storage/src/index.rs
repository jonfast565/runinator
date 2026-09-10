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
#[derive(Clone, Copy, Debug)]
pub struct Location {
    pub id: Id,
    pub pack: Id,
    pub offset: u64,
    pub length: u64,
    /// STANDALONE, or a member slot in a physical TinyBlock record.
    pub member: u32,
}
impl Location {
    pub fn bytes(self) -> [u8; 84] {
        let mut b = [0; 84];
        b[..32].copy_from_slice(&self.id.0);
        b[32..64].copy_from_slice(&self.pack.0);
        b[64..72].copy_from_slice(&self.offset.to_le_bytes());
        b[72..80].copy_from_slice(&self.length.to_le_bytes());
        b[80..84].copy_from_slice(&self.member.to_le_bytes());
        b
    }
    fn decode(b: [u8; 84]) -> Self {
        Self {
            id: Id(b[..32].try_into().unwrap()),
            pack: Id(b[32..64].try_into().unwrap()),
            offset: u64::from_le_bytes(b[64..72].try_into().unwrap()),
            length: u64::from_le_bytes(b[72..80].try_into().unwrap()),
            member: u32::from_le_bytes(b[80..84].try_into().unwrap()),
        }
    }
}
#[derive(Clone, Default)]
pub struct DiskIndex {
    file: Option<Arc<File>>,
    packs: Arc<Vec<Id>>,
    entries_offset: u64,
    pub count: u64,
}
impl DiskIndex {
    pub fn open(path: &Path) -> Result<Self> {
        let file = File::open(path)?;
        let mut header = [0; 24];
        read_at(&file, &mut header, 0)?;
        if &header[..8] != MAGIC {
            return Err(corrupt("bad index header"));
        }
        let count = u64::from_le_bytes(header[8..16].try_into().unwrap());
        let pack_count = u32::from_le_bytes(header[16..20].try_into().unwrap()) as usize;
        let stride = u32::from_le_bytes(header[20..24].try_into().unwrap());
        if pack_count > MAX_PACKS
            || stride as u64 != ENTRY_BYTES
            || (count == 0) != (pack_count == 0)
        {
            return Err(corrupt("invalid compact index table"));
        }
        let entries_offset = HEADER_BYTES + pack_count as u64 * 32;
        let length = count
            .checked_mul(ENTRY_BYTES)
            .and_then(|n| n.checked_add(entries_offset))
            .ok_or_else(|| corrupt("index size overflow"))?;
        if file.metadata()?.len() != length {
            return Err(corrupt("incorrect index length"));
        }
        let mut pack_bytes = vec![0; pack_count * 32];
        read_at(&file, &mut pack_bytes, HEADER_BYTES)?;
        let mut packs = Vec::with_capacity(pack_count);
        for bytes in pack_bytes.chunks_exact(32) {
            let id = Id(bytes.try_into().unwrap());
            if packs.last().is_some_and(|p| *p >= id) {
                return Err(corrupt("unordered index pack table"));
            }
            packs.push(id);
        }
        Ok(Self {
            file: Some(Arc::new(file)),
            packs: Arc::new(packs),
            entries_offset,
            count,
        })
    }
    pub fn entry(&self, i: u64) -> Result<Location> {
        if i >= self.count {
            return Err(corrupt("index entry outside bounds"));
        }
        let mut b = [0; 52];
        read_at(
            self.file
                .as_ref()
                .ok_or_else(|| corrupt("missing index file"))?,
            &mut b,
            self.entries_offset + i * ENTRY_BYTES,
        )?;
        let slot = u32::from_le_bytes(b[32..36].try_into().unwrap()) as usize;
        let pack = *self
            .packs
            .get(slot)
            .ok_or_else(|| corrupt("unknown compact pack slot"))?;
        Ok(Location {
            id: Id(b[..32].try_into().unwrap()),
            pack,
            offset: u64::from_le_bytes(b[36..44].try_into().unwrap()),
            length: u32::from_le_bytes(b[44..48].try_into().unwrap()) as u64,
            member: u32::from_le_bytes(b[48..52].try_into().unwrap()),
        })
    }
    pub fn lookup(&self, id: Id) -> Result<Option<Location>> {
        let (mut lo, mut hi) = (0, self.count);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let e = self.entry(mid)?;
            match e.id.cmp(&id) {
                std::cmp::Ordering::Equal => return Ok(Some(e)),
                std::cmp::Ordering::Less => lo = mid + 1,
                std::cmp::Ordering::Greater => hi = mid,
            }
        }
        Ok(None)
    }
    pub fn validate(&self) -> Result<()> {
        let mut previous = None;
        let mut entries = self.entries()?;
        while let Some(e) = entries.next()? {
            if previous.is_some_and(|id| id >= e.id) {
                return Err(corrupt("index is not strictly sorted"));
            }
            previous = Some(e.id);
        }
        Ok(())
    }
    pub fn pack_count(&self) -> usize {
        self.packs.len()
    }

    fn entries(&self) -> Result<DiskEntries> {
        if self.count == 0 {
            return Ok(DiskEntries {
                reader: None,
                packs: self.packs.clone(),
                remaining: 0,
            });
        }
        let mut file = self
            .file
            .as_ref()
            .ok_or_else(|| corrupt("missing index file"))?
            .try_clone()?;
        file.seek(SeekFrom::Start(self.entries_offset))?;
        Ok(DiskEntries {
            reader: Some(BufReader::new(file)),
            packs: self.packs.clone(),
            remaining: self.count,
        })
    }
}

struct DiskEntries {
    reader: Option<BufReader<File>>,
    packs: Arc<Vec<Id>>,
    remaining: u64,
}

impl DiskEntries {
    fn next(&mut self) -> Result<Option<Location>> {
        if self.remaining == 0 {
            return Ok(None);
        }
        let mut bytes = [0; ENTRY_BYTES as usize];
        self.reader
            .as_mut()
            .ok_or_else(|| corrupt("missing index reader"))?
            .read_exact(&mut bytes)?;
        self.remaining -= 1;
        decode_compact(bytes, &self.packs).map(Some)
    }
}

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
pub struct IndexWriter {
    file: BufWriter<NamedTempFile>,
    directory: PathBuf,
    packs: BTreeSet<Id>,
    count: u64,
    last: Option<Id>,
}
impl IndexWriter {
    pub fn new(dir: &Path) -> Result<Self> {
        Ok(Self {
            file: BufWriter::new(NamedTempFile::new_in(dir)?),
            directory: dir.to_owned(),
            packs: BTreeSet::new(),
            count: 0,
            last: None,
        })
    }
    pub fn push(&mut self, entry: Location) -> Result<()> {
        if self.last.is_some_and(|id| id >= entry.id) {
            return Err(corrupt("duplicate/unordered index insertion"));
        }
        if entry.length > u32::MAX as u64 {
            return Err(corrupt("record too large for compact index"));
        }
        if !self.packs.contains(&entry.pack) && self.packs.len() == MAX_PACKS {
            return Err(corrupt("compact index pack limit exceeded"));
        }
        self.packs.insert(entry.pack);
        self.file.write_all(&entry.bytes())?;
        self.last = Some(entry.id);
        self.count = self
            .count
            .checked_add(1)
            .ok_or_else(|| corrupt("index count overflow"))?;
        Ok(())
    }
    pub fn finish(mut self) -> Result<NamedTempFile> {
        let mut out = NamedTempFile::new_in(&self.directory)?;
        let mut dictionary = BTreeMap::new();
        self.file.flush()?;
        let mut input = BufReader::new(self.file.into_inner().map_err(|error| error.into_error())?);
        input.seek(SeekFrom::Start(0))?;
        {
            let mut writer = BufWriter::new(&mut out);
            writer.write_all(MAGIC)?;
            writer.write_all(&self.count.to_le_bytes())?;
            writer.write_all(&(self.packs.len() as u32).to_le_bytes())?;
            writer.write_all(&(ENTRY_BYTES as u32).to_le_bytes())?;
            for (slot, pack) in self.packs.iter().enumerate() {
                dictionary.insert(*pack, slot as u32);
                writer.write_all(&pack.0)?;
            }
            while let Some(entry) = next_raw(&mut input)? {
                writer.write_all(&entry.id.0)?;
                let slot = dictionary
                    .get(&entry.pack)
                    .ok_or_else(|| corrupt("missing pack table entry"))?;
                writer.write_all(&slot.to_le_bytes())?;
                writer.write_all(&entry.offset.to_le_bytes())?;
                writer.write_all(&(entry.length as u32).to_le_bytes())?;
                writer.write_all(&entry.member.to_le_bytes())?;
            }
            writer.flush()?;
        }
        Ok(out)
    }
}
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
pub struct ExternalSorter {
    dir: TempDir,
    buffer: Vec<Location>,
    runs: u64,
}
impl ExternalSorter {
    pub fn new(dir: &Path) -> Result<Self> {
        Ok(Self {
            dir: TempDir::new_in(dir)?,
            buffer: Vec::with_capacity(SORT_RUN_ENTRIES),
            runs: 0,
        })
    }
    fn path(&self, round: u64, run: u64) -> PathBuf {
        self.dir.path().join(format!("{round}-{run}"))
    }
    pub fn push(&mut self, e: Location) -> Result<()> {
        self.buffer.push(e);
        if self.buffer.len() == SORT_RUN_ENTRIES {
            self.flush()?;
        }
        Ok(())
    }
    fn flush(&mut self) -> Result<()> {
        if self.buffer.is_empty() {
            return Ok(());
        }
        self.buffer.sort_unstable_by_key(|e| e.id);
        let mut out = BufWriter::new(File::create(self.path(0, self.runs))?);
        for e in self.buffer.drain(..) {
            out.write_all(&e.bytes())?;
        }
        self.runs += 1;
        Ok(())
    }
    pub fn finish(mut self, pack: Id, output_dir: &Path) -> Result<NamedTempFile> {
        self.flush()?;
        let mut round = 0;
        let mut count = self.runs;
        while count > 1 {
            let jobs = (0..count)
                .step_by(2)
                .enumerate()
                .map(|(output, run)| {
                    (
                        self.path(round, run),
                        (run + 1 < count).then(|| self.path(round, run + 1)),
                        self.path(round + 1, output as u64),
                    )
                })
                .collect::<Vec<_>>();
            jobs.par_iter()
                .try_for_each(|(a, b, target)| -> Result<()> {
                    let Some(b) = b else {
                        fs::rename(a, target)?;
                        return Ok(());
                    };
                    merge_raw(a, b, target)?;
                    fs::remove_file(a)?;
                    fs::remove_file(b)?;
                    Ok(())
                })?;
            count = jobs.len() as u64;
            round += 1;
        }
        let mut out = IndexWriter::new(output_dir)?;
        if count == 1 {
            let mut input = BufReader::new(File::open(self.path(round, 0))?);
            while let Some(mut e) = next_raw(&mut input)? {
                e.pack = pack;
                out.push(e)?;
            }
        }
        out.finish()
    }
}
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
