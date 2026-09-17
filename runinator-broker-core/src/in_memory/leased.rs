#[allow(unused_imports)]
use super::*;

pub(super) struct Leased<T> {
    pub(super) delivery: T,
    pub(super) leased_until: Instant,
}
