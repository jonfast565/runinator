#[allow(unused_imports)]
use super::*;

pub struct WorkerObjects {
    pub(super) api: Arc<dyn WorkspaceObjectTransport>,
    pub(super) checkout: uuid::Uuid,
    pub(super) replica: uuid::Uuid,
    pub(super) runtime: tokio::runtime::Handle,
    pub(super) cache: tempfile::TempDir,
    pub(super) local: Option<LocalObjects>,
    pub(super) deadline: Instant,
}

impl WorkerObjects {
    pub(crate) fn has_complete_local_base(&self) -> bool {
        self.local.is_some()
    }

    pub(crate) fn cached(&self) -> CachedObjects<'_> {
        CachedObjects {
            path: self.cache.path(),
            local: self.local.as_ref(),
        }
    }

    pub fn new(
        api: impl WorkspaceObjectTransport + 'static,
        checkout: uuid::Uuid,
        replica: uuid::Uuid,
        deadline: Instant,
        local: Option<LocalObjects>,
    ) -> Result<Self, runinator_models::errors::SendableError> {
        Ok(Self {
            api: Arc::new(api),
            checkout,
            replica,
            runtime: tokio::runtime::Handle::current(),
            cache: tempfile::tempdir()?,
            local,
            deadline,
        })
    }
    pub(crate) fn remaining(&self) -> storage::Result<std::time::Duration> {
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

impl ReadStore for WorkerObjects {
    fn info(&self, id: Id) -> storage::Result<ObjectInfo> {
        if let Some(local) = &self.local {
            // a downloaded archive is a complete closure, so a miss is authoritative.
            return local.info(id);
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
            // a packed local base is complete, so its negative lookups are authoritative too.
            return local.get(id);
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
