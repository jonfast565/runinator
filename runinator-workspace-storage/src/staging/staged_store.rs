#[allow(unused_imports)]
use super::*;

pub trait StagedStore: ReadStore {
    fn is_staged(&self, id: Id) -> Result<bool>;
}
