#[allow(unused_imports)]
use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Inode {
    pub links: u64,
    pub metadata: Metadata,
    pub data: InodeData,
}

impl Binary for Inode {
    fn encode(&self) -> Result<Vec<u8>> {
        if self.links == 0 || self.metadata.xattrs.len() > 1024 {
            return Err(invalid("invalid inode"));
        }
        let mut e = Encoder::new();
        e.u64(self.links);
        e.u32(self.metadata.mode);
        e.i64(self.metadata.created_ns);
        e.i64(self.metadata.modified_ns);
        e.u32(self.metadata.xattrs.len() as u32);
        for (k, v) in &self.metadata.xattrs {
            if k.is_empty() || k.len() > 255 || v.len() > 65536 {
                return Err(invalid("invalid xattr"));
            }
            e.string(k)?;
            e.bytes(v)?;
        }
        match &self.data {
            InodeData::File(id) => {
                e.u8(1);
                e.id(*id);
            }
            InodeData::Directory(root) => {
                e.u8(2);
                e.optional_id(*root);
            }
            InodeData::Symlink(s) => {
                if s.is_empty() || s.len() > 4096 || s.contains('\0') || s.contains('\\') {
                    return Err(invalid("invalid symlink target"));
                }
                e.u8(3);
                e.string(s)?;
            }
        }
        e.finish()
    }
    fn decode(b: &[u8]) -> Result<Self> {
        let mut d = Decoder::new(b)?;
        let links = d.u64()?;
        let mode = d.u32()?;
        let created_ns = d.i64()?;
        let modified_ns = d.i64()?;
        let n = d.u32()?;
        if links == 0 || n > 1024 {
            return Err(corrupt("invalid inode"));
        }
        let mut xattrs = BTreeMap::new();
        let mut prev = String::new();
        for _ in 0..n {
            let k = d.string(255)?;
            if k <= prev {
                return Err(corrupt("unordered xattrs"));
            }
            prev = k.clone();
            xattrs.insert(k, d.bytes(65536)?);
        }
        let data = match d.u8()? {
            1 => InodeData::File(d.id()?),
            2 => InodeData::Directory(d.optional_id()?),
            3 => {
                let target = d.string(4096)?;
                if target.is_empty() || target.contains('\0') || target.contains('\\') {
                    return Err(corrupt("invalid symlink target"));
                }
                InodeData::Symlink(target)
            }
            _ => return Err(corrupt("invalid inode type")),
        };
        d.finish()?;
        Ok(Self {
            links,
            metadata: Metadata {
                mode,
                created_ns,
                modified_ns,
                xattrs,
            },
            data,
        })
    }
}
