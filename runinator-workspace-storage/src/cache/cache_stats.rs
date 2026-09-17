#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy, Debug)]
pub struct CacheStats {
    pub resident_bytes: usize,
    pub reserved_bytes: usize,
    pub entries: usize,
    pub hits: u64,
    pub misses: u64,
}
