#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy, Debug)]
pub struct Link(pub u64);

impl Binary for Link {
    fn encode(&self) -> Result<Vec<u8>> {
        let mut e = Encoder::new();
        e.u64(self.0);
        e.finish()
    }
    fn decode(b: &[u8]) -> Result<Self> {
        let mut d = Decoder::new(b)?;
        let v = Self(d.u64()?);
        d.finish()?;
        Ok(v)
    }
}
