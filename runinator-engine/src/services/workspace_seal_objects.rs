//! Batch registered pack locations and validate from bounded disposable local pack copies.
use super::workspace_objects::SharedObjects;
use runinator_store::roles::DurableWorkspaceStore;
use runinator_workspace::storage::{
    self, Id,
    index::{DiskIndex, IndexWriter, Location},
    store::{Object, ObjectInfo, ReadStore},
};
use std::{
    collections::{HashMap, VecDeque},
    io::Read,
    sync::{Arc, Mutex},
};

const MAX_PACK_BYTES: u64 = 80 * 1024 * 1024;
const MAX_CACHED_PACKS: usize = 8;
const MAX_MEMORY_INDEX_ENTRIES: usize = 65_536;

#[derive(Clone, Copy)]
struct MemoryLocation {
    location: Location,
    info: ObjectInfo,
}

struct FoundObject {
    pack: Arc<LocalPack>,
    location: Location,
    info: Option<ObjectInfo>,
}

struct LocalPack {
    index: DiskIndex,
    memory_index: Option<HashMap<Id, MemoryLocation>>,
    _directory: tempfile::TempDir,
}

impl LocalPack {
    fn lookup(&self, id: Id) -> storage::Result<Option<(Location, Option<ObjectInfo>)>> {
        if let Some(index) = &self.memory_index {
            return Ok(index
                .get(&id)
                .map(|found| (found.location, Some(found.info))));
        }
        Ok(self.index.lookup(id)?.map(|location| (location, None)))
    }
}

pub(super) struct SealObjects<T: DurableWorkspaceStore>(
    storage::cache::BufferedStore<PackObjects<T>>,
);

impl<T: DurableWorkspaceStore> SealObjects<T> {
    pub fn new(source: SharedObjects<T>) -> Self {
        Self(storage::cache::BufferedStore::new(
            PackObjects {
                source,
                packs: Mutex::new(VecDeque::new()),
                records: storage::cache::ByteCache::new(64 * 1024 * 1024),
                #[cfg(test)]
                database_reads: std::sync::atomic::AtomicU64::new(0),
                #[cfg(test)]
                blob_reads: std::sync::atomic::AtomicU64::new(0),
            },
            48 * 1024 * 1024,
        ))
    }
    #[cfg(test)]
    pub fn io_counts(&self) -> (u64, u64) {
        use std::sync::atomic::Ordering;
        (
            self.0.inner.database_reads.load(Ordering::Relaxed),
            self.0.inner.blob_reads.load(Ordering::Relaxed),
        )
    }
}

impl<T: DurableWorkspaceStore> ReadStore for SealObjects<T> {
    fn info(&self, id: Id) -> storage::Result<ObjectInfo> {
        self.0.info(id)
    }
    fn get(&self, id: Id) -> storage::Result<Object> {
        self.0.get(id)
    }
}

struct PackObjects<T: DurableWorkspaceStore> {
    source: SharedObjects<T>,
    packs: Mutex<VecDeque<Arc<LocalPack>>>,
    records: storage::cache::ByteCache,
    #[cfg(test)]
    database_reads: std::sync::atomic::AtomicU64,
    #[cfg(test)]
    blob_reads: std::sync::atomic::AtomicU64,
}

impl<T: DurableWorkspaceStore> PackObjects<T> {
    fn find(&self, id: Id) -> storage::Result<Option<FoundObject>> {
        let mut packs = self.packs.lock().map_err(|_| storage::Error::Poisoned)?;
        for i in 0..packs.len() {
            let Some((location, info)) = packs[i].lookup(id)? else {
                continue;
            };

            let pack = packs.remove(i).ok_or(storage::Error::Poisoned)?;
            packs.push_back(pack.clone());
            return Ok(Some(FoundObject {
                pack,
                location,
                info,
            }));
        }
        Ok(None)
    }

