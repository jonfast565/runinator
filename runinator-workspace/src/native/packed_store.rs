#[allow(unused_imports)]
use super::*;

pub struct PackedStore {
    pub(super) _directory: tempfile::TempDir,
    pub(super) packs: Vec<File>,
    pub(super) objects: HashMap<Id, PackedObject>,
    pub(super) blocks: ByteCache,
}

impl PackedStore {
    pub fn object_count(&self) -> usize {
        self.objects.len()
    }

    pub fn pack_count(&self) -> usize {
        self.packs.len()
    }
}

impl ReadStore for PackedStore {
    fn info(&self, id: Id) -> storage::Result<ObjectInfo> {
        self.objects
            .get(&id)
            .map(|entry| entry.info)
            .ok_or_else(|| storage::Error::NotFound(id.to_string()))
    }

    fn get(&self, id: Id) -> storage::Result<Object> {
        let entry = self
            .objects
            .get(&id)
            .ok_or_else(|| storage::Error::NotFound(id.to_string()))?;
        storage::record::read_indexed(&self.packs[entry.pack], entry.location, &self.blocks)
    }

    fn contains(&self, id: Id) -> storage::Result<bool> {
        Ok(self.objects.contains_key(&id))
    }
}
