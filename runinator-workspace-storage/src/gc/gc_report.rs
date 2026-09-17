#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy, Debug)]
pub struct GcReport {
    pub before_objects: u64,
    pub live_objects: u64,
    pub removed_objects: u64,
    pub packs_before: usize,
    pub packs_after: usize,
}
