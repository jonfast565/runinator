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

mod object_info;
pub use object_info::ObjectInfo;

mod object;
pub use object::Object;

mod read_store;
pub use read_store::ReadStore;

mod write_store;
pub use write_store::WriteStore;

mod memory_store;
pub use memory_store::MemoryStore;
