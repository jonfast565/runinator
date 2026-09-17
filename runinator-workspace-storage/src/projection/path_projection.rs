#[allow(unused_imports)]
use super::*;

#[derive(Clone, Debug)]
pub struct PathProjection {
    pub root: Id,
    /// inode-number -> RefList for inodes having more than one pathname.
    pub hardlinks: Option<Id>,
    /// directory-inode -> PathRef that names that directory in its parent.
    /// Root inode 1 intentionally has no entry.
    pub directory_refs: Option<Id>,
}

impl Binary for PathProjection {
    fn encode(&self) -> Result<Vec<u8>> {
        let mut e = Encoder::new();
        e.id(self.root);
        e.optional_id(self.hardlinks);
        e.optional_id(self.directory_refs);
        e.finish()
    }

    fn decode(bytes: &[u8]) -> Result<Self> {
        let mut d = Decoder::new(bytes)?;
        let value = Self {
            root: d.id()?,
            hardlinks: d.optional_id()?,
            directory_refs: d.optional_id()?,
        };
        d.finish()?;
        Ok(value)
    }
}
