#[allow(unused_imports)]
use super::*;

#[derive(Clone, Debug)]
pub struct Workspace {
    pub inodes: Option<Id>,
    pub next_inode: u64,
}

impl Binary for Workspace {
    fn encode(&self) -> Result<Vec<u8>> {
        let mut e = Encoder::new();
        e.optional_id(self.inodes);
        e.u64(self.next_inode);
        e.finish()
    }
    fn decode(b: &[u8]) -> Result<Self> {
        let mut d = Decoder::new(b)?;
        let x = Self {
            inodes: d.optional_id()?,
            next_inode: d.u64()?,
        };
        d.finish()?;
        if x.next_inode < 2 || x.inodes.is_none() {
            return Err(corrupt("invalid workspace"));
        }
        Ok(x)
    }
}
