#[allow(unused_imports)]
use super::*;

/// Small file bytes live in this leaf, not in a page/map/chunk chain.
/// Tiny leaves are grouped physically at pack sealing; grouping never changes
/// this object's identity. `pages` and `small` are mutually exclusive.
#[derive(Clone, Debug)]
pub struct FileObject {
    pub size: u64,
    pub layout: Layout,
    pub pages: Option<Id>,
    pub small: Option<Vec<u8>>,
}

impl FileObject {
    pub fn paged(size: u64, layout: Layout, pages: Option<Id>) -> Self {
        Self {
            size,
            layout,
            pages,
            small: None,
        }
    }
    pub fn from_small(layout: Layout, bytes: Vec<u8>) -> Result<Self> {
        if bytes.len() > TINY_LIMIT {
            return Err(invalid("small file exceeds limit"));
        }
        // Nonempty all-zero files use the sparse form, not a 64 KiB zero blob.
        let size = bytes.len() as u64;
        let small = if size != 0 && bytes.iter().all(|b| *b == 0) {
            None
        } else {
            Some(bytes)
        };
        Ok(Self {
            size,
            layout,
            pages: None,
            small,
        })
    }
    pub fn is_tiny(&self) -> bool {
        self.small.as_ref().is_some_and(|b| b.len() > INLINE_LIMIT)
    }
    pub fn validate(&self) -> Result<()> {
        self.layout.validate()?;
        if let Some(bytes) = &self.small {
            if self.pages.is_some() || bytes.len() > TINY_LIMIT || bytes.len() as u64 != self.size {
                return Err(corrupt("invalid small file representation"));
            }
            if !bytes.is_empty() && bytes.iter().all(|b| *b == 0) {
                return Err(corrupt("all-zero small files must be sparse"));
            }
        }
        Ok(())
    }
}

impl Binary for FileObject {
    fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let mut e = Encoder::new();
        e.u64(self.size);
        self.layout.write(&mut e);
        match &self.small {
            None => {
                e.u8(0);
                e.optional_id(self.pages);
            }
            Some(bytes) => {
                e.u8(if bytes.len() <= INLINE_LIMIT { 1 } else { 2 });
                e.bytes(bytes)?;
            }
        }
        e.finish()
    }
    fn decode(b: &[u8]) -> Result<Self> {
        let mut d = Decoder::new(b)?;
        let size = d.u64()?;
        let layout = Layout::read(&mut d)?;
        let tag = d.u8()?;
        let (pages, small) = match tag {
            0 => (d.optional_id()?, None),
            1 | 2 => {
                let bytes = d.bytes(TINY_LIMIT)?;
                if (tag == 1) != (bytes.len() <= INLINE_LIMIT) {
                    return Err(corrupt("noncanonical small file tag"));
                }
                (None, Some(bytes))
            }
            _ => return Err(corrupt("unknown file representation")),
        };
        d.finish()?;
        let value = Self {
            size,
            layout,
            pages,
            small,
        };
        value.validate()?;
        Ok(value)
    }
}
