//! Shared blob reads adapted to the synchronous storage algorithms on blocking workers.
use runinator_models::{errors::SendableError, workspaces::*};
use runinator_store::roles::DurableWorkspaceStore;
use runinator_workspace::storage::{
    self, Id,
    store::{Object, ObjectInfo, ReadStore},
};
use std::sync::Arc;

async fn read_pack_range_from(
    blobs: Arc<dyn runinator_blob_core::BlobStore>,
    workspace: uuid::Uuid,
    pack: &str,
    offset: u64,
    length: u64,
) -> Result<Vec<u8>, SendableError> {
    use tokio::io::AsyncReadExt;
    let uri = crate::artifact_storage::workspace_pack_uri(workspace, pack)?;
    let end = offset
        .checked_add(length)
        .and_then(|n| n.checked_sub(1))
        .ok_or_else(|| storage::Error::Corrupt("invalid object range".into()))?;
    let content = crate::artifact_storage::open_artifact(
        &blobs,
        &uri,
        Some(runinator_blob_core::ByteRange::From {
            start: offset,
            end: Some(end),
        }),
    )
    .await?;
    let mut bytes = Vec::new();
    content
        .body
        .take(length + 1)
        .read_to_end(&mut bytes)
        .await?;
    if bytes.len() as u64 != length {
        return Err(storage::Error::Corrupt("workspace object range is truncated".into()).into());
    }
    Ok(bytes)
}

pub struct SharedObjects<T: DurableWorkspaceStore> {
    pub db: Arc<T>,
    pub blobs: Arc<dyn runinator_blob_core::BlobStore>,
    pub workspace: uuid::Uuid,
    pub runtime: tokio::runtime::Handle,
    pub reader: Option<LazyReaderGuard<T>>,
    pub records: Arc<storage::cache::ByteCache>,
    pub decoded_records: Arc<storage::cache::ByteCache>,
    pub indexed_records: std::sync::Mutex<std::collections::HashSet<Id>>,
    pub database_reads: std::sync::atomic::AtomicU64,
    pub blob_reads: std::sync::atomic::AtomicU64,
    pub locations: std::sync::Mutex<std::collections::HashMap<Id, WorkspaceObjectLocation>>,
    pub metadata_reads: Arc<tokio::sync::Semaphore>,
}

impl<T: DurableWorkspaceStore> SharedObjects<T> {
    fn record_key(location: &WorkspaceObjectLocation) -> Id {
        Id::sha256(format!("{}:{}:{}", location.pack, location.offset, location.length).as_bytes())
    }

