#[allow(unused_imports)]
use super::*;

pub(crate) struct PackBuilder {
    pub(super) file: NamedTempFile,
    pub(super) sort: ExternalSorter,
    pub(super) objects: u64,
    pub(super) tiny: tiny::Pending,
    pub(super) metadata: crate::metadata_block::Pending,
    pub(super) chunks: crate::chunkblock::Pending,
}

impl PackBuilder {
    pub fn encoded_bytes(&self) -> Result<u64> {
        Ok(self.file.as_file().metadata()?.len())
    }
    pub fn new(root: &Path) -> Result<Self> {
        let mut file = NamedTempFile::new_in(root.join("tmp"))?;
        file.write_all(record::PACK_MAGIC)?;
        Ok(Self {
            file,
            sort: ExternalSorter::new(&root.join("tmp"))?,
            objects: 0,
            tiny: tiny::Pending::default(),
            metadata: crate::metadata_block::Pending::default(),
            chunks: crate::chunkblock::Pending::default(),
        })
    }
    pub fn add(&mut self, kind: Kind, raw: &[u8]) -> Result<Id> {
        if matches!(
            kind,
            Kind::TinyBlock | Kind::ChunkBlock | Kind::MetadataBlock
        ) {
            return Err(invalid(
                "cannot insert a physical container as a logical object",
            ));
        }
        let id = Id::object(kind, raw);
        if tiny::eligible(kind, raw)? {
            if self.tiny.would_overflow(raw.len()) {
                self.flush_tiny()?;
            }
            self.tiny.push(id, raw)?;
        } else if crate::metadata_block::eligible(kind, raw) {
            if self.metadata.would_overflow(raw.len()) {
                self.flush_metadata()?;
            }
            self.metadata.push(id, kind, raw)?;
        } else if kind == Kind::Chunk {
            if self.chunks.would_overflow(raw.len()) {
                self.flush_chunks()?;
            }
            self.chunks.push(id, raw)?;
        } else {
            let offset = self.file.stream_position()?;
            let (_, length) = record::write(&mut self.file, kind, raw)?;
            self.sort.push(Location {
                id,
                pack: Id::default(),
                offset,
                length,
                member: STANDALONE,
            })?;
        }
        self.objects += 1;
        Ok(id)
    }
    pub fn add_many(&mut self, objects: &[Object]) -> Result<()> {
        let mut chunk_groups = Vec::new();
        for object in objects {
            if object.kind != Kind::Chunk {
                self.add(object.kind, &object.bytes)?;
                continue;
            }
            let id = Id::object(Kind::Chunk, &object.bytes);
            if self.chunks.would_overflow(object.bytes.len())
                && let Some(group) = self.chunks.take_members()
            {
                chunk_groups.push(group);
            }
            self.chunks.push(id, &object.bytes)?;
            self.objects += 1;
        }
        let encoded = chunk_groups
            .into_par_iter()
            .map(|members| crate::chunkblock::encode(&members))
            .collect::<Vec<_>>()
            .into_iter()
            .collect::<Result<Vec<_>>>()?;
        for (raw, ids) in encoded {
            self.write_chunk_block(&raw, ids)?;
        }
        Ok(())
    }
    pub(super) fn flush_tiny(&mut self) -> Result<()> {
        let Some((raw, ids)) = self.tiny.take()? else {
            return Ok(());
        };
        let offset = self.file.stream_position()?;
        let (_, length) = record::write(&mut self.file, Kind::TinyBlock, &raw)?;
        for (slot, id) in ids.into_iter().enumerate() {
            self.sort.push(Location {
                id,
                pack: Id::default(),
                offset,
                length,
                member: slot as u32,
            })?;
        }
        Ok(())
    }
    pub(super) fn flush_metadata(&mut self) -> Result<()> {
        let Some((raw, ids)) = self.metadata.take()? else {
            return Ok(());
        };
        let offset = self.file.stream_position()?;
        let (_, length) = record::write(&mut self.file, Kind::MetadataBlock, &raw)?;
        for (slot, id) in ids.into_iter().enumerate() {
            self.sort.push(Location {
                id,
                pack: Id::default(),
                offset,
                length,
                member: slot as u32,
            })?;
        }
        Ok(())
    }
    pub(super) fn flush_chunks(&mut self) -> Result<()> {
        let Some((raw, ids)) = self.chunks.take()? else {
            return Ok(());
        };
        self.write_chunk_block(&raw, ids)
    }
    pub(super) fn write_chunk_block(&mut self, raw: &[u8], ids: Vec<Id>) -> Result<()> {
        let offset = self.file.stream_position()?;
        let (_, length) = record::write(&mut self.file, Kind::ChunkBlock, raw)?;
        for (slot, id) in ids.into_iter().enumerate() {
            self.sort.push(Location {
                id,
                pack: Id::default(),
                offset,
                length,
                member: slot as u32,
            })?;
        }
        Ok(())
    }
    pub fn finish(mut self, root: &Path) -> Result<SealedPack> {
        self.flush_tiny()?;
        self.flush_metadata()?;
        self.flush_chunks()?;
        self.file.flush()?;
        let pack = if self.objects == 0 {
            None
        } else {
            Some(io_util::install(self.file, &root.join("packs"), ".pack")?)
        };
        let index = self
            .sort
            .finish(pack.unwrap_or_default(), &root.join("tmp"))?;
        Ok(SealedPack { pack, index })
    }
}
