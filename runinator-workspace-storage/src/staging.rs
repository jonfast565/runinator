//! Disk-spooled immutable objects over an arbitrary base store.

use crate::{
    Error, Id, Result,
    cache::ByteCache,
    error::{corrupt, invalid},
    model::Kind,
    record,
    store::{Object, ObjectInfo, ReadStore, WriteStore},
};
use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
};

pub struct EmptyStore;
impl ReadStore for EmptyStore {
    fn info(&self, id: Id) -> Result<ObjectInfo> {
        Err(Error::NotFound(id.to_string()))
    }
    fn get(&self, id: Id) -> Result<Object> {
        Err(Error::NotFound(id.to_string()))
    }
}

pub struct Staging<S> {
    pub base: S,
    directory: tempfile::TempDir,
    lock: Mutex<()>,
    cache: ByteCache,
}

impl<S: ReadStore> Staging<S> {
    pub fn new(base: S, scratch: &Path) -> Result<Self> {
        Ok(Self {
            base,
            directory: tempfile::tempdir_in(scratch)?,
            lock: Mutex::new(()),
            cache: ByteCache::new(32 * 1024 * 1024),
        })
    }
    fn path(&self, id: Id) -> PathBuf {
        let key = id.to_string();
        self.directory.path().join(&key[..2]).join(&key[2..])
    }
    pub fn scratch(&self) -> &Path {
        self.directory.path()
    }
    pub fn is_staged(&self, id: Id) -> Result<bool> {
        Ok(self.path(id).try_exists()?)
    }
    fn raw_get(&self, id: Id) -> Result<Object> {
        match File::open(self.path(id)) {
            Ok(file) => {
                let (header, object) = record::read(&file, 0, Some(id))?;
                if header.record_len()? != file.metadata()?.len() {
                    return Err(corrupt("staged record length mismatch"));
                }
                Ok(object)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => self.base.get(id),
            Err(error) => Err(error.into()),
        }
    }
}

impl<S: ReadStore> ReadStore for Staging<S> {
    fn info(&self, id: Id) -> Result<ObjectInfo> {
        match File::open(self.path(id)) {
            Ok(file) => {
                let header = record::header(&file, 0)?;
                if header.id != id || header.record_len()? != file.metadata()?.len() {
                    return Err(corrupt("staged record mismatch"));
                }
                Ok(header.info())
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => self.base.info(id),
            Err(error) => Err(error.into()),
        }
    }
    fn get(&self, id: Id) -> Result<Object> {
        let info = self.info(id)?;
        let bytes = self
            .cache
            .get_or_load(id, info.raw_len, || Ok((*self.raw_get(id)?.bytes).clone()))?;
        Ok(Object {
            kind: info.kind,
            bytes,
        })
    }
}

impl<S: ReadStore> WriteStore for Staging<S> {
    fn put(&self, kind: Kind, raw: &[u8]) -> Result<Id> {
        if raw.len() > crate::codec::MAX_OBJECT
            || matches!(kind, Kind::TinyBlock | Kind::ChunkBlock)
        {
            return Err(invalid("invalid logical object"));
        }
        let id = Id::object(kind, raw);
        let _guard = self.lock.lock().map_err(|_| Error::Poisoned)?;
        // stage locally without probing a potentially remote base for each new object.
        if self.is_staged(id)? {
            self.info(id)?;
            return Ok(id);
        }
        let path = self.path(id);
        let parent = path
            .parent()
            .ok_or_else(|| invalid("staging path has no parent"))?;
        fs::create_dir_all(parent)?;
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        record::write(&mut file, kind, raw)?;
        file.flush()?;
        file.persist_noclobber(path).map_err(|e| e.error)?;
        Ok(id)
    }
}
