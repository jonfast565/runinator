#[allow(unused_imports)]
use super::*;

pub(super) struct LocalPack {
    pub(super) index: DiskIndex,
    pub(super) memory_index: Option<HashMap<Id, MemoryLocation>>,
    pub(super) _directory: tempfile::TempDir,
}

impl LocalPack {
    pub(super) fn lookup(&self, id: Id) -> storage::Result<Option<(Location, Option<ObjectInfo>)>> {
        if let Some(index) = &self.memory_index {
            return Ok(index
                .get(&id)
                .map(|found| (found.location, Some(found.info))));
        }
        Ok(self.index.lookup(id)?.map(|location| (location, None)))
    }
}
