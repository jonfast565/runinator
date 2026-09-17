#[allow(unused_imports)]
use super::*;

#[derive(Clone, Debug)]
pub struct RepositoryStats {
    pub generation: u64,
    pub objects: u64,
    pub packs: usize,
    pub refs: usize,
}
