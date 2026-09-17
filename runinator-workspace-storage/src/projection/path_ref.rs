#[allow(unused_imports)]
use super::*;

/// Stable structural name for one directory entry.
///
/// The parent is an inode number, not another path object, so moving an
/// ancestor does not invalidate this ref. The basename is the only string
/// retained and is bounded by the namespace component limit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PathRef {
    pub parent_inode: u64,
    pub name: String,
}

impl Binary for PathRef {
    fn encode(&self) -> Result<Vec<u8>> {
        if self.parent_inode == 0 {
            return Err(invalid("path ref parent inode is zero"));
        }
        validate_name(&self.name)?;
        let mut e = Encoder::new();
        e.u64(self.parent_inode);
        e.string(&self.name)?;
        e.finish()
    }

    fn decode(bytes: &[u8]) -> Result<Self> {
        let mut d = Decoder::new(bytes)?;
        let parent_inode = d.u64()?;
        let name = d.string(255)?;
        if parent_inode == 0 || validate_name(&name).is_err() {
            return Err(corrupt("invalid structural path ref"));
        }
        d.finish()?;
        Ok(Self { parent_inode, name })
    }
}
