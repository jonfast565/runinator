#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy)]
pub(super) struct PackedObject {
    pub(super) pack: usize,
    pub(super) location: Location,
    pub(super) info: ObjectInfo,
}
