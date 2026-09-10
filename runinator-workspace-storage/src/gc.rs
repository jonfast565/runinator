//! Reachability stays in bounded memory for ordinary graphs and spills to disk
//! for large ones. Repository::gc excludes active readers and transactions.
use crate::{
    Id,
    cache::ByteCache,
    codec::Binary,
    error::{Result, corrupt},
    model::{
        FileObject, Inode, InodeData, Kind, Link, Page, PageExtent, RadixNode, Revision, Workspace,
    },
    namespace, pages, projection,
    projection::{PathNode, PathProjection, PathRef, RefList},
    radix,
    store::{ReadStore, load},
};
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
};
use tempfile::TempDir;

const MEMORY_MARK_LIMIT: usize = 65_536;
const MEMORY_STACK_LIMIT: usize = 65_536;
const MEMORY_COUNTER_LIMIT: usize = 65_536;

pub struct DiskMarks {
    dir: TempDir,
    memory: HashSet<Id>,
    memory_limit: usize,
    spilled: bool,
    pub count: u64,
}
impl DiskMarks {
    pub fn new(parent: &Path) -> Result<Self> {
        Self::with_memory_limit(parent, MEMORY_MARK_LIMIT)
    }
    fn with_memory_limit(parent: &Path, memory_limit: usize) -> Result<Self> {
        Ok(Self {
            dir: TempDir::new_in(parent)?,
            memory: HashSet::new(),
            memory_limit,
            spilled: false,
            count: 0,
        })
    }
    fn path(&self, id: Id) -> std::path::PathBuf {
        let h = id.to_string();
        self.dir.path().join(&h[..2]).join(&h[2..])
    }
    pub fn contains(&self, id: Id) -> Result<bool> {
        if !self.spilled {
            return Ok(self.memory.contains(&id));
        }
        Ok(self.path(id).try_exists()?)
    }
    pub fn insert(&mut self, id: Id) -> Result<bool> {
        if !self.spilled && self.memory.len() < self.memory_limit {
            if self.memory.insert(id) {
                self.count += 1;
                return Ok(true);
            }
            return Ok(false);
        }
        if !self.spilled {
            self.spill()?;
        }
        if self.insert_disk(id)? {
            self.count += 1;
            return Ok(true);
        }
        Ok(false)
    }
    fn insert_disk(&self, id: Id) -> Result<bool> {
        let path = self.path(id);
        fs::create_dir_all(path.parent().unwrap())?;
        match OpenOptions::new().write(true).create_new(true).open(path) {
            Ok(_) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
            Err(e) => Err(e.into()),
        }
    }
    fn spill(&mut self) -> Result<()> {
        for &id in &self.memory {
            self.insert_disk(id)?;
        }
        self.memory.clear();
        self.spilled = true;
        Ok(())
    }
    pub fn visit<F: FnMut(Id) -> Result<()>>(&self, mut f: F) -> Result<()> {
        if !self.spilled {
            for &id in &self.memory {
                f(id)?;
            }
            return Ok(());
        }
        for bucket in fs::read_dir(self.dir.path())? {
            let bucket = bucket?;
            let prefix = bucket.file_name().to_string_lossy().into_owned();
            for item in fs::read_dir(bucket.path())? {
                let item = item?;
                let name = item.file_name().to_string_lossy().into_owned();
                f(format!("{prefix}{name}").parse()?)?;
            }
        }
        Ok(())
    }
}
pub fn references(
    kind: Kind,
    bytes: &[u8],
    include_ancestors: bool,
) -> Result<Vec<(Id, Option<Kind>)>> {
    let mut out = Vec::new();
    match kind {
        Kind::Chunk => {}
        Kind::TinyBlock | Kind::ChunkBlock => {
            return Err(corrupt("physical container is not a logical object"));
        }
        Kind::Page => {
            for extent in Page::decode(bytes)?.extents {
                if let PageExtent::Data(c) = extent {
                    out.push((c.id, Some(Kind::Chunk)));
                }
            }
        }
        Kind::Radix => {
            let n = RadixNode::decode(bytes)?;
            for id in n.children.values() {
                out.push((*id, Some(Kind::Radix)));
            }
            if let Some(id) = n.value {
                out.push((id, None));
            }
        }
        Kind::File => {
            if let Some(id) = FileObject::decode(bytes)?.pages {
                out.push((id, Some(Kind::Radix)));
            }
        }
        Kind::Inode => match Inode::decode(bytes)?.data {
            InodeData::File(id) => out.push((id, Some(Kind::File))),
            InodeData::Directory(Some(id)) => out.push((id, Some(Kind::Radix))),
            _ => {}
        },
        Kind::Link => {
            Link::decode(bytes)?;
        }
        Kind::Workspace => {
            if let Some(id) = Workspace::decode(bytes)?.inodes {
                out.push((id, Some(Kind::Radix)));
            }
        }
        Kind::Revision => {
            let r = Revision::decode(bytes)?;
            if let Some(id) = r.parent.filter(|_| include_ancestors) {
                out.push((id, Some(Kind::Revision)));
            }
            out.push((r.workspace, Some(Kind::Workspace)));
            out.push((r.projection, Some(Kind::PathProjection)));
            if let Some(id) = r.attachments {
                out.push((id, Some(Kind::Radix)));
            }
        }
        Kind::PathProjection => {
            let p = PathProjection::decode(bytes)?;
            out.push((p.root, Some(Kind::PathNode)));
            if let Some(id) = p.hardlinks {
                out.push((id, Some(Kind::Radix)));
            }
            if let Some(id) = p.directory_refs {
                out.push((id, Some(Kind::Radix)));
            }
        }
        Kind::PathNode => {
            let n = PathNode::decode(bytes)?;
            out.push((n.inode_id, Some(Kind::Inode)));
            if let Some(id) = n.children {
                out.push((id, Some(Kind::Radix)));
            }
        }
        Kind::PathList => {
            return Err(corrupt("legacy PathList object is not valid in format 6"));
        }
        Kind::PathRef => {
            PathRef::decode(bytes)?;
        }
        Kind::RefList => {
            let list = RefList::decode(bytes)?;
            for id in list.refs {
                out.push((id, Some(Kind::PathRef)));
            }
        }
    }
    Ok(out)
}
// A disk-backed LIFO keeps a directory with millions of children from making
// graph traversal allocate a correspondingly large in-memory frontier.
struct DiskStack<const N: usize> {
    file: File,
    memory: Vec<[u8; N]>,
    memory_limit: usize,
    spilled: bool,
    count: u64,
}
impl<const N: usize> DiskStack<N> {
    fn new(parent: &Path) -> Result<Self> {
        Self::with_memory_limit(parent, MEMORY_STACK_LIMIT)
    }
    fn with_memory_limit(parent: &Path, memory_limit: usize) -> Result<Self> {
        Ok(Self {
            file: tempfile::tempfile_in(parent)?,
            memory: Vec::new(),
            memory_limit,
            spilled: false,
            count: 0,
        })
    }
    fn push(&mut self, bytes: [u8; N]) -> Result<()> {
        if !self.spilled && self.memory.len() < self.memory_limit {
            self.memory.push(bytes);
            return Ok(());
        }
        if !self.spilled {
            for item in self.memory.drain(..) {
                self.file.write_all(&item)?;
                self.count += 1;
            }
            self.spilled = true;
        }
        let offset = self
            .count
            .checked_mul(N as u64)
            .ok_or_else(|| corrupt("traversal stack overflow"))?;
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.write_all(&bytes)?;
        self.count += 1;
        Ok(())
    }
    fn pop(&mut self) -> Result<Option<[u8; N]>> {
        if !self.spilled {
            return Ok(self.memory.pop());
        }
        if self.count == 0 {
            return Ok(None);
        }
        self.count -= 1;
        self.file.seek(SeekFrom::Start(self.count * N as u64))?;
        let mut bytes = [0; N];
        self.file.read_exact(&mut bytes)?;
        Ok(Some(bytes))
    }
}
fn edge(id: Id, kind: Option<Kind>) -> [u8; 33] {
    let mut b = [0; 33];
    b[..32].copy_from_slice(&id.0);
    b[32] = kind.map(|k| k as u8).unwrap_or(0);
    b
}
pub fn mark<S: ReadStore + ?Sized>(s: &S, roots: &[Id], scratch: &Path) -> Result<DiskMarks> {
    mark_roots(s, roots, scratch, true)
}
/// Mark only explicitly retained revisions when `include_ancestors` is false.
pub fn mark_roots<S: ReadStore + ?Sized>(
    s: &S,
    roots: &[Id],
    scratch: &Path,
    include_ancestors: bool,
) -> Result<DiskMarks> {
    let mut marks = DiskMarks::new(scratch)?;
    let mut stack = DiskStack::<33>::new(scratch)?;
    for &id in roots {
        stack.push(edge(id, Some(Kind::Revision)))?;
    }
    while let Some(bytes) = stack.pop()? {
        let id = Id(bytes[..32].try_into().unwrap());
        let expected = if bytes[32] == 0 {
            None
        } else {
            Some(Kind::try_from(bytes[32])?)
        };
        let info = s.info(id)?;
        if expected.is_some_and(|k| k != info.kind) {
            return Err(corrupt("object graph type mismatch"));
        }
        if !marks.insert(id)? {
            continue;
        }
        if info.kind == Kind::Chunk {
            continue;
        }
        let object = s.get(id)?;
        for (child, kind) in references(object.kind, &object.bytes, include_ancestors)? {
            stack.push(edge(child, kind))?;
        }
    }
    Ok(marks)
}
struct Counters {
    dir: TempDir,
    memory: HashMap<u64, u64>,
    memory_limit: usize,
    spilled: bool,
}
impl Counters {
    fn new(parent: &Path) -> Result<Self> {
        Self::with_memory_limit(parent, MEMORY_COUNTER_LIMIT)
    }
    fn with_memory_limit(parent: &Path, memory_limit: usize) -> Result<Self> {
        Ok(Self {
            dir: TempDir::new_in(parent)?,
            memory: HashMap::new(),
            memory_limit,
            spilled: false,
        })
    }
    fn path(&self, n: u64) -> std::path::PathBuf {
        let id = Id::sha256(&n.to_be_bytes()).to_string();
        self.dir.path().join(&id[..2]).join(&id[2..])
    }
    fn get(&self, n: u64) -> Result<u64> {
        if !self.spilled {
            return Ok(self.memory.get(&n).copied().unwrap_or(0));
        }
        match File::open(self.path(n)) {
            Ok(mut f) => {
                let mut b = [0; 8];
                f.read_exact(&mut b)?;
                Ok(u64::from_le_bytes(b))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(0),
            Err(e) => Err(e.into()),
        }
    }
    fn increment(&mut self, n: u64) -> Result<()> {
        if !self.spilled && (self.memory.contains_key(&n) || self.memory.len() < self.memory_limit)
        {
            let value = self.memory.entry(n).or_default();
            *value = value
                .checked_add(1)
                .ok_or_else(|| corrupt("link count overflow"))?;
            return Ok(());
        }
        if !self.spilled {
            self.spill()?;
        }
        let value = self
            .get(n)?
            .checked_add(1)
            .ok_or_else(|| corrupt("link count overflow"))?;
        self.write_disk(n, value)
    }
    fn write_disk(&self, n: u64, value: u64) -> Result<()> {
        let p = self.path(n);
        fs::create_dir_all(p.parent().unwrap())?;
        File::create(p)?.write_all(&value.to_le_bytes())?;
        Ok(())
    }
    fn spill(&mut self) -> Result<()> {
        for (&n, &value) in &self.memory {
            self.write_disk(n, value)?;
        }
        self.memory.clear();
        self.spilled = true;
        Ok(())
    }
}
/// Verifies directory targets, link counts, root reachability and absence of
/// directory hard links/cycles. Counts and visited inode IDs spill to disk.
pub fn verify_workspace<S: ReadStore + ?Sized>(s: &S, w: &Workspace, scratch: &Path) -> Result<()> {
    let root = namespace::inode(s, w, 1)?;
    if !matches!(root.data, InodeData::Directory(_)) {
        return Err(corrupt("root inode is not a directory"));
    }
    let mut counts = Counters::new(scratch)?;
    counts.increment(1)?;
    radix::visit(s, w.inodes, &mut |key, id| {
        let n = u64::from_be_bytes(
            key.try_into()
                .map_err(|_| corrupt("invalid inode-map key"))?,
        );
        if n == 0 || n >= w.next_inode {
            return Err(corrupt("inode number outside allocation range"));
        }
        let i: Inode = load(s, id, Kind::Inode)?;
        if let InodeData::Directory(entries) = i.data {
            if i.links != 1 {
                return Err(corrupt("directory hard links are unsupported"));
            }
            radix::visit(s, entries, &mut |name, id| {
                let name =
                    std::str::from_utf8(name).map_err(|_| corrupt("invalid directory UTF-8"))?;
                if name.contains('/') || namespace::components(name)?.len() != 1 {
                    return Err(corrupt("invalid directory name"));
                }
                let link: Link = load(s, id, Kind::Link)?;
                if link.0 == 1 {
                    return Err(corrupt("link to root inode"));
                }
                namespace::inode(s, w, link.0)?;
                counts.increment(link.0)
            })?;
        }
        Ok(())
    })?;
    let mut visited = DiskMarks::new(scratch)?;
    let mut stack = DiskStack::<8>::new(scratch)?;
    stack.push(1u64.to_be_bytes())?;
    while let Some(bytes) = stack.pop()? {
        let n = u64::from_be_bytes(bytes);
        if !visited.insert(Id::sha256(&n.to_be_bytes()))? {
            continue;
        }
        let i = namespace::inode(s, w, n)?;
        let InodeData::Directory(entries) = i.data else {
            continue;
        };

        radix::visit(s, entries, &mut |_, id| {
            stack.push(load::<Link, _>(s, id, Kind::Link)?.0.to_be_bytes())?;
            Ok(())
        })?;
    }
    radix::visit(s, w.inodes, &mut |key, id| {
        let n = u64::from_be_bytes(key.try_into().map_err(|_| corrupt("bad inode-map key"))?);
        let i: Inode = load(s, id, Kind::Inode)?;
        if counts.get(n)? != i.links || !visited.contains(Id::sha256(&n.to_be_bytes()))? {
            return Err(corrupt("orphan inode or incorrect link count"));
        }
        Ok(())
    })
}

pub fn verify_file<S: ReadStore + ?Sized>(s: &S, cache: &ByteCache, f: &FileObject) -> Result<()> {
    f.validate()?;
    let p = f.layout.page_size as u64;
    radix::visit(s, f.pages, &mut |key, id| {
        let n = u64::from_be_bytes(key.try_into().map_err(|_| corrupt("bad page-map key"))?);
        if f.size == 0 || n > (f.size - 1) / p {
            return Err(corrupt("page mapping beyond EOF"));
        }
        let manifest: Page = load(s, id, Kind::Page)?;
        let allowed = (f.size - n * p).min(p);
        if manifest.used as u64 > allowed {
            return Err(corrupt("page data beyond EOF"));
        }
        pages::pin_page(s, cache, id, f.layout.page_size)?;
        Ok(())
    })
}

fn verify_file_from_verified_info<S: ReadStore + ?Sized>(s: &S, f: &FileObject) -> Result<()> {
    f.validate()?;
    let page_size = f.layout.page_size as u64;
    radix::visit(s, f.pages, &mut |key, id| {
        let number = u64::from_be_bytes(key.try_into().map_err(|_| corrupt("bad page-map key"))?);
        if f.size == 0 || number > (f.size - 1) / page_size {
            return Err(corrupt("page mapping beyond EOF"));
        }
        let manifest: Page = load(s, id, Kind::Page)?;
        if manifest.page_size != f.layout.page_size {
            return Err(corrupt("page-size mismatch"));
        }
        let allowed = (f.size - number * page_size).min(page_size);
        if manifest.used as u64 > allowed {
            return Err(corrupt("page data beyond EOF"));
        }
        let mut final_chunk = None;
        for extent in &manifest.extents {
            let PageExtent::Data(chunk) = extent else {
                continue;
            };

            let info = s.info(chunk.id)?;
            if info.kind != Kind::Chunk || info.raw_len != chunk.len as usize {
                return Err(corrupt("invalid chunk reference"));
            }
            final_chunk = Some(chunk.id);
        }
        let final_chunk = final_chunk.ok_or_else(|| corrupt("page has no data extent"))?;
        let object = s.get(final_chunk)?;
        if object.kind != Kind::Chunk || object.bytes.last().is_none_or(|byte| *byte == 0) {
            return Err(corrupt("noncanonical page padding"));
        }
        Ok(())
    })
}
pub fn verify_graph<S: ReadStore + ?Sized>(
    s: &S,
    roots: &[Id],
    scratch: &Path,
) -> Result<DiskMarks> {
    verify_roots(s, roots, scratch, true)
}
pub fn verify_roots<S: ReadStore + ?Sized>(
    s: &S,
    roots: &[Id],
    scratch: &Path,
    include_ancestors: bool,
) -> Result<DiskMarks> {
    verify_roots_inner(s, roots, scratch, include_ancestors, false)
}

/// Verify a graph whose `ObjectInfo` values came from validated immutable packs.
/// Chunk lengths use that trusted metadata, while structural objects and the
/// final chunk of each page are still decoded and checked.
pub fn verify_roots_with_verified_info<S: ReadStore + ?Sized>(
    s: &S,
    roots: &[Id],
    scratch: &Path,
    include_ancestors: bool,
) -> Result<DiskMarks> {
    verify_roots_inner(s, roots, scratch, include_ancestors, true)
}

fn verify_roots_inner<S: ReadStore + ?Sized>(
    s: &S,
    roots: &[Id],
    scratch: &Path,
    include_ancestors: bool,
    verified_info: bool,
) -> Result<DiskMarks> {
    let marks = mark_roots(s, roots, scratch, include_ancestors)?;
    let pages = ByteCache::new(8 * 1024 * 1024);
    marks.visit(|id| {
        let kind = s.info(id)?.kind;
        match kind {
            Kind::Workspace => verify_workspace(s, &load(s, id, Kind::Workspace)?, scratch)?,
            Kind::File => {
                let file = load(s, id, Kind::File)?;
                if verified_info {
                    verify_file_from_verified_info(s, &file)?;
                } else {
                    verify_file(s, &pages, &file)?;
                }
            }
            Kind::Revision => {
                let revision: Revision = load(s, id, Kind::Revision)?;
                let workspace: Workspace = load(s, revision.workspace, Kind::Workspace)?;
                let p: PathProjection = load(s, revision.projection, Kind::PathProjection)?;
                projection::verify(s, &workspace, &p)?;
            }
            _ => {}
        }
        Ok(())
    })?;
    Ok(marks)
}
#[derive(Clone, Copy, Debug)]
pub struct GcReport {
    pub before_objects: u64,
    pub live_objects: u64,
    pub removed_objects: u64,
    pub packs_before: usize,
    pub packs_after: usize,
}

#[cfg(test)]
#[path = "gc_tests.rs"]
mod gc_tests;
