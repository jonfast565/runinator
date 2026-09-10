use crate::{
    Error, Id,
    codec::{Binary, MAX_OBJECT},
    error::{Result, corrupt, invalid},
    model::Kind,
};
use rayon::prelude::*;
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

const PARALLEL_BATCH_MIN: usize = 8;
#[derive(Clone, Copy, Debug)]
pub struct ObjectInfo {
    pub kind: Kind,
    pub raw_len: usize,
}
#[derive(Clone, Debug)]
pub struct Object {
    pub kind: Kind,
    pub bytes: Arc<Vec<u8>>,
}
pub trait ReadStore: Sync {
    fn info(&self, id: Id) -> Result<ObjectInfo>;
    fn get(&self, id: Id) -> Result<Object>;
    /// Read objects in input order; backends may fetch independent objects concurrently.
    fn get_many(&self, ids: &[Id]) -> Result<Vec<Object>> {
        if ids.len() < PARALLEL_BATCH_MIN {
            return ids.iter().map(|id| self.get(*id)).collect();
        }
        ids.par_iter()
            .map(|id| self.get(*id))
            .collect::<Vec<_>>()
            .into_iter()
            .collect()
    }
    fn contains(&self, id: Id) -> Result<bool> {
        match self.info(id) {
            Ok(_) => Ok(true),
            Err(Error::NotFound(_)) => Ok(false),
            Err(e) => Err(e),
        }
    }
}
pub trait WriteStore: ReadStore {
    fn put(&self, kind: Kind, raw: &[u8]) -> Result<Id>;
}
impl<S: ReadStore + ?Sized> ReadStore for &S {
    fn get_many(&self, ids: &[Id]) -> Result<Vec<Object>> {
        (**self).get_many(ids)
    }
    fn info(&self, id: Id) -> Result<ObjectInfo> {
        (**self).info(id)
    }
    fn get(&self, id: Id) -> Result<Object> {
        (**self).get(id)
    }
}
impl<S: WriteStore + ?Sized> WriteStore for &S {
    fn put(&self, kind: Kind, raw: &[u8]) -> Result<Id> {
        (**self).put(kind, raw)
    }
}
pub fn load<T: Binary, S: ReadStore + ?Sized>(s: &S, id: Id, kind: Kind) -> Result<T> {
    let object = s.get(id)?;
    if object.kind != kind {
        return Err(corrupt("object has incorrect type"));
    }
    T::decode(&object.bytes)
}
pub fn save<T: Binary, S: WriteStore + ?Sized>(s: &S, kind: Kind, value: &T) -> Result<Id> {
    s.put(kind, &value.encode()?)
}
/// Test/embedding backend. Unlike the disk backend, its memory is not bounded.
#[derive(Default)]
pub struct MemoryStore {
    objects: RwLock<HashMap<Id, Object>>,
}
impl MemoryStore {
    pub fn len(&self) -> usize {
        self.objects.read().map(|m| m.len()).unwrap_or(0)
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn ids(&self) -> Result<Vec<Id>> {
        Ok(self
            .objects
            .read()
            .map_err(|_| Error::Poisoned)?
            .keys()
            .copied()
            .collect())
    }
}
impl ReadStore for MemoryStore {
    fn info(&self, id: Id) -> Result<ObjectInfo> {
        let m = self.objects.read().map_err(|_| Error::Poisoned)?;
        let x = m.get(&id).ok_or_else(|| Error::NotFound(id.to_string()))?;
        Ok(ObjectInfo {
            kind: x.kind,
            raw_len: x.bytes.len(),
        })
    }
    fn get(&self, id: Id) -> Result<Object> {
        let m = self.objects.read().map_err(|_| Error::Poisoned)?;
        let x = m.get(&id).ok_or_else(|| Error::NotFound(id.to_string()))?;
        if Id::object(x.kind, &x.bytes) != id {
            return Err(corrupt("object hash mismatch"));
        }
        Ok(x.clone())
    }
}
impl WriteStore for MemoryStore {
    fn put(&self, kind: Kind, raw: &[u8]) -> Result<Id> {
        if raw.len() > MAX_OBJECT {
            return Err(invalid("object too large"));
        }
        let id = Id::object(kind, raw);
        let mut m = self.objects.write().map_err(|_| Error::Poisoned)?;
        m.entry(id).or_insert_with(|| Object {
            kind,
            bytes: Arc::new(raw.to_vec()),
        });
        Ok(id)
    }
}

/// Decode an ordered batch, fetching each identity at most once.
pub fn load_many<T: Binary + Send, S: ReadStore + ?Sized>(
    s: &S,
    ids: &[Id],
    kind: Kind,
) -> Result<Vec<T>> {
    let mut positions = HashMap::new();
    let mut unique = Vec::new();
    for id in ids {
        if !positions.contains_key(id) {
            positions.insert(*id, unique.len());
            unique.push(*id);
        }
    }
    let objects = s.get_many(&unique)?;
    if objects.len() != unique.len() {
        return Err(corrupt("bulk read returned incorrect object count"));
    }
    let decode = |id: &Id| {
        let object = &objects[positions[id]];
        if object.kind != kind {
            return Err(corrupt("object has incorrect type"));
        }
        T::decode(&object.bytes)
    };
    if ids.len() < PARALLEL_BATCH_MIN {
        return ids.iter().map(decode).collect();
    }
    ids.par_iter()
        .map(decode)
        .collect::<Vec<_>>()
        .into_iter()
        .collect()
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
