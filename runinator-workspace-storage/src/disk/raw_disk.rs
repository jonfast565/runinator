#[allow(unused_imports)]
use super::*;

#[derive(Clone)]
pub struct RawDisk {
    pub root: PathBuf,
    pub index: DiskIndex,
    pub tiny_blocks: Arc<ByteCache>,
}

impl RawDisk {
    pub(super) fn open_pack(&self, pack: Id) -> Result<(File, u64)> {
        let file = File::open(self.root.join("packs").join(format!("{pack}.pack")))?;
        let mut magic = [0; 8];
        io_util::read_at(&file, &mut magic, 0)?;
        if &magic != record::PACK_MAGIC {
            return Err(corrupt("invalid pack magic"));
        }
        let length = file.metadata()?.len();
        Ok((file, length))
    }

    pub(super) fn validate_location(loc: &Location, pack_len: u64) -> Result<()> {
        if loc.offset < 8 || loc.length < record::HEADER_LEN {
            return Err(corrupt("invalid pack location"));
        }
        let end = loc
            .offset
            .checked_add(loc.length)
            .ok_or_else(|| corrupt("pack location overflow"))?;
        if end > pack_len {
            return Err(corrupt("index points outside pack"));
        }
        Ok(())
    }

    pub(super) fn locate(&self, id: Id) -> Result<(File, Location)> {
        let loc = self
            .index
            .lookup(id)?
            .ok_or_else(|| Error::NotFound(id.to_string()))?;
        let (file, pack_len) = self.open_pack(loc.pack)?;
        Self::validate_location(&loc, pack_len)?;
        Ok((file, loc))
    }
}

impl RawDisk {
    pub(super) fn tiny_block(&self, file: &File, loc: &Location) -> Result<Arc<Vec<u8>>> {
        let h = record::header(file, loc.offset)?;
        if h.kind != Kind::TinyBlock
            || h.record_len()? != loc.length
            || h.raw_len > tiny::BLOCK_LIMIT as u64
        {
            return Err(corrupt("invalid tiny block location"));
        }
        self.tiny_blocks.get_or_load(h.id, h.raw_len as usize, || {
            let (_, object) = record::read(file, loc.offset, Some(h.id))?;
            Ok((*object.bytes).clone())
        })
    }
    pub(super) fn chunk_block(&self, file: &File, loc: &Location) -> Result<Arc<Vec<u8>>> {
        let h = record::header(file, loc.offset)?;
        if h.kind != Kind::ChunkBlock
            || h.record_len()? != loc.length
            || h.raw_len > crate::codec::MAX_OBJECT as u64
        {
            return Err(corrupt("invalid chunk block location"));
        }
        self.tiny_blocks.get_or_load(h.id, h.raw_len as usize, || {
            let (_, object) = record::read(file, loc.offset, Some(h.id))?;
            Ok((*object.bytes).clone())
        })
    }
    pub(super) fn metadata_block(&self, file: &File, loc: &Location) -> Result<Arc<Vec<u8>>> {
        let h = record::header(file, loc.offset)?;
        if h.kind != Kind::MetadataBlock
            || h.record_len()? != loc.length
            || h.raw_len > crate::metadata_block::BLOCK_LIMIT as u64
        {
            return Err(corrupt("invalid metadata block location"));
        }
        self.tiny_blocks.get_or_load(h.id, h.raw_len as usize, || {
            let (_, object) = record::read(file, loc.offset, Some(h.id))?;
            Ok((*object.bytes).clone())
        })
    }
}

