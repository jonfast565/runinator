#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy)]
pub(super) struct StagedObject {
    pub(super) offset: u64,
    pub(super) info: ObjectInfo,
}
