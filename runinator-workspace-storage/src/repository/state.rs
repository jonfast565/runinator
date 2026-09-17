#[allow(unused_imports)]
use super::*;

pub(super) struct State {
    pub(super) digest: Id,
    pub(super) catalog: Catalog,
    pub(super) index: DiskIndex,
    pub(super) uncertain: bool,
}