impl ReadStore for RawDisk {
    fn info(&self, id: Id) -> Result<ObjectInfo> {
        let (file, loc) = self.locate(id)?;
        if loc.member != STANDALONE {
            let h = record::header(&file, loc.offset)?;
            return match h.kind {
                Kind::TinyBlock => {
                    let raw = self.tiny_block(&file, &loc)?;
                    let member = tiny::member(&raw, loc.member, id)?;
                    Ok(ObjectInfo {
                        kind: Kind::File,
                        raw_len: member.raw.len(),
                    })
                }
                Kind::ChunkBlock => {
                    let raw = self.chunk_block(&file, &loc)?;
                    Ok(ObjectInfo {
                        kind: Kind::Chunk,
                        raw_len: crate::chunkblock::info(&raw, loc.member, id)?,
                    })
                }
                Kind::MetadataBlock => {
                    let raw = self.metadata_block(&file, &loc)?;
                    let member = crate::metadata_block::member(&raw, loc.member, id)?;
                    Ok(ObjectInfo {
                        kind: member.kind,
                        raw_len: member.raw.len(),
                    })
                }
                _ => Err(corrupt("indexed member points at non-container record")),
            };
        }
        let h = record::header(&file, loc.offset)?;
        if h.id != id
            || h.record_len()? != loc.length
            || matches!(
                h.kind,
                Kind::TinyBlock | Kind::ChunkBlock | Kind::MetadataBlock
            )
        {
            return Err(corrupt("record/index mismatch"));
        }
        Ok(h.info())
    }
    fn get(&self, id: Id) -> Result<Object> {
        let (file, loc) = self.locate(id)?;
        if loc.member != STANDALONE {
            let h = record::header(&file, loc.offset)?;
            return match h.kind {
                Kind::TinyBlock => {
                    let raw = self.tiny_block(&file, &loc)?;
                    let member = tiny::member(&raw, loc.member, id)?;
                    Ok(Object {
                        kind: Kind::File,
                        bytes: Arc::new(member.raw.to_vec()),
                    })
                }
                Kind::ChunkBlock => {
                    let raw = self.chunk_block(&file, &loc)?;
                    let member = crate::chunkblock::member(&raw, loc.member, id)?;
                    Ok(Object {
                        kind: Kind::Chunk,
                        bytes: Arc::new(member.raw),
                    })
                }
                Kind::MetadataBlock => {
                    let raw = self.metadata_block(&file, &loc)?;
                    let member = crate::metadata_block::member(&raw, loc.member, id)?;
                    Ok(Object {
                        kind: member.kind,
                        bytes: Arc::new(member.raw.to_vec()),
                    })
                }
                _ => Err(corrupt("indexed member points at non-container record")),
            };
        }
        let (h, object) = record::read(&file, loc.offset, Some(id))?;
        if h.record_len()? != loc.length
            || matches!(
                object.kind,
                Kind::TinyBlock | Kind::ChunkBlock | Kind::MetadataBlock
            )
        {
            return Err(corrupt("record/index mismatch"));
        }
        Ok(object)
    }
    fn get_many(&self, ids: &[Id]) -> Result<Vec<Object>> {
        let mut packs = BTreeMap::<Id, Vec<Location>>::new();
        let mut seen = HashSet::with_capacity(ids.len());
        for id in ids {
            if !seen.insert(*id) {
                continue;
            }
            let loc = self
                .index
                .lookup(*id)?
                .ok_or_else(|| Error::NotFound(id.to_string()))?;
            packs.entry(loc.pack).or_default().push(loc);
        }
        let groups = packs.into_iter().collect::<Vec<_>>();
        let decoded = groups
            .par_iter()
            .map(|(pack, locations)| -> Result<Vec<(Id, Object)>> {
                let (file, pack_len) = self.open_pack(*pack)?;
                locations
                    .iter()
                    .map(|loc| {
                        Self::validate_location(loc, pack_len)?;
                        Ok((
                            loc.id,
                            record::read_indexed(&file, *loc, &self.tiny_blocks)?,
                        ))
                    })
                    .collect()
            })
            .collect::<Vec<_>>()
            .into_iter()
            .collect::<Result<Vec<_>>>()?;
        let found = decoded.into_iter().flatten().collect::<HashMap<_, _>>();
        ids.iter()
            .map(|id| {
                found
                    .get(id)
                    .cloned()
                    .ok_or_else(|| corrupt("batch read omitted indexed object"))
            })
            .collect()
    }
    fn contains(&self, id: Id) -> Result<bool> {
        Ok(self.index.lookup(id)?.is_some())
    }
}
