//! Byte-budgeted CLOCK caches. Arc ownership is the pin: a live returned Arc
//! prevents eviction; dropping it unpins without a manual counter to leak.
use crate::{
    Error, Id,
    error::{Result, corrupt},
    model::Kind,
    store::{Object, ObjectInfo, ReadStore},
};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{Arc, Condvar, Mutex},
};
struct Entry {
    bytes: Arc<Vec<u8>>,
    referenced: bool,
}
#[derive(Default)]
struct State {
    entries: HashMap<Id, Entry>,
    clock: VecDeque<Id>,
    pending: HashSet<Id>,
    used: usize,
    reserved: usize,
    hits: u64,
    misses: u64,
}
#[derive(Clone, Copy, Debug)]
pub struct CacheStats {
    pub resident_bytes: usize,
    pub reserved_bytes: usize,
    pub entries: usize,
    pub hits: u64,
    pub misses: u64,
}
pub struct ByteCache {
    capacity: usize,
    state: Mutex<State>,
    changed: Condvar,
}
impl ByteCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            state: Mutex::new(State::default()),
            changed: Condvar::new(),
        }
    }
    pub fn capacity(&self) -> usize {
        self.capacity
    }
    pub fn stats(&self) -> Result<CacheStats> {
        let s = self.state.lock().map_err(|_| Error::Poisoned)?;
        Ok(CacheStats {
            resident_bytes: s.used,
            reserved_bytes: s.reserved,
            entries: s.entries.len(),
            hits: s.hits,
            misses: s.misses,
        })
    }
    /// Return a resident value without invoking a loader.
    pub fn get_cached(&self, id: Id) -> Result<Option<Arc<Vec<u8>>>> {
        let mut state = self.state.lock().map_err(|_| Error::Poisoned)?;
        let Some(entry) = state.entries.get_mut(&id) else {
            return Ok(None);
        };
        entry.referenced = true;
        let bytes = entry.bytes.clone();
        state.hits += 1;
        Ok(Some(bytes))
    }
    pub fn get_or_load<F>(&self, id: Id, len: usize, loader: F) -> Result<Arc<Vec<u8>>>
    where
        F: FnOnce() -> Result<Vec<u8>>,
    {
        if len > self.capacity {
            return Err(Error::CacheFull);
        }
        let mut s = self.state.lock().map_err(|_| Error::Poisoned)?;
        loop {
            if let Some(e) = s.entries.get_mut(&id) {
                if e.bytes.len() != len {
                    return Err(corrupt("cache length mismatch"));
                }
                e.referenced = true;
                let bytes = e.bytes.clone();
                s.hits += 1;
                return Ok(bytes);
            }
            if !s.pending.contains(&id) {
                break;
            }
            // Same-object requests coalesce behind a single in-flight loader.
            s = self.changed.wait(s).map_err(|_| Error::Poisoned)?;
        }
        let mut attempts = s.clock.len().saturating_mul(2);
        while s.used + s.reserved > self.capacity - len {
            if attempts == 0 {
                return Err(Error::CacheFull);
            }
            attempts -= 1;
            let Some(key) = s.clock.pop_front() else {
                return Err(Error::CacheFull);
            };
            let e = s
                .entries
                .get_mut(&key)
                .ok_or_else(|| corrupt("cache clock entry missing"))?;
            let remove = Arc::strong_count(&e.bytes) == 1 && !e.referenced;
            e.referenced = false;
            if !remove {
                s.clock.push_back(key);
                continue;
            }
            let removed = s.entries.remove(&key).unwrap();
            s.used -= removed.bytes.len();
        }
        s.pending.insert(id);
        s.reserved += len;
        s.misses += 1;
        drop(s);
        let mut reservation = Reservation {
            cache: self,
            id,
            len,
            complete: false,
        };
        let bytes = loader()?;
        if bytes.len() != len {
            return Err(corrupt("cache loader length mismatch"));
        }
        let bytes = Arc::new(bytes);
        let mut s = self.state.lock().map_err(|_| Error::Poisoned)?;
        s.reserved -= len;
        s.pending.remove(&id);
        s.used += len;
        s.clock.push_back(id);
        s.entries.insert(
            id,
            Entry {
                bytes: bytes.clone(),
                referenced: true,
            },
        );
        reservation.complete = true;
        self.changed.notify_all();
        Ok(bytes)
    }
}
struct Reservation<'a> {
    cache: &'a ByteCache,
    id: Id,
    len: usize,
    complete: bool,
}
impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        if self.complete {
            return;
        }
        if let Ok(mut s) = self.cache.state.lock() {
            s.pending.remove(&self.id);
            s.reserved = s.reserved.saturating_sub(self.len);
        }
        self.cache.changed.notify_all();
    }
}
/// Independent metadata and chunk budgets prevent a sequential data scan from
/// evicting all namespace/index metadata.
pub struct ObjectCaches {
    pub metadata: ByteCache,
    pub chunks: ByteCache,
}
impl ObjectCaches {
    pub fn new(metadata: usize, chunks: usize) -> Self {
        Self {
            metadata: ByteCache::new(metadata),
            chunks: ByteCache::new(chunks),
        }
    }
    pub fn get<S: ReadStore + ?Sized>(&self, inner: &S, id: Id) -> Result<Object> {
        let info = inner.info(id)?;
        let cache = if info.kind == Kind::Chunk {
            &self.chunks
        } else {
            &self.metadata
        };
        let bytes = cache.get_or_load(id, info.raw_len, || Ok((*inner.get(id)?.bytes).clone()))?;
        Ok(Object {
            kind: info.kind,
            bytes,
        })
    }
}
pub struct CachedStore<S> {
    pub inner: S,
    pub caches: Arc<ObjectCaches>,
}
impl<S: ReadStore> ReadStore for CachedStore<S> {
    fn get_many(&self, ids: &[Id]) -> Result<Vec<Object>> {
        let mut found = HashMap::new();
        let mut missing = Vec::new();
        let mut seen = HashSet::with_capacity(ids.len());
        for id in ids {
            if !seen.insert(*id) {
                continue;
            }
            if let Some(bytes) = self.caches.chunks.get_cached(*id)? {
                found.insert(
                    *id,
                    Object {
                        kind: Kind::Chunk,
                        bytes,
                    },
                );
                continue;
            }
            if let Some(bytes) = self.caches.metadata.get_cached(*id)? {
                let info = self.inner.info(*id)?;
                if info.kind == Kind::Chunk {
                    return Err(corrupt("object is resident in incorrect cache"));
                }
                if bytes.len() != info.raw_len {
                    return Err(corrupt("cache length mismatch"));
                }
                found.insert(
                    *id,
                    Object {
                        kind: info.kind,
                        bytes,
                    },
                );
                continue;
            }
            missing.push(*id);
        }
        let loaded = self.inner.get_many(&missing)?;
        if loaded.len() != missing.len() {
            return Err(corrupt("bulk read returned incorrect object count"));
        }
        for (id, object) in missing.into_iter().zip(loaded) {
            let cache = if object.kind == Kind::Chunk {
                &self.caches.chunks
            } else {
                &self.caches.metadata
            };
            let bytes =
                cache.get_or_load(id, object.bytes.len(), || Ok((*object.bytes).clone()))?;
            found.insert(
                id,
                Object {
                    kind: object.kind,
                    bytes,
                },
            );
        }
        Ok(ids.iter().map(|id| found[id].clone()).collect())
    }

