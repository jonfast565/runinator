use crate::{
    error::{Result, invalid},
    model::Kind,
};
use sha2::{Digest, Sha256};
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
        Self(Sha256::digest(bytes).into())
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
        if s.len() != 64
            || !s
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(invalid(
                "digest must be 64 lowercase hexadecimal characters",
            ));
        }
        let mut bytes = [0; 32];
        for (i, b) in bytes.iter_mut().enumerate() {
            *b = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16)
                .map_err(|_| invalid("invalid digest"))?;
        }
        Ok(Self(bytes))
    }
}