    async fn download(
        &self,
        name: &str,
        directory: &std::path::Path,
    ) -> Result<std::fs::File, runinator_models::errors::SendableError> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        #[cfg(test)]
        self.blob_reads
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let uri = crate::artifact_storage::workspace_pack_uri(self.source.workspace, name)?;
        let content =
            crate::artifact_storage::open_artifact(&self.source.blobs, &uri, None).await?;
        let expected_digest = name
            .split_once('/')
            .ok_or_else(|| storage::Error::Corrupt("invalid registered pack key".into()))?
            .1;
        if content.sha256.as_deref() != Some(expected_digest) {
            return Err(storage::Error::Corrupt(
                "workspace pack checksum differs from its key".into(),
            )
            .into());
        }
        if content.size_bytes > MAX_PACK_BYTES {
            return Err(storage::Error::Corrupt("oversized workspace pack".into()).into());
        }
        let mut input = content.body.take(MAX_PACK_BYTES + 1);
        let mut file = tokio::fs::File::create(directory.join("pack")).await?;
        let copied = tokio::io::copy(&mut input, &mut file).await?;
        if copied != content.size_bytes || copied > MAX_PACK_BYTES {
            return Err(storage::Error::Corrupt("workspace pack length mismatch".into()).into());
        }
        file.flush().await?;
        Ok(std::fs::File::open(directory.join("pack"))?)
    }

    fn object_info(
        location: &runinator_models::workspaces::WorkspaceObjectLocation,
    ) -> storage::Result<ObjectInfo> {
        let kind = storage::model::Kind::try_from(location.kind)?;
        if matches!(
            kind,
            storage::model::Kind::TinyBlock | storage::model::Kind::ChunkBlock
        ) {
            return Err(storage::Error::Corrupt(
                "physical container registered as logical object".into(),
            ));
        }
        let raw_len = usize::try_from(location.raw_len)
            .map_err(|_| storage::Error::Corrupt("registered object is too large".into()))?;
        if raw_len > storage::codec::MAX_OBJECT {
            return Err(storage::Error::Corrupt(
                "registered object is too large".into(),
            ));
        }
        Ok(ObjectInfo { kind, raw_len })
    }

    fn load(&self, id: Id) -> storage::Result<FoundObject> {
        #[cfg(test)]
        self.database_reads
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let location = self
            .source
            .runtime
            .block_on(
                self.source
                    .db
                    .fetch_workspace_object(self.source.workspace, id.to_string()),
            )
            .map_err(super::workspace_objects::storage_error)?
            .ok_or_else(|| storage::Error::NotFound(id.to_string()))?;
        let requested_info = Self::object_info(&location)?;
        let directory = tempfile::tempdir()?;
        let mut file = self
            .source
            .runtime
            .block_on(self.download(&location.pack, directory.path()))
            .map_err(super::workspace_objects::storage_error)?;
        let mut magic = [0; 8];
        file.read_exact(&mut magic)?;
        if &magic != storage::record::PACK_MAGIC {
            return Err(storage::Error::Corrupt(
                "invalid workspace pack magic".into(),
            ));
        }
        let size = file.metadata()?.len();
        let pack_id = Id::sha256(location.pack.as_bytes());
        let mut writer = IndexWriter::new(directory.path())?;
        let mut memory_index = Some(HashMap::new());
        let mut after = None;
        loop {
            #[cfg(test)]
            self.database_reads
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let batch = self
                .source
                .runtime
                .block_on(self.source.db.workspace_pack_objects(
                    self.source.workspace,
                    location.pack.clone(),
                    after.clone(),
                ))
                .map_err(super::workspace_objects::storage_error)?;
            if batch.is_empty() {
                break;
            }
            for object in batch {
                let object_id: Id = object.id.parse()?;
                let info = Self::object_info(&object)?;
                if object.pack != location.pack
                    || object.offset < 8
                    || object.length
                        > storage::codec::MAX_OBJECT as u64 + storage::record::HEADER_LEN + 65536
                    || object
                        .offset
                        .checked_add(object.length)
                        .is_none_or(|end| end > size)
                {
                    return Err(storage::Error::Corrupt(
                        "invalid registered pack range".into(),
                    ));
                }
                let indexed = Location {
                    id: object_id,
                    pack: pack_id,
                    offset: object.offset,
                    length: object.length,
                    member: object.member,
                };
                writer.push(indexed)?;
                if let Some(index) = &mut memory_index {
                    if index.len() == MAX_MEMORY_INDEX_ENTRIES {
                        memory_index = None;
                    } else {
                        index.insert(
                            object_id,
                            MemoryLocation {
                                location: indexed,
                                info,
                            },
                        );
                    }
                }
                after = Some(object.id);
            }
        }
        let index_file = writer.finish()?;
        let index = DiskIndex::open(index_file.path())?;
        let found = index
            .lookup(id)?
            .ok_or_else(|| storage::Error::NotFound(id.to_string()))?;
        let pack = Arc::new(LocalPack {
            index,
            memory_index,
            _directory: directory,
        });
        let mut packs = self.packs.lock().map_err(|_| storage::Error::Poisoned)?;
        while packs.len() >= MAX_CACHED_PACKS {
            packs.pop_front();
        }
        packs.push_back(pack.clone());
        Ok(FoundObject {
            pack,
            location: found,
            info: Some(requested_info),
        })
    }
}

impl<T: DurableWorkspaceStore> ReadStore for PackObjects<T> {
    fn info(&self, id: Id) -> storage::Result<ObjectInfo> {
        let found = match self.find(id)? {
            Some(found) => found,
            None => self.load(id)?,
        };
        match found.info {
            Some(info) => Ok(info),
            None => {
                let object = self.get(id)?;
                Ok(ObjectInfo {
                    kind: object.kind,
                    raw_len: object.bytes.len(),
                })
            }
        }
    }

    fn get(&self, id: Id) -> storage::Result<Object> {
        let found = match self.find(id)? {
            Some(found) => found,
            None => self.load(id)?,
        };
        let file = std::fs::File::open(found.pack._directory.path().join("pack"))?;
        storage::record::read_indexed(&file, found.location, &self.records)
    }
}
