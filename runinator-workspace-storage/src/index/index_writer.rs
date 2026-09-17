#[allow(unused_imports)]
use super::*;

pub struct IndexWriter {
    pub(super) file: BufWriter<NamedTempFile>,
    pub(super) directory: PathBuf,
    pub(super) packs: BTreeSet<Id>,
    pub(super) count: u64,
    pub(super) last: Option<Id>,
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
