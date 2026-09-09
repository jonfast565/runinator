//! A fixed, versioned binary schema; no JSON, map ordering, or host-endian
//! behavior participates in immutable object identity.
use crate::{
    Id,
    error::{Result, corrupt, invalid},
};
pub const MAX_OBJECT: usize = 16 * 1024 * 1024;
#[derive(Default)]
pub struct Encoder(pub Vec<u8>);
impl Encoder {
    pub fn new() -> Self {
        Self(vec![3])
    }
    pub fn u8(&mut self, n: u8) {
        self.0.push(n);
    }
    pub fn u16(&mut self, n: u16) {
        self.0.extend(n.to_le_bytes());
    }
    pub fn u32(&mut self, n: u32) {
        self.0.extend(n.to_le_bytes());
    }
    pub fn u64(&mut self, n: u64) {
        self.0.extend(n.to_le_bytes());
    }
    pub fn i64(&mut self, n: i64) {
        self.0.extend(n.to_le_bytes());
    }
    pub fn id(&mut self, id: Id) {
        self.0.extend(id.0);
    }
    pub fn optional_id(&mut self, id: Option<Id>) {
        self.u8(id.is_some() as u8);
        if let Some(id) = id {
            self.id(id);
        }
    }
    pub fn bytes(&mut self, b: &[u8]) -> Result<()> {
        let len = u32::try_from(b.len()).map_err(|_| invalid("field too large"))?;
        self.u32(len);
        self.0.extend_from_slice(b);
        Ok(())
    }
    pub fn string(&mut self, s: &str) -> Result<()> {
        self.bytes(s.as_bytes())
    }
    pub fn finish(self) -> Result<Vec<u8>> {
        if self.0.len() > MAX_OBJECT {
            return Err(invalid("metadata object exceeds limit"));
        }
        Ok(self.0)
    }
}
pub struct Decoder<'a> {
    data: &'a [u8],
    pos: usize,
}
impl<'a> Decoder<'a> {
    pub fn new(data: &'a [u8]) -> Result<Self> {
        if data.len() > MAX_OBJECT {
            return Err(corrupt("oversized metadata"));
        }
        let mut d = Self { data, pos: 0 };
        if d.u8()? != 3 {
            return Err(corrupt("unsupported metadata version"));
        }
        Ok(d)
    }
    pub fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or_else(|| corrupt("length overflow"))?;
        let bytes = self
            .data
            .get(self.pos..end)
            .ok_or_else(|| corrupt("truncated metadata"))?;
        self.pos = end;
        Ok(bytes)
    }
    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    pub fn i64(&mut self) -> Result<i64> {
        Ok(i64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    pub fn id(&mut self) -> Result<Id> {
        Ok(Id(self.take(32)?.try_into().unwrap()))
    }
    pub fn optional_id(&mut self) -> Result<Option<Id>> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.id()?)),
            _ => Err(corrupt("noncanonical option")),
        }
    }
    pub fn bytes(&mut self, max: usize) -> Result<Vec<u8>> {
        let n = self.u32()? as usize;
        if n > max {
            return Err(corrupt("field exceeds limit"));
        }
        Ok(self.take(n)?.to_vec())
    }
    pub fn string(&mut self, max: usize) -> Result<String> {
        String::from_utf8(self.bytes(max)?).map_err(|_| corrupt("invalid UTF-8"))
    }
    pub fn finish(self) -> Result<()> {
        if self.pos != self.data.len() {
            return Err(corrupt("trailing metadata bytes"));
        }
        Ok(())
    }
}
pub trait Binary: Sized {
    fn encode(&self) -> Result<Vec<u8>>;
    fn decode(data: &[u8]) -> Result<Self>;
}
