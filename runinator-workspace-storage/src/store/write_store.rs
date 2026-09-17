#[allow(unused_imports)]
use super::*;

pub trait WriteStore: ReadStore {
    fn put(&self, kind: Kind, raw: &[u8]) -> Result<Id>;
}

impl<S: WriteStore + ?Sized> WriteStore for &S {
    fn put(&self, kind: Kind, raw: &[u8]) -> Result<Id> {
        (**self).put(kind, raw)
    }
}