    fn info(&self, id: Id) -> Result<ObjectInfo> {
        self.inner.info(id)
    }
    fn get(&self, id: Id) -> Result<Object> {
        self.caches.get(&self.inner, id)
    }
}

/// Bounded read-through cache for backends whose info operation also requires an object fetch.
pub struct BufferedCache {
    capacity: usize,
    state: Mutex<(usize, HashMap<Id, Object>)>,
}

impl BufferedCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            state: Mutex::new((0, HashMap::new())),
        }
    }

    fn retain(&self, id: Id, object: &Object) -> Result<()> {
        let size = object.bytes.len().saturating_add(128);
        if size <= self.capacity {
            let mut state = self.state.lock().map_err(|_| Error::Poisoned)?;
            if !state.1.contains_key(&id) {
                while state.0 > self.capacity - size {
                    let key = state.1.keys().next().copied().ok_or(Error::Poisoned)?;
                    if let Some(evicted) = state.1.remove(&key) {
                        state.0 -= evicted.bytes.len().saturating_add(128);
                    }
                }
                state.0 += size;
                state.1.insert(id, object.clone());
            }
        }
        Ok(())
    }
}

pub struct BufferedStore<S> {
    pub inner: S,
    cache: Arc<BufferedCache>,
}

impl<S> BufferedStore<S> {
    pub fn new(inner: S, capacity: usize) -> Self {
        Self::with_cache(inner, Arc::new(BufferedCache::new(capacity)))
    }

    /// Use a caller-owned cache so immutable objects survive short-lived store adapters.
    pub fn with_cache(inner: S, cache: Arc<BufferedCache>) -> Self {
        Self { inner, cache }
    }
}
impl<S: ReadStore> ReadStore for BufferedStore<S> {
    fn get_many(&self, ids: &[Id]) -> Result<Vec<Object>> {
        let mut found = HashMap::new();
        {
            let state = self.cache.state.lock().map_err(|_| Error::Poisoned)?;
            for id in ids {
                if let Some(object) = state.1.get(id) {
                    found.insert(*id, object.clone());
                }
            }
        }
        let missing: Vec<_> = ids
            .iter()
            .copied()
            .filter(|id| !found.contains_key(id))
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        let loaded = self.inner.get_many(&missing)?;
        if loaded.len() != missing.len() {
            return Err(corrupt("bulk read returned incorrect object count"));
        }
        for (id, object) in missing.into_iter().zip(loaded) {
            self.cache.retain(id, &object)?;
            found.insert(id, object);
        }
        Ok(ids.iter().map(|id| found[id].clone()).collect())
    }

    fn info(&self, id: Id) -> Result<ObjectInfo> {
        if let Some(object) = self
            .cache
            .state
            .lock()
            .map_err(|_| Error::Poisoned)?
            .1
            .get(&id)
        {
            return Ok(ObjectInfo {
                kind: object.kind,
                raw_len: object.bytes.len(),
            });
        }
        self.inner.info(id)
    }
    fn get(&self, id: Id) -> Result<Object> {
        if let Some(object) = self
            .cache
            .state
            .lock()
            .map_err(|_| Error::Poisoned)?
            .1
            .get(&id)
            .cloned()
        {
            return Ok(object);
        }
        let object = self.inner.get(id)?;
        self.cache.retain(id, &object)?;
        Ok(object)
    }
    fn contains(&self, id: Id) -> Result<bool> {
        if self
            .cache
            .state
            .lock()
            .map_err(|_| Error::Poisoned)?
            .1
            .contains_key(&id)
        {
            return Ok(true);
        }
        self.inner.contains(id)
    }
}
