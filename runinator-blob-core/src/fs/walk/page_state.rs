#[allow(unused_imports)]
use super::*;

pub(super) struct PageState<'a> {
    pub(super) bucket: &'a str,
    pub(super) request: &'a ListRequest,
    pub(super) cache: &'a MetadataCache,
    pub(super) objects: Vec<ObjectSummary>,
    pub(super) common_prefixes: BTreeSet<String>,
    pub(super) last_seen: Option<String>,
    pub(super) truncated: bool,
    pub(super) stats: ListingStats,
}
