#[allow(unused_imports)]
use super::*;

pub(super) struct Entry {
    pub(super) meta: Arc<ObjectMeta>,
    pub(super) weight: usize,
    pub(super) generation: u64,
}
