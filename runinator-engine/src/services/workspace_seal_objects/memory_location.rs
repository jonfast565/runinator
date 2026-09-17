#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy)]
pub(super) struct MemoryLocation {
    pub(super) location: Location,
    pub(super) info: ObjectInfo,
}
