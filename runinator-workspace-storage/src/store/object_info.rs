#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy, Debug)]
pub struct ObjectInfo {
    pub kind: Kind,
    pub raw_len: usize,
}
