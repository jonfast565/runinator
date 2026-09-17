#[allow(unused_imports)]
use super::*;

pub(super) struct Entry {
    pub(super) bytes: Arc<Vec<u8>>,
    pub(super) referenced: bool,
}
