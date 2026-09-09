//! A global immutable sorted disk index. Binary search needs O(1) user-space
//! memory and never scans historical packs. External pairwise merge-sort keeps
//! index construction bounded, at the cost of rewriting the index on commit.
use crate::{
    Id,
    error::{Result, corrupt},
    io_util::read_at,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{Read, Seek, SeekFrom, Write},
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
        let mut packs = Vec::with_capacity(pack_count);
        for slot in 0..pack_count {
            let mut bytes = [0; 32];
            read_at(&file, &mut bytes, HEADER_BYTES + slot as u64 * 32)?;
            let id = Id(bytes);
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
        for i in 0..self.count {
            let e = self.entry(i)?;
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
}

/// Spools entries on disk while collecting only the bounded pack dictionary.
pub struct IndexWriter {
    file: NamedTempFile,
    directory: PathBuf,
    packs: BTreeSet<Id>,
    count: u64,
    last: Option<Id>,
}
impl IndexWriter {
    pub fn new(dir: &Path) -> Result<Self> {
        Ok(Self {
            file: NamedTempFile::new_in(dir)?,
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
        out.write_all(MAGIC)?;
        out.write_all(&self.count.to_le_bytes())?;
        out.write_all(&(self.packs.len() as u32).to_le_bytes())?;
        out.write_all(&(ENTRY_BYTES as u32).to_le_bytes())?;
        let mut dictionary = BTreeMap::new();
        for (slot, pack) in self.packs.iter().enumerate() {
            dictionary.insert(*pack, slot as u32);
            out.write_all(&pack.0)?;
        }
        self.file.flush()?;
        self.file.seek(SeekFrom::Start(0))?;
        while let Some(entry) = next_raw(self.file.as_file_mut())? {
            out.write_all(&entry.id.0)?;
            let slot = dictionary
                .get(&entry.pack)
                .ok_or_else(|| corrupt("missing pack table entry"))?;
            out.write_all(&slot.to_le_bytes())?;
            out.write_all(&entry.offset.to_le_bytes())?;
            out.write_all(&(entry.length as u32).to_le_bytes())?;
            out.write_all(&entry.member.to_le_bytes())?;
        }
        out.flush()?;
        Ok(out)
    }
}
/// Both inputs are sorted; equal IDs reuse the old location.
pub fn merge(old: &DiskIndex, new: &DiskIndex, dir: &Path) -> Result<NamedTempFile> {
    let mut out = IndexWriter::new(dir)?;
    let (mut a, mut b) = (0, 0);
    while a < old.count || b < new.count {
        let x = if a < old.count {
            Some(old.entry(a)?)
        } else {
            None
        };
        let y = if b < new.count {
            Some(new.entry(b)?)
        } else {
            None
        };
        match (x, y) {
            (Some(x), Some(y)) if x.id == y.id => {
                out.push(x)?;
                a += 1;
                b += 1;
            }
            (Some(x), Some(y)) if x.id < y.id => {
                out.push(x)?;
                a += 1;
            }
            (Some(_), Some(y)) => {
                out.push(y)?;
                b += 1;
            }
            (Some(x), None) => {
                out.push(x)?;
                a += 1;
            }
            (None, Some(y)) => {
                out.push(y)?;
                b += 1;
            }
            (None, None) => break,
        }
    }
    out.finish()
}
/// Spill 4096 entries at a time. Pairwise merging has constant merge memory and
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
            buffer: Vec::with_capacity(4096),
            runs: 0,
        })
    }
    fn path(&self, round: u64, run: u64) -> PathBuf {
        self.dir.path().join(format!("{round}-{run}"))
    }
    pub fn push(&mut self, e: Location) -> Result<()> {
        self.buffer.push(e);
        if self.buffer.len() == 4096 {
            self.flush()?;
        }
        Ok(())
    }
    fn flush(&mut self) -> Result<()> {
        if self.buffer.is_empty() {
            return Ok(());
        }
        self.buffer.sort_unstable_by_key(|e| e.id);
        let mut out = File::create(self.path(0, self.runs))?;
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
            let mut output = 0;
            let mut run = 0;
            while run < count {
                let a = self.path(round, run);
                let target = self.path(round + 1, output);
                if run + 1 == count {
                    fs::rename(a, target)?;
                } else {
                    let b = self.path(round, run + 1);
                    merge_raw(&a, &b, &target)?;
                    fs::remove_file(a)?;
                    fs::remove_file(b)?;
                }
                output += 1;
                run += 2;
            }
            count = output;
            round += 1;
        }
        let mut out = IndexWriter::new(output_dir)?;
        if count == 1 {
            let mut input = File::open(self.path(round, 0))?;
            while let Some(mut e) = next_raw(&mut input)? {
                e.pack = pack;
                out.push(e)?;
            }
        }
        out.finish()
    }
}
fn next_raw(file: &mut File) -> Result<Option<Location>> {
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
    let mut a = File::open(a)?;
    let mut b = File::open(b)?;
    let mut out = File::create(out)?;
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
