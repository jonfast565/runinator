#[allow(unused_imports)]
use super::*;

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