    async fn location(&self, id: Id) -> Result<WorkspaceObjectLocation, SendableError> {
        if let Some(location) = self
            .locations
            .lock()
            .map_err(|_| storage::Error::Poisoned)?
            .get(&id)
            .cloned()
        {
            return Ok(location);
        }
        self.database_reads
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let location = self
            .db
            .fetch_workspace_object(self.workspace, id.to_string())
            .await?
            .ok_or_else(|| Box::new(storage::Error::NotFound(id.to_string())) as SendableError)?;
        let mut locations = self
            .locations
            .lock()
            .map_err(|_| storage::Error::Poisoned)?;
        // bounded to one small directory page's metadata, including traversal nodes.
        if locations.len() >= 8192 {
            locations.clear();
            self.indexed_records
                .lock()
                .map_err(|_| storage::Error::Poisoned)?
                .clear();
        }
        locations.insert(id, location.clone());
        Ok(location)
    }
    async fn locations(&self, ids: &[Id]) -> Result<Vec<WorkspaceObjectLocation>, SendableError> {
        let (mut found, missing) = {
            let locations = self
                .locations
                .lock()
                .map_err(|_| storage::Error::Poisoned)?;
            let found = ids
                .iter()
                .filter_map(|id| locations.get(id).cloned().map(|location| (*id, location)))
                .collect::<std::collections::HashMap<_, _>>();
            let mut missing = ids
                .iter()
                .filter(|id| !found.contains_key(id))
                .copied()
                .collect::<Vec<_>>();
            missing.sort_unstable();
            missing.dedup();
            (found, missing)
        };
        for batch in missing.chunks(500) {
            self.database_reads
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let objects = self
                .db
                .fetch_workspace_objects(
                    self.workspace,
                    batch.iter().map(ToString::to_string).collect(),
                )
                .await?;
            let mut locations = self
                .locations
                .lock()
                .map_err(|_| storage::Error::Poisoned)?;
            if locations.len().saturating_add(objects.len()) >= 8192 {
                locations.clear();
                self.indexed_records
                    .lock()
                    .map_err(|_| storage::Error::Poisoned)?
                    .clear();
            }
            for object in objects {
                let id = object.id.parse()?;
                found.insert(id, object.clone());
                locations.insert(id, object);
            }
        }
        ids.iter()
            .map(|id| {
                found.get(id).cloned().ok_or_else(|| {
                    Box::new(storage::Error::NotFound(id.to_string())) as SendableError
                })
            })
            .collect()
    }
    fn read_location(&self, id: Id, location: WorkspaceObjectLocation) -> storage::Result<Object> {
        let member = location.member;
        self.read_location_group(&location, &[(0, id, member)])?
            .pop()
            .map(|(_, object)| object)
            .ok_or_else(|| storage::Error::Corrupt("record omitted object".into()))
    }
    fn read_location_group(
        &self,
        location: &WorkspaceObjectLocation,
        members: &[(usize, Id, u32)],
    ) -> storage::Result<Vec<(usize, Object)>> {
        let _permit = self
            .runtime
            .block_on(self.metadata_reads.acquire())
            .map_err(|_| storage::Error::Conflict)?;
        if self.reader.as_ref().is_some_and(|reader| !reader.alive()) {
            return Err(storage::Error::Conflict);
        }
        if location.length > storage::codec::MAX_OBJECT as u64 + storage::record::HEADER_LEN + 65536
        {
            return Err(storage::Error::Corrupt("oversized physical record".into()));
        }
        let key = Self::record_key(location);
        let load = || {
            self.runtime
                .block_on(self.read_record(location))
                .map_err(storage_error)
        };
        let bytes = match self
            .records
            .get_or_load(key, location.length as usize, load)
        {
            Ok(bytes) => bytes,
            Err(storage::Error::CacheFull) => Arc::new(load()?),
            Err(error) => return Err(error),
        };
        let indexed = self
            .indexed_records
            .lock()
            .map_err(|_| storage::Error::Poisoned)?
            .contains(&key);
        if !indexed
            && let Some(index) =
                storage::record::metadata_block_members_cached(&bytes, &self.decoded_records)?
        {
            let mut locations = self
                .locations
                .lock()
                .map_err(|_| storage::Error::Poisoned)?;
            if locations.len().saturating_add(index.len()) >= 8192 {
                locations.clear();
                self.indexed_records
                    .lock()
                    .map_err(|_| storage::Error::Poisoned)?
                    .clear();
            }
            for member in index {
                let mut indexed = location.clone();
                indexed.member = member.member;
                indexed.kind = member.kind as u8;
                indexed.raw_len = member.raw_len as u64;
                locations.insert(member.id, indexed);
            }
            self.indexed_records
                .lock()
                .map_err(|_| storage::Error::Poisoned)?
                .insert(key);
        }
        let requests = members
            .iter()
            .map(|(_, id, member)| (*id, *member))
            .collect::<Vec<_>>();
        let objects =
            storage::record::decode_ranges_cached(&bytes, &requests, &self.decoded_records)?;
        Ok(members
            .iter()
            .map(|(position, _, _)| *position)
            .zip(objects)
            .collect())
    }

    fn read_locations(
        &self,
        ids: &[Id],
        locations: &[WorkspaceObjectLocation],
    ) -> storage::Result<Vec<Object>> {
        let mut grouped = std::collections::BTreeMap::<
            (String, u64, u64),
            (WorkspaceObjectLocation, Vec<(usize, Id, u32)>),
        >::new();
        for (position, (id, location)) in ids.iter().zip(locations).enumerate() {
            grouped
                .entry((location.pack.clone(), location.offset, location.length))
                .or_insert_with(|| (location.clone(), Vec::new()))
                .1
                .push((position, *id, location.member));
        }
        let groups = grouped.into_values().collect::<Vec<_>>();
        let grouped_objects = std::thread::scope(|scope| {
            let chunk_size = groups.len().div_ceil(8).max(1);
            let tasks = groups
                .chunks(chunk_size)
                .map(|groups| {
                    scope.spawn(move || {
                        let mut output = Vec::new();
                        for (location, members) in groups {
                            output.extend(self.read_location_group(location, members)?);
                        }
                        Ok::<_, storage::Error>(output)
                    })
                })
                .collect::<Vec<_>>();
            let mut output = Vec::with_capacity(ids.len());
            for task in tasks {
                output.extend(task.join().map_err(|_| storage::Error::Conflict)??);
            }
            Ok::<_, storage::Error>(output)
        })?;
        let mut ordered = vec![None; ids.len()];
        for (position, object) in grouped_objects {
            ordered[position] = Some(object);
        }
        ordered
            .into_iter()
            .map(|object| {
                object.ok_or_else(|| storage::Error::Corrupt("bulk record omitted object".into()))
            })
            .collect()
    }
    fn read(&self, id: Id) -> storage::Result<Object> {
        if let Some(reader) = &self.reader {
            reader.ensure(&self.runtime)?;
        }
        let location = self
            .runtime
            .block_on(self.location(id))
            .map_err(storage_error)?;
        self.read_location(id, location)
    }
    async fn read_record(
        &self,
        location: &WorkspaceObjectLocation,
    ) -> Result<Vec<u8>, SendableError> {
        self.read_pack_range(&location.pack, location.offset, location.length)
            .await
    }
    async fn read_pack_range(
        &self,
        pack: &str,
        offset: u64,
        length: u64,
    ) -> Result<Vec<u8>, SendableError> {
        self.blob_reads
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        read_pack_range_from(self.blobs.clone(), self.workspace, pack, offset, length).await
    }

