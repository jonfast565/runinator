#[allow(unused_imports)]
use super::*;

pub struct EmptyStore;

impl ReadStore for EmptyStore {
    fn info(&self, id: Id) -> Result<ObjectInfo> {
        Err(Error::NotFound(id.to_string()))
    }
    fn get(&self, id: Id) -> Result<Object> {
        Err(Error::NotFound(id.to_string()))
    }

    fn contains(&self, _: Id) -> Result<bool> {
        Ok(false)
    }
}
