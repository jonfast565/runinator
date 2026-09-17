#[allow(unused_imports)]
use super::*;

pub struct BufferedStore<S> {
    pub inner: S,
    pub(super) cache: Arc<BufferedCache>,
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
