#[allow(unused_imports)]
use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Metadata {
    pub mode: u32,
    pub created_ns: i64,
    pub modified_ns: i64,
    pub xattrs: BTreeMap<String, Vec<u8>>,
}

impl Default for Metadata {
    fn default() -> Self {
        Self {
            mode: 0o644,
            created_ns: 0,
            modified_ns: 0,
            xattrs: BTreeMap::new(),
        }
    }
}
