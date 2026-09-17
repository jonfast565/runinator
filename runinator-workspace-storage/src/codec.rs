//! A fixed, versioned binary schema; no JSON, map ordering, or host-endian
//! behavior participates in immutable object identity.
use crate::{
    Id,
    error::{Result, corrupt, invalid},
};
pub const MAX_OBJECT: usize = 16 * 1024 * 1024;

mod encoder;
pub use encoder::Encoder;

mod decoder;
pub use decoder::Decoder;

mod binary;
pub use binary::Binary;
