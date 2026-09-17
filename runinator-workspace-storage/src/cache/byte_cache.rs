#[allow(unused_imports)]
use super::*;

pub struct ByteCache {
    pub(super) capacity: usize,
    pub(super) state: Mutex<State>,
    pub(super) changed: Condvar,
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
