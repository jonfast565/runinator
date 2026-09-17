#[allow(unused_imports)]
use super::*;

pub struct Decoder<'a> {
    pub(super) data: &'a [u8],
    pub(super) pos: usize,
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
