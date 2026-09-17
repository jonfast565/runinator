#[allow(unused_imports)]
use super::*;

#[derive(Default)]
pub(super) struct BatchSummary {
    pub(super) accepted: usize,
    pub(super) skipped: usize,
}
