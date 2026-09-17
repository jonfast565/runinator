#[allow(unused_imports)]
use super::*;

pub(super) struct Meta<'a> {
    pub(super) id: Id,
    pub(super) raw_len: usize,
    pub(super) codec: u8,
    pub(super) base: u32,
    pub(super) payload: &'a [u8],
}
