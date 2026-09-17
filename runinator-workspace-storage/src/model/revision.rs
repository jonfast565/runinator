#[allow(unused_imports)]
use super::*;

#[derive(Clone, Debug)]
pub struct Revision {
    pub parent: Option<Id>,
    pub workspace: Id,
    /// Revision-local Merkle projection of path -> inode/content identity.
    pub projection: Id,
    pub attachments: Option<Id>,
    pub message: String,
}

impl Binary for Revision {
    fn encode(&self) -> Result<Vec<u8>> {
        if self.message.len() > 65536 {
            return Err(invalid("commit message too large"));
        }
        let mut e = Encoder::new();
        e.optional_id(self.parent);
        e.id(self.workspace);
        e.id(self.projection);
        e.optional_id(self.attachments);
        e.string(&self.message)?;
        e.finish()
    }
    fn decode(b: &[u8]) -> Result<Self> {
        let mut d = Decoder::new(b)?;
        let x = Self {
            parent: d.optional_id()?,
            workspace: d.id()?,
            projection: d.id()?,
            attachments: d.optional_id()?,
            message: d.string(65536)?,
        };
        d.finish()?;
        Ok(x)
    }
}
