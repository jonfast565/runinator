#[allow(unused_imports)]
use super::*;

#[derive(Default)]
pub(crate) struct ListingStats {
    pub(crate) visited_entries: u64,
    pub(crate) metadata_reads: u64,
}
