//! Single-spool immutable objects over an arbitrary base store.

use crate::{
    Error, Id, Result,
    cache::ByteCache,
    error::{corrupt, invalid},
    model::Kind,
    record,
    store::{Object, ObjectInfo, ReadStore, WriteStore},
};
use std::{
    collections::HashMap,
    fs::File,
    io::{Seek, Write},
    path::Path,
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

    fn contains(&self, _: Id) -> Result<bool> {
        Ok(false)
    }
}

pub struct Staging<S> {
    pub base: S,
    directory: tempfile::TempDir,
    state: Mutex<State>,
    cache: ByteCache,
    deduplicate_base: bool,
}

#[derive(Clone, Copy)]
struct StagedObject {
    offset: u64,
    info: ObjectInfo,
}

struct State {
    file: File,
    objects: HashMap<Id, StagedObject>,
}

impl<S: ReadStore> Staging<S> {
    pub fn new(base: S, scratch: &Path) -> Result<Self> {
        Self::create(base, scratch, false)
    }

    /// Create a staging store that omits objects already present in a complete local base.
    /// Callers must not enable this for a demand-loaded remote base: every `put` would otherwise
    /// become a remote existence probe.
    pub fn new_deduplicating(base: S, scratch: &Path) -> Result<Self> {
        Self::create(base, scratch, true)
    }

    fn create(base: S, scratch: &Path, deduplicate_base: bool) -> Result<Self> {
        let directory = tempfile::tempdir_in(scratch)?;
        let file = tempfile::tempfile_in(directory.path())?;
        Ok(Self {
            base,
            directory,
            state: Mutex::new(State {
                file,
                objects: HashMap::new(),
            }),
            cache: ByteCache::new(32 * 1024 * 1024),
            deduplicate_base,
        })
    }

    pub fn scratch(&self) -> &Path {
        self.directory.path()
    }

    pub fn is_staged(&self, id: Id) -> Result<bool> {
        Ok(self
            .state
            .lock()
            .map_err(|_| Error::Poisoned)?
            .objects
            .contains_key(&id))
    }

    fn raw_get(&self, id: Id) -> Result<Object> {
        let state = self.state.lock().map_err(|_| Error::Poisoned)?;
        if let Some(staged) = state.objects.get(&id).copied() {
            let file = state.file.try_clone()?;
            drop(state);
            let (header, object) = record::read(&file, staged.offset, Some(id))?;
            if header.info().kind != staged.info.kind
                || header.info().raw_len != staged.info.raw_len
            {
                return Err(corrupt("staged record metadata mismatch"));
            }
            return Ok(object);
        }
        drop(state);
        self.base.get(id)
    }
}

impl<S: ReadStore> ReadStore for Staging<S> {
    fn info(&self, id: Id) -> Result<ObjectInfo> {
        let state = self.state.lock().map_err(|_| Error::Poisoned)?;
        if let Some(staged) = state.objects.get(&id) {
            return Ok(staged.info);
        }
        drop(state);
        self.base.info(id)
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
            || matches!(
                kind,
                Kind::TinyBlock | Kind::ChunkBlock | Kind::MetadataBlock
            )
        {
            return Err(invalid("invalid logical object"));
        }
        let id = Id::object(kind, raw);
        {
            let state = self.state.lock().map_err(|_| Error::Poisoned)?;
            if state.objects.contains_key(&id) {
                return Ok(id);
            }
        }
        if self.deduplicate_base && self.base.contains(id)? {
            return Ok(id);
        }
        // compression and checksumming happen outside the append lock so independent puts scale.
        let mut encoded = Vec::new();
        let (_, length) = record::write(&mut encoded, kind, raw)?;
        let mut state = self.state.lock().map_err(|_| Error::Poisoned)?;
        if state.objects.contains_key(&id) {
            return Ok(id);
        }
        let offset = state.file.seek(std::io::SeekFrom::End(0))?;
        state.file.write_all(&encoded)?;
        state.objects.insert(
            id,
            StagedObject {
                offset,
                info: ObjectInfo {
                    kind,
                    raw_len: raw.len(),
                },
            },
        );
        debug_assert_eq!(state.file.stream_position()?, offset + length);
        Ok(id)
    }
}

#[cfg(test)]
#[path = "staging_tests.rs"]
mod tests;
