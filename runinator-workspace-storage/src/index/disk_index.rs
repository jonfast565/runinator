#[allow(unused_imports)]
use super::*;

#[derive(Clone, Default)]
pub struct DiskIndex {
    pub(super) file: Option<Arc<File>>,
    pub(super) packs: Arc<Vec<Id>>,
    pub(super) entries_offset: u64,
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

    pub(super) fn entries(&self) -> Result<DiskEntries> {
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
