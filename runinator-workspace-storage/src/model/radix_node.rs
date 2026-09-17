#[allow(unused_imports)]
use super::*;

/// Compressed byte-radix (Patricia) node. Child edge bytes are outside the
/// child's prefix. Values are object IDs, so GC can traverse without guessing.
#[derive(Clone, Debug, Default)]
pub struct RadixNode {
    pub prefix: Vec<u8>,
    pub value: Option<Id>,
    pub children: BTreeMap<u8, Id>,
}

impl Binary for RadixNode {
    fn encode(&self) -> Result<Vec<u8>> {
        let mut e = Encoder::new();
        e.bytes(&self.prefix)?;
        e.optional_id(self.value);
        e.u16(self.children.len() as u16);
        for (&key, &id) in &self.children {
            e.u8(key);
            e.id(id);
        }
        e.finish()
    }
    fn decode(b: &[u8]) -> Result<Self> {
        let mut d = Decoder::new(b)?;
        let prefix = d.bytes(4096)?;
        let value = d.optional_id()?;
        let n = d.u16()?;
        if n > 256 {
            return Err(corrupt("too many radix children"));
        }
        let mut children = BTreeMap::new();
        let mut previous = None;
        for _ in 0..n {
            let k = d.u8()?;
            if previous.is_some_and(|p| p >= k) {
                return Err(corrupt("unordered radix edges"));
            }
            previous = Some(k);
            children.insert(k, d.id()?);
        }
        d.finish()?;
        if value.is_none() && children.len() < 2 {
            return Err(corrupt("noncanonical uncompressed radix node"));
        }
        Ok(Self {
            prefix,
            value,
            children,
        })
    }
}
