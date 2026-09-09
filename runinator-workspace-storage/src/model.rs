use crate::{
    Id,
    codec::{Binary, Decoder, Encoder},
    error::{Result, corrupt, invalid},
};
use std::collections::BTreeMap;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Kind {
    Chunk = 1,
    Page = 2,
    Radix = 3,
    File = 4,
    Inode = 5,
    Link = 6,
    Workspace = 7,
    Revision = 8,
    PathProjection = 9,
    PathNode = 10,
    PathList = 11,
    PathRef = 12,
    RefList = 13,
    /// Physical-only container; never a logical Merkle edge.
    TinyBlock = 14,
    /// Physical-only compression group for logical Chunk objects.
    ChunkBlock = 15,
}
impl TryFrom<u8> for Kind {
    type Error = crate::Error;
    fn try_from(v: u8) -> Result<Self> {
        Ok(match v {
            1 => Self::Chunk,
            2 => Self::Page,
            3 => Self::Radix,
            4 => Self::File,
            5 => Self::Inode,
            6 => Self::Link,
            7 => Self::Workspace,
            8 => Self::Revision,
            9 => Self::PathProjection,
            10 => Self::PathNode,
            11 => Self::PathList,
            12 => Self::PathRef,
            13 => Self::RefList,
            14 => Self::TinyBlock,
            15 => Self::ChunkBlock,
            _ => return Err(corrupt("unknown object kind")),
        })
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Layout {
    pub page_size: u32,
    pub min: u32,
    pub avg: u32,
    pub max: u32,
}
impl Default for Layout {
    fn default() -> Self {
        Self {
            page_size: 4 * 1024 * 1024,
            min: 64 * 1024,
            avg: 256 * 1024,
            max: 1024 * 1024,
        }
    }
}
impl Layout {
    /// Select a page size before ingestion. It remains fixed for this file.
    /// A caller with an unknown input length uses the default 4 MiB layout.
    pub fn for_size(size: u64) -> Self {
        let page_size = if size < 16 * 1024 * 1024 {
            1024 * 1024
        } else if size < 1024 * 1024 * 1024 {
            4 * 1024 * 1024
        } else {
            8 * 1024 * 1024
        };
        Self {
            page_size,
            ..Self::default()
        }
    }

    pub fn validate(&self) -> Result<()> {
        if !self.page_size.is_power_of_two() || !(4096..=8 * 1024 * 1024).contains(&self.page_size)
        {
            return Err(invalid(
                "page size must be a power of two from 4 KiB through 8 MiB",
            ));
        }
        if !(64..=1_048_576).contains(&self.min)
            || !(256..=4_194_304).contains(&self.avg)
            || !(1024..=16_777_216).contains(&self.max)
            || self.min > self.avg
            || self.avg > self.max
            || self.max > self.page_size
        {
            return Err(invalid("invalid FastCDC sizes"));
        }
        Ok(())
    }
    fn write(&self, e: &mut Encoder) {
        e.u32(self.page_size);
        e.u32(self.min);
        e.u32(self.avg);
        e.u32(self.max);
        e.u8(1);
        e.u64(0);
    }
    fn read(d: &mut Decoder<'_>) -> Result<Self> {
        let value = Self {
            page_size: d.u32()?,
            min: d.u32()?,
            avg: d.u32()?,
            max: d.u32()?,
        };
        if d.u8()? != 1 || d.u64()? != 0 {
            return Err(corrupt("unsupported FastCDC normalization/seed"));
        }
        value.validate()?;
        Ok(value)
    }
}
/// Representation limits are format constants, not per-process heuristics.
pub const INLINE_LIMIT: usize = 128;
pub const TINY_LIMIT: usize = 64 * 1024;
pub const ZERO_RUN_MIN: usize = 4096;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChunkRef {
    pub id: Id,
    pub len: u32,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PageExtent {
    Data(ChunkRef),
    Zero(u32),
}
impl PageExtent {
    pub fn len(&self) -> u32 {
        match self {
            Self::Data(c) => c.len,
            Self::Zero(n) => *n,
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
#[derive(Clone, Debug)]
pub struct Page {
    pub page_size: u32,
    pub used: u32,
    pub extents: Vec<PageExtent>,
}
impl Page {
    fn validate(&self) -> Result<()> {
        if !(4096..=8 * 1024 * 1024).contains(&self.page_size)
            || !self.page_size.is_power_of_two()
            || self.used == 0
            || self.used > self.page_size
            || self.extents.is_empty()
            || self.extents.len() > 131072
        {
            return Err(corrupt("invalid page manifest"));
        }
        let mut total = 0u64;
        let mut previous_zero = false;
        for extent in &self.extents {
            if extent.is_empty() {
                return Err(corrupt("empty page extent"));
            }
            let zero = matches!(extent, PageExtent::Zero(_));
            if zero && (previous_zero || extent.len() < ZERO_RUN_MIN as u32) {
                return Err(corrupt("noncanonical zero extent"));
            }
            previous_zero = zero;
            total += extent.len() as u64;
        }
        if previous_zero || total != self.used as u64 {
            return Err(corrupt("invalid extent lengths or explicit trailing zeros"));
        }
        Ok(())
    }
}
impl Binary for Page {
    fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let mut e = Encoder::new();
        e.u32(self.page_size);
        e.u32(self.used);
        e.u32(self.extents.len() as u32);
        for extent in &self.extents {
            match extent {
                PageExtent::Data(c) => {
                    e.u8(1);
                    e.id(c.id);
                    e.u32(c.len);
                }
                PageExtent::Zero(n) => {
                    e.u8(0);
                    e.u32(*n);
                }
            }
        }
        e.finish()
    }
    fn decode(b: &[u8]) -> Result<Self> {
        let mut d = Decoder::new(b)?;
        let page_size = d.u32()?;
        let used = d.u32()?;
        let n = d.u32()?;
        if n == 0 || n > 131072 {
            return Err(corrupt("invalid extent count"));
        }
        let mut extents = Vec::new();
        for _ in 0..n {
            extents.push(match d.u8()? {
                0 => PageExtent::Zero(d.u32()?),
                1 => PageExtent::Data(ChunkRef {
                    id: d.id()?,
                    len: d.u32()?,
                }),
                _ => return Err(corrupt("unknown page extent tag")),
            });
        }
        d.finish()?;
        let value = Self {
            page_size,
            used,
            extents,
        };
        value.validate()?;
        Ok(value)
    }
}
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
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Metadata {
    pub mode: u32,
    pub created_ns: i64,
    pub modified_ns: i64,
    pub xattrs: BTreeMap<String, Vec<u8>>,
}
impl Default for Metadata {
    fn default() -> Self {
        Self {
            mode: 0o644,
            created_ns: 0,
            modified_ns: 0,
            xattrs: BTreeMap::new(),
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InodeData {
    File(Id),
    Directory(Option<Id>),
    Symlink(String),
}
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
