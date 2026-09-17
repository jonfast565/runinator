#[allow(unused_imports)]
use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChunkRef {
    pub id: Id,
    pub len: u32,
}
