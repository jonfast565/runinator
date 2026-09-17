#[allow(unused_imports)]
use super::*;

pub(crate) struct SealedPack {
    pub pack: Option<Id>,
    pub index: NamedTempFile,
}
