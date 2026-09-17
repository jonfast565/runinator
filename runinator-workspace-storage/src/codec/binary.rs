#[allow(unused_imports)]
use super::*;

pub trait Binary: Sized {
    fn encode(&self) -> Result<Vec<u8>>;
    fn decode(data: &[u8]) -> Result<Self>;
}