    async fn prefetch_records(
        &self,
        locations: &[WorkspaceObjectLocation],
    ) -> Result<(), SendableError> {
        const MAX_SPAN: u64 = 8 * 1024 * 1024;
        const MAX_GAP: u64 = 512 * 1024;
        const MAX_RECORD_OVERHEAD: u64 = 64 * 1024;

        let mut missing = Vec::new();
        for location in locations {
            if location.length
                > storage::codec::MAX_OBJECT as u64
                    + storage::record::HEADER_LEN
                    + MAX_RECORD_OVERHEAD
            {
                return Err(storage::Error::Corrupt("oversized physical record".into()).into());
            }
            if self
                .records
                .get_cached(Self::record_key(location))?
                .is_none()
            {
                missing.push(location);
            }
        }
        missing.sort_unstable_by(|a, b| {
            (&a.pack, a.offset, a.length).cmp(&(&b.pack, b.offset, b.length))
        });
        missing.dedup_by(|a, b| a.pack == b.pack && a.offset == b.offset && a.length == b.length);

        struct Span {
            pack: String,
            start: u64,
            end: u64,
            locations: Vec<WorkspaceObjectLocation>,
        }
        let mut spans = Vec::new();
        let mut first = 0;
        while first < missing.len() {
            let pack = missing[first].pack.as_str();
            let start = missing[first].offset;
            let mut end = start
                .checked_add(missing[first].length)
                .ok_or_else(|| storage::Error::Corrupt("workspace object range overflow".into()))?;
            let mut last = first + 1;
            while let Some(location) = missing.get(last) {
                if location.pack != pack || location.offset > end.saturating_add(MAX_GAP) {
                    break;
                }
                let next_end = location
                    .offset
                    .checked_add(location.length)
                    .ok_or_else(|| {
                        storage::Error::Corrupt("workspace object range overflow".into())
                    })?;
                if next_end.saturating_sub(start) > MAX_SPAN {
                    break;
                }
                end = end.max(next_end);
                last += 1;
            }
            spans.push(Span {
                pack: pack.to_owned(),
                start,
                end,
                locations: missing[first..last]
                    .iter()
                    .map(|location| (*location).clone())
                    .collect(),
            });
            first = last;
        }
        let retain = |span: Span, bytes: Vec<u8>| -> Result<(), SendableError> {
            for location in &span.locations {
                let begin = usize::try_from(location.offset - span.start)?;
                let finish = begin
                    .checked_add(usize::try_from(location.length)?)
                    .ok_or_else(|| {
                        storage::Error::Corrupt("workspace object range overflow".into())
                    })?;
                let record = bytes.get(begin..finish).ok_or_else(|| {
                    storage::Error::Corrupt("workspace object range is truncated".into())
                })?;
                match self
                    .records
                    .get_or_load(Self::record_key(location), record.len(), || {
                        Ok(record.to_vec())
                    }) {
                    Ok(_) | Err(storage::Error::CacheFull) => {}
                    Err(error) => return Err(error.into()),
                }
            }
            Ok(())
        };
        let mut tasks = tokio::task::JoinSet::new();
        for span in spans {
            while tasks.len() >= 8 {
                let result = tasks.join_next().await.ok_or(storage::Error::Conflict)??;
                let (span, bytes) = result?;
                retain(span, bytes)?;
            }
            self.blob_reads
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let blobs = self.blobs.clone();
            let workspace = self.workspace;
            let metadata_reads = self.metadata_reads.clone();
            tasks.spawn(async move {
                let _permit = metadata_reads.acquire_owned().await?;
                let bytes = read_pack_range_from(
                    blobs,
                    workspace,
                    &span.pack,
                    span.start,
                    span.end - span.start,
                )
                .await?;
                Ok::<_, SendableError>((span, bytes))
            });
        }
        while let Some(result) = tasks.join_next().await {
            let (span, bytes) = result??;
            retain(span, bytes)?;
        }
        Ok(())
    }
}

