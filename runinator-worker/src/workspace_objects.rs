//! Checkout-scoped object transport and disposable local cache.
use runinator_api::{AsyncApiClient, StaticLocator};
use runinator_workspace::storage::{
    self, Id,
    store::{Object, ObjectInfo, ReadStore},
};
use std::{fs, io::Write, time::Instant};

pub(super) struct LocalObjects {
    store: runinator_workspace::native::PackedStore,
    _scratch: tempfile::TempDir,
}

impl LocalObjects {
    pub(super) fn new(
        store: runinator_workspace::native::PackedStore,
        scratch: tempfile::TempDir,
    ) -> Self {
        Self {
            store,
            _scratch: scratch,
        }
    }
}

impl ReadStore for LocalObjects {
    fn info(&self, id: Id) -> storage::Result<ObjectInfo> {
        self.store.info(id)
    }

    fn get(&self, id: Id) -> storage::Result<Object> {
        self.store.get(id)
    }
}

pub struct WorkerObjects {
    api: AsyncApiClient<StaticLocator>,
    checkout: uuid::Uuid,
    replica: uuid::Uuid,
    runtime: tokio::runtime::Handle,
    cache: tempfile::TempDir,
    local: Option<LocalObjects>,
    deadline: Instant,
}

impl WorkerObjects {
    pub(super) fn cached(&self) -> CachedObjects<'_> {
        CachedObjects {
            path: self.cache.path(),
            local: self.local.as_ref(),
        }
    }

    pub fn new(
        api: AsyncApiClient<StaticLocator>,
        checkout: uuid::Uuid,
        replica: uuid::Uuid,
        deadline: Instant,
        local: Option<LocalObjects>,
    ) -> Result<Self, runinator_models::errors::SendableError> {
        Ok(Self {
            api,
            checkout,
            replica,
            runtime: tokio::runtime::Handle::current(),
            cache: tempfile::tempdir()?,
            local,
            deadline,
        })
    }
    pub(super) fn remaining(&self) -> storage::Result<std::time::Duration> {
        self.deadline
            .checked_duration_since(Instant::now())
            .filter(|duration| !duration.is_zero())
            .ok_or_else(|| {
                storage::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "workspace attempt deadline exceeded",
                ))
            })
    }
    pub fn upload(&self, bytes: Vec<u8>) -> storage::Result<()> {
        self.runtime
            .block_on(tokio::time::timeout(
                self.remaining()?,
                self.api
                    .upload_workspace_pack(self.checkout, self.replica, bytes),
            ))
            .map_err(io_error)?
            .map_err(io_error)
    }
}

/// The verified local subset of the remote store, used for pack deduplication.
pub(super) struct CachedObjects<'a> {
    path: &'a std::path::Path,
    local: Option<&'a LocalObjects>,
}

impl ReadStore for CachedObjects<'_> {
    fn info(&self, id: Id) -> storage::Result<ObjectInfo> {
        let object = self.get(id)?;
        Ok(ObjectInfo {
            kind: object.kind,
            raw_len: object.bytes.len(),
        })
    }

    fn get(&self, id: Id) -> storage::Result<Object> {
        if let Some(local) = self.local {
            match local.get(id) {
                Ok(object) => return Ok(object),
                Err(storage::Error::NotFound(_)) => {}
                Err(error) => return Err(error),
            }
        }
        match fs::File::open(self.path.join(id.to_string())) {
            Ok(file) => storage::record::read(&file, 0, Some(id)).map(|(_, object)| object),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Err(storage::Error::NotFound(id.to_string()))
            }
            Err(error) => Err(error.into()),
        }
    }
}

#[cfg(test)]
#[path = "workspace_objects_tests.rs"]
mod workspace_objects_tests;

fn io_error(error: impl std::error::Error + Send + Sync + 'static) -> storage::Error {
    storage::Error::Io(std::io::Error::other(error))
}

impl ReadStore for WorkerObjects {
    fn info(&self, id: Id) -> storage::Result<ObjectInfo> {
        if let Some(local) = &self.local {
            match local.info(id) {
                Ok(info) => return Ok(info),
                Err(storage::Error::NotFound(_)) => {}
                Err(error) => return Err(error),
            }
        }
        let object = self.get(id)?;
        Ok(ObjectInfo {
            kind: object.kind,
            raw_len: object.bytes.len(),
        })
    }
    fn get(&self, id: Id) -> storage::Result<Object> {
        self.remaining()?;
        if let Some(local) = &self.local {
            match local.get(id) {
                Ok(object) => return Ok(object),
                Err(storage::Error::NotFound(_)) => {}
                Err(error) => return Err(error),
            }
        }
        let path = self.cache.path().join(id.to_string());
        match fs::File::open(&path) {
            Ok(file) => return storage::record::read(&file, 0, Some(id)).map(|(_, object)| object),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let bytes = self.runtime.block_on(async {
            let mut attempt = 0;
            loop {
                match tokio::time::timeout(
                    self.remaining()?,
                    self.api
                        .workspace_object(self.checkout, self.replica, &id.to_string()),
                )
                .await
                .map_err(io_error)?
                {
                    Ok(Some(bytes)) => break Ok(bytes),
                    Ok(None) => break Err(storage::Error::NotFound(id.to_string())),
                    Err(_) if attempt < 20 => {
                        attempt += 1;
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    }
                    Err(error) => break Err(io_error(error)),
                }
            }
        })?;
        let object = storage::record::decode_range(&bytes, id, storage::index::STANDALONE)?;
        let mut file = tempfile::NamedTempFile::new_in(self.cache.path())?;
        file.write_all(&bytes)?;
        match file.persist_noclobber(path) {
            Ok(_) => {}
            Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.error.into()),
        }
        Ok(object)
    }
}

/// Apply the action deadline even when every object access hits local staging.
pub(super) struct CheckedStore<S> {
    pub inner: S,
    pub deadline: std::sync::Arc<WorkerObjects>,
}
impl<S: ReadStore> ReadStore for CheckedStore<S> {
    fn info(&self, id: Id) -> storage::Result<ObjectInfo> {
        self.deadline.remaining()?;
        self.inner.info(id)
    }
    fn get(&self, id: Id) -> storage::Result<Object> {
        self.deadline.remaining()?;
        self.inner.get(id)
    }
    fn contains(&self, id: Id) -> storage::Result<bool> {
        self.deadline.remaining()?;
        self.inner.contains(id)
    }
}
impl<S: storage::store::WriteStore> storage::store::WriteStore for CheckedStore<S> {
    fn put(&self, kind: storage::model::Kind, bytes: &[u8]) -> storage::Result<Id> {
        self.deadline.remaining()?;
        self.inner.put(kind, bytes)
    }
}
