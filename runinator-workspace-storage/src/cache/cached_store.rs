#[allow(unused_imports)]
use super::*;

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
