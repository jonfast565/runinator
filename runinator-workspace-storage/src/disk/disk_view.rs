#[allow(unused_imports)]
use super::*;

#[derive(Clone)]
pub struct DiskView {
    pub(crate) raw: RawDisk,
    pub(crate) caches: Arc<ObjectCaches>,
}

impl ReadStore for DiskView {
    fn info(&self, id: Id) -> Result<ObjectInfo> {
        self.raw.info(id)
    }
    fn get(&self, id: Id) -> Result<Object> {
        self.caches.get(&self.raw, id)
    }
    fn contains(&self, id: Id) -> Result<bool> {
        self.raw.contains(id)
    }
}
