#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct ReleaseKey {
    pub(super) timestamp: u64,
    pub(super) major: u64,
    pub(super) minor: u64,
    pub(super) build: u64,
    pub(super) tag: String,
}
