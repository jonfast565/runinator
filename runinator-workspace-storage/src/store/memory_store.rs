#[allow(unused_imports)]
use super::*;

/// Test/embedding backend. Unlike the disk backend, its memory is not bounded.
#[derive(Default)]
pub struct MemoryStore {
    pub(super) objects: RwLock<HashMap<Id, Object>>,
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