pub struct LazyReaderGuard<T: DurableWorkspaceStore> {
    db: Arc<T>,
    workspace: uuid::Uuid,
    version: i64,
    guard: std::sync::Mutex<Option<ReaderGuard<T>>>,
}

impl<T: DurableWorkspaceStore> LazyReaderGuard<T> {
    pub fn new(db: Arc<T>, workspace: uuid::Uuid, version: i64) -> Self {
        Self {
            db,
            workspace,
            version,
            guard: std::sync::Mutex::new(None),
        }
    }

    fn ensure(&self, runtime: &tokio::runtime::Handle) -> storage::Result<()> {
        let mut guard = self.guard.lock().map_err(|_| storage::Error::Poisoned)?;
        if guard.as_ref().is_some_and(ReaderGuard::alive) {
            return Ok(());
        }
        *guard = Some(
            runtime
                .block_on(ReaderGuard::new(
                    self.db.clone(),
                    self.workspace,
                    self.version,
                ))
                .map_err(storage_error)?,
        );
        Ok(())
    }

    fn alive(&self) -> bool {
        self.guard
            .lock()
            .is_ok_and(|guard| guard.as_ref().is_some_and(ReaderGuard::alive))
    }
}

pub(super) fn storage_error(error: SendableError) -> storage::Error {
    match error.downcast::<storage::Error>() {
        Ok(error) => *error,
        Err(error) => storage::Error::Io(std::io::Error::other(error)),
    }
}

impl<T: DurableWorkspaceStore> ReadStore for SharedObjects<T> {
    fn get_many(&self, ids: &[Id]) -> storage::Result<Vec<Object>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        if let Some(reader) = &self.reader {
            reader.ensure(&self.runtime)?;
        }
        let locations = self
            .runtime
            .block_on(self.locations(ids))
            .map_err(storage_error)?;
        self.runtime
            .block_on(self.prefetch_records(&locations))
            .map_err(storage_error)?;
        self.read_locations(ids, &locations)
    }

    fn info(&self, id: Id) -> storage::Result<ObjectInfo> {
        let location = self
            .runtime
            .block_on(self.location(id))
            .map_err(storage_error)?;
        if location.raw_len > storage::codec::MAX_OBJECT as u64 {
            return Err(storage::Error::Corrupt(
                "invalid logical object length".into(),
            ));
        }
        Ok(ObjectInfo {
            kind: location.kind.try_into()?,
            raw_len: location.raw_len as usize,
        })
    }
    fn get(&self, id: Id) -> storage::Result<Object> {
        self.read(id)
    }
    fn contains(&self, id: Id) -> storage::Result<bool> {
        self.runtime
            .block_on(
                self.db
                    .fetch_workspace_object(self.workspace, id.to_string()),
            )
            .map(|object| object.is_some())
            .map_err(storage_error)
    }
}

pub struct ReaderGuard<T: DurableWorkspaceStore> {
    db: Arc<T>,
    lease: WorkspaceReaderLease,
    task: tokio::task::JoinHandle<()>,
    runtime: tokio::runtime::Handle,
    expires: Arc<std::sync::atomic::AtomicI64>,
}
impl<T: DurableWorkspaceStore> ReaderGuard<T> {
    pub async fn new(
        db: Arc<T>,
        workspace: uuid::Uuid,
        version: i64,
    ) -> Result<Self, SendableError> {
        let lease = db.pin_workspace_reader(workspace, version).await?;
        let expires = Arc::new(std::sync::atomic::AtomicI64::new(
            lease.expires_at.timestamp(),
        ));
        let renewing_db = db.clone();
        let renewing_lease = lease.clone();
        let renewing_expires = expires.clone();
        let task = tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                match renewing_db
                    .renew_workspace_reader(renewing_lease.clone())
                    .await
                {
                    Ok(true) => {
                        renewing_expires.store(
                            (chrono::Utc::now() + chrono::Duration::minutes(4)).timestamp(),
                            std::sync::atomic::Ordering::Release,
                        );
                    }
                    _ => {
                        renewing_expires.store(0, std::sync::atomic::Ordering::Release);
                        return;
                    }
                }
            }
        });
        Ok(Self {
            db,
            lease,
            task,
            runtime: tokio::runtime::Handle::current(),
            expires,
        })
    }
    fn alive(&self) -> bool {
        self.expires.load(std::sync::atomic::Ordering::Acquire) > chrono::Utc::now().timestamp()
    }
}
impl<T: DurableWorkspaceStore> Drop for ReaderGuard<T> {
    fn drop(&mut self) {
        self.task.abort();
        let db = self.db.clone();
        let id = self.lease.id;
        self.runtime.spawn(async move {
            let _ = db.release_workspace_reader(id).await;
        });
    }
}
