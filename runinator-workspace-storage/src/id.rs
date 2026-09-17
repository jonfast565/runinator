use crate::{
    error::{Result, invalid},
    model::Kind,
};
use std::{fmt, str::FromStr};
#[derive(
    Clone,
    Copy,
    Default,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    Hash,
    Debug,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct Id(pub [u8; 32]);
impl Id {
    /// Domain separation prevents a metadata object and a data chunk with the
    /// same payload from being interpreted as one another.
    pub fn object(kind: Kind, raw: &[u8]) -> Self {
        let mut h = blake3::Hasher::new();
        h.update(b"runinator/workspace/object/v1\0");
        h.update(&[kind as u8]);
        h.update(&(raw.len() as u64).to_le_bytes());
        h.update(raw);
        Self(*h.finalize().as_bytes())
    }
    pub fn sha256(bytes: &[u8]) -> Self {
        Self(runinator_hash::sha256(bytes))
    }
}
impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for b in self.0 {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}
impl FromStr for Id {
    type Err = crate::Error;
    fn from_str(s: &str) -> Result<Self> {
        let bytes = runinator_hash::parse_lowercase_hex(s)
            .ok_or_else(|| invalid("digest must be 64 lowercase hexadecimal characters"))?;
        Ok(Self(bytes))
    }
}
