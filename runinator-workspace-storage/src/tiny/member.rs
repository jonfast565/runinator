#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy)]
pub struct Member<'a> {
    pub id: Id,
    pub raw: &'a [u8],
}
