#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SourceSnapshot {
    pub(super) files: Vec<SourceFileSnapshot>,
}
