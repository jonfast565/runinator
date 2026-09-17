#[allow(unused_imports)]
use super::*;

#[derive(Clone, Debug)]
pub struct Member {
    pub id: Id,
    pub raw: Vec<u8>,
    #[cfg(test)]
    pub is_delta: bool,
}
