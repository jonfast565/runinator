#[allow(unused_imports)]
use super::*;

#[derive(Default)]
pub(super) struct Summary {
    pub(super) healthy: usize,
    pub(super) attention: usize,
    pub(super) bad: usize,
    pub(super) inactive: usize,
}
