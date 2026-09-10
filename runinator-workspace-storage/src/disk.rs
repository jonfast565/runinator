use crate::{
    Error, Id,
    cache::{ByteCache, ObjectCaches},
    error::{Result, corrupt, invalid},
    index::{DiskIndex, ExternalSorter, Location, STANDALONE},
    io_util,
    model::Kind,
    record,
    store::{Object, ObjectInfo, ReadStore, WriteStore},
    tiny,
};
use rayon::prelude::*;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs::{self, File},
    io::{Seek, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tempfile::{NamedTempFile, TempDir};
#[derive(Clone)]
pub struct RawDisk {
    pub root: PathBuf,
    pub index: DiskIndex,
    pub tiny_blocks: Arc<ByteCache>,
}
impl RawDisk {
    fn open_pack(&self, pack: Id) -> Result<(File, u64)> {
        let file = File::open(self.root.join("packs").join(format!("{pack}.pack")))?;
        let mut magic = [0; 8];
        io_util::read_at(&file, &mut magic, 0)?;
        if &magic != record::PACK_MAGIC {
            return Err(corrupt("invalid pack magic"));
        }
        let length = file.metadata()?.len();
        Ok((file, length))
    }

    fn validate_location(loc: &Location, pack_len: u64) -> Result<()> {
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

    fn locate(&self, id: Id) -> Result<(File, Location)> {
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
    fn tiny_block(&self, file: &File, loc: &Location) -> Result<Arc<Vec<u8>>> {
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
    fn chunk_block(&self, file: &File, loc: &Location) -> Result<Arc<Vec<u8>>> {
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
                _ => Err(corrupt("indexed member points at non-container record")),
            };
        }
        let h = record::header(&file, loc.offset)?;
        if h.id != id
            || h.record_len()? != loc.length
            || matches!(h.kind, Kind::TinyBlock | Kind::ChunkBlock)
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
                _ => Err(corrupt("indexed member points at non-container record")),
            };
        }
        let (h, object) = record::read(&file, loc.offset, Some(id))?;
        if h.record_len()? != loc.length
            || matches!(object.kind, Kind::TinyBlock | Kind::ChunkBlock)
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
#[derive(Clone)]
pub struct DiskView {
    pub(crate) raw: RawDisk,
    pub(crate) caches: Arc<ObjectCaches>,
}
impl ReadStore for DiskView {
    fn info(&self, id: Id) -> Result<ObjectInfo> {
        self.raw.info(id)
    }
    fn get(&self, id: Id) -> Result<Object> {
        self.caches.get(&self.raw, id)
    }
    fn contains(&self, id: Id) -> Result<bool> {
        self.raw.contains(id)
    }
}
/// Unpublished objects spill immediately to transaction-local disk files, not
/// an unbounded HashMap. A failed operation can leave harmless staged garbage.
pub struct OverlayStore {
    pub(crate) base: RawDisk,
    dir: TempDir,
    caches: Arc<ObjectCaches>,
    write_lock: Mutex<()>,
}
impl OverlayStore {
    pub fn new(base: RawDisk, caches: Arc<ObjectCaches>) -> Result<Self> {
        let dir = tempfile::Builder::new()
            .prefix("txn-")
            .tempdir_in(base.root.join("tmp"))?;
        Ok(Self {
            base,
            dir,
            caches,
            write_lock: Mutex::new(()),
        })
    }
    fn path(&self, id: Id) -> PathBuf {
        let hex = id.to_string();
        self.dir
            .path()
            .join(&hex[..2])
            .join(format!("{}.rec", &hex[2..]))
    }
    fn raw_info(&self, id: Id) -> Result<ObjectInfo> {
        let path = self.path(id);
        match File::open(&path) {
            Ok(f) => {
                let h = record::header(&f, 0)?;
                if h.id != id || h.record_len()? != f.metadata()?.len() {
                    return Err(corrupt("invalid staged record"));
                }
                Ok(h.info())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => self.base.info(id),
            Err(e) => Err(e.into()),
        }
    }
    fn raw_get(&self, id: Id) -> Result<Object> {
        match File::open(self.path(id)) {
            Ok(f) => {
                let (h, obj) = record::read(&f, 0, Some(id))?;
                if h.record_len()? != f.metadata()?.len() {
                    return Err(corrupt("staged record has trailing bytes"));
                }
                Ok(obj)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => self.base.get(id),
            Err(e) => Err(e.into()),
        }
    }
}
struct RawOverlay<'a>(&'a OverlayStore);
impl ReadStore for RawOverlay<'_> {
    fn info(&self, id: Id) -> Result<ObjectInfo> {
        self.0.raw_info(id)
    }
    fn get(&self, id: Id) -> Result<Object> {
        self.0.raw_get(id)
    }
}
impl ReadStore for OverlayStore {
    fn info(&self, id: Id) -> Result<ObjectInfo> {
        self.raw_info(id)
    }
    fn get(&self, id: Id) -> Result<Object> {
        self.caches.get(&RawOverlay(self), id)
    }
    fn contains(&self, id: Id) -> Result<bool> {
        Ok(self.path(id).try_exists()? || self.base.contains(id)?)
    }
}
impl WriteStore for OverlayStore {
    fn put(&self, kind: Kind, raw: &[u8]) -> Result<Id> {
        if raw.len() > crate::codec::MAX_OBJECT {
            return Err(invalid("object too large"));
        }
        let id = Id::object(kind, raw);
        if self.contains(id)? {
            return Ok(id);
        }
        let mut encoded = Vec::new();
        record::write(&mut encoded, kind, raw)?;
        let _lock = self.write_lock.lock().map_err(|_| Error::Poisoned)?;
        if self.path(id).try_exists()? {
            return Ok(id);
        }
        let path = self.path(id);
        let parent = path.parent().unwrap();
        fs::create_dir_all(parent)?;
        let mut tmp = NamedTempFile::new_in(parent)?;
        tmp.write_all(&encoded)?;
        tmp.flush()?;
        tmp.persist_noclobber(path).map_err(|e| e.error)?;
        Ok(id)
    }
}
pub(crate) struct PackBuilder {
    file: NamedTempFile,
    sort: ExternalSorter,
    objects: u64,
    tiny: tiny::Pending,
    chunks: crate::chunkblock::Pending,
}
pub(crate) struct SealedPack {
    pub pack: Option<Id>,
    pub index: NamedTempFile,
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
            chunks: crate::chunkblock::Pending::default(),
        })
    }
    pub fn add(&mut self, kind: Kind, raw: &[u8]) -> Result<Id> {
        if matches!(kind, Kind::TinyBlock | Kind::ChunkBlock) {
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
    fn flush_tiny(&mut self) -> Result<()> {
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
    fn flush_chunks(&mut self) -> Result<()> {
        let Some((raw, ids)) = self.chunks.take()? else {
            return Ok(());
        };
        self.write_chunk_block(&raw, ids)
    }
    fn write_chunk_block(&mut self, raw: &[u8], ids: Vec<Id>) -> Result<()> {
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
pub(crate) fn seal(overlay: &OverlayStore, latest: &RawDisk, revision: Id) -> Result<SealedPack> {
    let mut builder = PackBuilder::new(&latest.root)?;
    let marks = crate::gc::mark_roots(overlay, &[revision], &latest.root.join("tmp"), true)?;
    marks.visit(|id| {
        if !latest.contains(id)? {
            let object = overlay.get(id)?;
            builder.add(object.kind, &object.bytes)?;
        }
        Ok(())
    })?;
    builder.finish(&latest.root)
}
