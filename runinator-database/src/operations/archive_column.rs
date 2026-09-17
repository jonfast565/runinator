#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy)]
pub(crate) struct ArchiveColumn {
    pub(super) name: &'static str,
    pub(super) kind: ArchiveColumnKind,
}
