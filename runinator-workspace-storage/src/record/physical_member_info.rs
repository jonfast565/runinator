#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy, Debug)]
pub struct PhysicalMemberInfo {
    pub id: Id,
    pub member: u32,
    pub kind: Kind,
    pub raw_len: usize,
}
