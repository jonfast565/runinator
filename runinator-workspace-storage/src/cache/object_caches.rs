#[allow(unused_imports)]
use super::*;

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
