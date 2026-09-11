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
use rayon::prelude::*;
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
};
use tempfile::TempDir;

const MEMORY_MARK_LIMIT: usize = 65_536;
const MEMORY_STACK_LIMIT: usize = 65_536;
const MEMORY_COUNTER_LIMIT: usize = 65_536;
const MARK_SLOT_BYTES: u64 = 33;
const MARK_BATCH: usize = 256;
const COUNTER_SLOT_BYTES: u64 = 17;

pub struct DiskMarks {
    dir: TempDir,
    memory: HashSet<Id>,
    memory_limit: usize,
    table: Option<File>,
    table_slots: usize,
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
            table: None,
            table_slots: 0,
            count: 0,
        })
    }
    pub fn contains(&self, id: Id) -> Result<bool> {
        if self.table.is_none() {
            return Ok(self.memory.contains(&id));
        }
        self.contains_disk(id)
    }
    pub fn insert(&mut self, id: Id) -> Result<bool> {
        if self.table.is_none() && self.memory.len() < self.memory_limit {
            if self.memory.insert(id) {
                self.count += 1;
                return Ok(true);
            }
            return Ok(false);
        }
        if self.table.is_none() {
            self.spill()?;
        }
        if self.count.saturating_add(1).saturating_mul(10) >= self.table_slots as u64 * 7 {
            if self.contains_disk(id)? {
                return Ok(false);
            }
            self.grow()?;
        }
        if self.insert_disk(id)? {
            self.count += 1;
            return Ok(true);
        }
        Ok(false)
    }
    fn slot(id: Id, slots: usize) -> usize {
        let hash = u64::from_le_bytes(id.0[..8].try_into().unwrap()) as usize;
        hash & (slots - 1)
    }
    fn contains_disk(&self, id: Id) -> Result<bool> {
        let file = self
            .table
            .as_ref()
            .ok_or_else(|| corrupt("missing mark table"))?;
        let mut slot = Self::slot(id, self.table_slots);
        for _ in 0..self.table_slots {
            let mut bytes = [0; MARK_SLOT_BYTES as usize];
            crate::io_util::read_at(file, &mut bytes, slot as u64 * MARK_SLOT_BYTES)?;
            if bytes[0] == 0 {
                return Ok(false);
            }
            if bytes[1..] == id.0 {
                return Ok(true);
            }
            slot = (slot + 1) & (self.table_slots - 1);
        }
        Ok(false)
    }
    fn insert_into(file: &mut File, slots: usize, id: Id) -> Result<bool> {
        let mut slot = Self::slot(id, slots);
        for _ in 0..slots {
            let offset = slot as u64 * MARK_SLOT_BYTES;
            let mut bytes = [0; MARK_SLOT_BYTES as usize];
            crate::io_util::read_at(file, &mut bytes, offset)?;
            if bytes[0] == 0 {
                bytes[0] = 1;
                bytes[1..].copy_from_slice(&id.0);
                file.seek(SeekFrom::Start(offset))?;
                file.write_all(&bytes)?;
                return Ok(true);
            }
            if bytes[1..] == id.0 {
                return Ok(false);
            }
            slot = (slot + 1) & (slots - 1);
        }
        Err(corrupt("mark table is full"))
    }
    fn insert_disk(&mut self, id: Id) -> Result<bool> {
        Self::insert_into(
            self.table
                .as_mut()
                .ok_or_else(|| corrupt("missing mark table"))?,
            self.table_slots,
            id,
        )
    }
    fn spill(&mut self) -> Result<()> {
        let slots = self.memory_limit.max(self.memory.len()).max(1) * 2;
        self.table_slots = slots.next_power_of_two();
        let mut table = tempfile::tempfile_in(self.dir.path())?;
        table.set_len(self.table_slots as u64 * MARK_SLOT_BYTES)?;
        for &id in &self.memory {
            Self::insert_into(&mut table, self.table_slots, id)?;
        }
        self.memory.clear();
        self.table = Some(table);
        Ok(())
    }
    fn grow(&mut self) -> Result<()> {
        let old_slots = self.table_slots;
        let slots = old_slots
            .checked_mul(2)
            .ok_or_else(|| corrupt("mark table size overflow"))?;
        let old = self
            .table
            .take()
            .ok_or_else(|| corrupt("missing mark table"))?;
        let mut table = tempfile::tempfile_in(self.dir.path())?;
        table.set_len(slots as u64 * MARK_SLOT_BYTES)?;
        for slot in 0..old_slots {
            let mut bytes = [0; MARK_SLOT_BYTES as usize];
            crate::io_util::read_at(&old, &mut bytes, slot as u64 * MARK_SLOT_BYTES)?;
            if bytes[0] != 0 {
                Self::insert_into(&mut table, slots, Id(bytes[1..].try_into().unwrap()))?;
            }
        }
        self.table_slots = slots;
        self.table = Some(table);
        Ok(())
    }
    pub fn visit<F: FnMut(Id) -> Result<()>>(&self, mut f: F) -> Result<()> {
        let Some(table) = &self.table else {
            for &id in &self.memory {
                f(id)?;
            }
            return Ok(());
        };
        for slot in 0..self.table_slots {
            let mut bytes = [0; MARK_SLOT_BYTES as usize];
            crate::io_util::read_at(table, &mut bytes, slot as u64 * MARK_SLOT_BYTES)?;
            if bytes[0] != 0 {
                f(Id(bytes[1..].try_into().unwrap()))?;
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
        Kind::TinyBlock | Kind::ChunkBlock | Kind::MetadataBlock => {
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
    walk_roots(
        s,
        roots,
        scratch,
        include_ancestors,
        false,
        MARK_BATCH,
        |_, _| Ok(()),
    )
}

/// walk each reachable logical object once, exposing already-loaded traversal batches to callers.
/// this keeps pack production from repeating the complete mark traversal and object read pass.
pub(crate) fn walk_roots<S, F>(
    s: &S,
    roots: &[Id],
    scratch: &Path,
    include_ancestors: bool,
    load_chunks: bool,
    batch_size: usize,
    mut visit: F,
) -> Result<DiskMarks>
where
    S: ReadStore + ?Sized,
    F: FnMut(&[Id], &[crate::store::Object]) -> Result<()>,
{
    let mut marks = DiskMarks::new(scratch)?;
    let mut stack = DiskStack::<33>::new(scratch)?;
    for &id in roots {
        stack.push(edge(id, Some(Kind::Revision)))?;
    }
    loop {
        let mut frontier = Vec::with_capacity(batch_size);
        while frontier.len() < batch_size {
            let Some(bytes) = stack.pop()? else { break };
            frontier.push(bytes);
        }
        if frontier.is_empty() {
            break;
        }
        let mut ids = Vec::with_capacity(frontier.len());
        let mut expected_kinds = Vec::with_capacity(frontier.len());
        for bytes in frontier {
            let id = Id(bytes[..32].try_into().unwrap());
            let expected = if bytes[32] == 0 {
                None
            } else {
                Some(Kind::try_from(bytes[32])?)
            };
            if marks.insert(id)? {
                if !load_chunks && expected == Some(Kind::Chunk) {
                    let info = s.info(id)?;
                    if info.kind != Kind::Chunk {
                        return Err(corrupt("object graph type mismatch"));
                    }
                    continue;
                }
                ids.push(id);
                expected_kinds.push(expected);
            }
        }
        let loaded = s.get_many(&ids)?;
        if loaded.len() != ids.len() {
            return Err(corrupt("bulk graph read returned incorrect object count"));
        }
        for (object, expected) in loaded.iter().zip(&expected_kinds) {
            if expected.is_some_and(|kind| kind != object.kind) {
                return Err(corrupt("object graph type mismatch"));
            }
        }
        let children = loaded
            .par_iter()
            .map(|object| {
                if object.kind == Kind::Chunk {
                    Ok(Vec::new())
                } else {
                    references(object.kind, &object.bytes, include_ancestors)
                }
            })
            .collect::<Vec<_>>()
            .into_iter()
            .collect::<Result<Vec<_>>>()?;
        visit(&ids, &loaded)?;
        for children in children {
            for (child, kind) in children {
                stack.push(edge(child, kind))?;
            }
        }
    }
    Ok(marks)
}
struct Counters {
    dir: TempDir,
    memory: HashMap<u64, u64>,
    memory_limit: usize,
    table: Option<File>,
    table_slots: usize,
    table_entries: usize,
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
            table: None,
            table_slots: 0,
            table_entries: 0,
        })
    }
    fn get(&self, n: u64) -> Result<u64> {
        let Some(table) = &self.table else {
            return Ok(self.memory.get(&n).copied().unwrap_or(0));
        };
        Ok(Self::find_slot(table, self.table_slots, n)?.1.unwrap_or(0))
    }
    fn increment(&mut self, n: u64) -> Result<()> {
        if self.table.is_none()
            && (self.memory.contains_key(&n) || self.memory.len() < self.memory_limit)
        {
            let value = self.memory.entry(n).or_default();
            *value = value
                .checked_add(1)
                .ok_or_else(|| corrupt("link count overflow"))?;
            return Ok(());
        }
        if self.table.is_none() {
            self.spill()?;
        }
        let (mut slot, current) = Self::find_slot(
            self.table
                .as_ref()
                .ok_or_else(|| corrupt("missing counter table"))?,
            self.table_slots,
            n,
        )?;
        if current.is_none()
            && self.table_entries.saturating_add(1).saturating_mul(10) >= self.table_slots * 7
        {
            self.grow()?;
            slot = Self::find_slot(
                self.table
                    .as_ref()
                    .ok_or_else(|| corrupt("missing counter table"))?,
                self.table_slots,
                n,
            )?
            .0;
        }
        let value = current
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| corrupt("link count overflow"))?;
        Self::write_slot(
            self.table
                .as_mut()
                .ok_or_else(|| corrupt("missing counter table"))?,
            slot,
            n,
            value,
        )?;
        if current.is_none() {
            self.table_entries += 1;
        }
        Ok(())
    }
    fn slot(n: u64, slots: usize) -> usize {
        n.wrapping_mul(0x9e37_79b9_7f4a_7c15) as usize & (slots - 1)
    }
    fn find_slot(file: &File, slots: usize, n: u64) -> Result<(usize, Option<u64>)> {
        let mut slot = Self::slot(n, slots);
        for _ in 0..slots {
            let mut bytes = [0; COUNTER_SLOT_BYTES as usize];
            crate::io_util::read_at(file, &mut bytes, slot as u64 * COUNTER_SLOT_BYTES)?;
            if bytes[0] == 0 {
                return Ok((slot, None));
            }
            if u64::from_le_bytes(bytes[1..9].try_into().unwrap()) == n {
                return Ok((
                    slot,
                    Some(u64::from_le_bytes(bytes[9..17].try_into().unwrap())),
                ));
            }
            slot = (slot + 1) & (slots - 1);
        }
        Err(corrupt("counter table is full"))
    }
    fn write_slot(file: &mut File, slot: usize, n: u64, value: u64) -> Result<()> {
        let mut bytes = [0; COUNTER_SLOT_BYTES as usize];
        bytes[0] = 1;
        bytes[1..9].copy_from_slice(&n.to_le_bytes());
        bytes[9..17].copy_from_slice(&value.to_le_bytes());
        file.seek(SeekFrom::Start(slot as u64 * COUNTER_SLOT_BYTES))?;
        file.write_all(&bytes)?;
        Ok(())
    }
    fn spill(&mut self) -> Result<()> {
        self.table_slots =
            (self.memory_limit.max(self.memory.len()).max(1) * 2).next_power_of_two();
        let mut table = tempfile::tempfile_in(self.dir.path())?;
        table.set_len(self.table_slots as u64 * COUNTER_SLOT_BYTES)?;
        for (&n, &value) in &self.memory {
            let (slot, _) = Self::find_slot(&table, self.table_slots, n)?;
            Self::write_slot(&mut table, slot, n, value)?;
        }
        self.table_entries = self.memory.len();
        self.memory.clear();
        self.table = Some(table);
        Ok(())
    }
    fn grow(&mut self) -> Result<()> {
        let old_slots = self.table_slots;
        let new_slots = old_slots
            .checked_mul(2)
            .ok_or_else(|| corrupt("counter table size overflow"))?;
        let old = self
            .table
            .take()
            .ok_or_else(|| corrupt("missing counter table"))?;
        let mut table = tempfile::tempfile_in(self.dir.path())?;
        table.set_len(new_slots as u64 * COUNTER_SLOT_BYTES)?;
        for slot in 0..old_slots {
            let mut bytes = [0; COUNTER_SLOT_BYTES as usize];
            crate::io_util::read_at(&old, &mut bytes, slot as u64 * COUNTER_SLOT_BYTES)?;
            if bytes[0] == 0 {
                continue;
            }
            let n = u64::from_le_bytes(bytes[1..9].try_into().unwrap());
            let value = u64::from_le_bytes(bytes[9..17].try_into().unwrap());
            let (slot, _) = Self::find_slot(&table, new_slots, n)?;
            Self::write_slot(&mut table, slot, n, value)?;
        }
        self.table_slots = new_slots;
        self.table = Some(table);
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
    let pages = ByteCache::new(8 * 1024 * 1024);
    walk_roots(
        s,
        roots,
        scratch,
        include_ancestors,
        false,
        MARK_BATCH,
        |_, objects| {
            for object in objects {
                match object.kind {
                    Kind::Workspace => {
                        verify_workspace(s, &Workspace::decode(&object.bytes)?, scratch)?
                    }
                    Kind::File => {
                        let file = FileObject::decode(&object.bytes)?;
                        if verified_info {
                            verify_file_from_verified_info(s, &file)?;
                        } else {
                            verify_file(s, &pages, &file)?;
                        }
                    }
                    Kind::Revision => {
                        let revision = Revision::decode(&object.bytes)?;
                        let workspace: Workspace = load(s, revision.workspace, Kind::Workspace)?;
                        let projection: PathProjection =
                            load(s, revision.projection, Kind::PathProjection)?;
                        projection::verify(s, &workspace, &projection)?;
                    }
                    _ => {}
                }
            }
            Ok(())
        },
    )
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
