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
    visit: F,
) -> Result<DiskMarks>
where
    S: ReadStore + ?Sized,
    F: FnMut(&[Id], &[crate::store::Object]) -> Result<()>,
{
    walk_roots_filtered(
        s,
        roots,
        scratch,
        WalkOptions {
            include_ancestors,
            load_chunks,
            batch_size,
        },
        |_| Ok(true),
        visit,
    )
}

/// Walk reachable objects accepted by `select`, pruning rejected subtrees before loading them.
pub(crate) fn walk_roots_filtered<S, P, F>(
    s: &S,
    roots: &[Id],
    scratch: &Path,
    options: WalkOptions,
    mut select: P,
    mut visit: F,
) -> Result<DiskMarks>
where
    S: ReadStore + ?Sized,
    P: FnMut(Id) -> Result<bool>,
    F: FnMut(&[Id], &[crate::store::Object]) -> Result<()>,
{
    let mut marks = DiskMarks::new(scratch)?;
    let mut stack = DiskStack::<33>::new(scratch)?;
    for &id in roots {
        stack.push(edge(id, Some(Kind::Revision)))?;
    }
    loop {
        let mut frontier = Vec::with_capacity(options.batch_size);
        while frontier.len() < options.batch_size {
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
                if !select(id)? {
                    continue;
                }
                if !options.load_chunks && expected == Some(Kind::Chunk) {
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
                    references(object.kind, &object.bytes, options.include_ancestors)
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

#[cfg(test)]
#[path = "gc_tests.rs"]
mod gc_tests;

mod disk_marks;
pub use disk_marks::DiskMarks;

mod disk_stack;
use disk_stack::DiskStack;

mod walk_options;
pub(crate) use walk_options::WalkOptions;

mod counters;
use counters::Counters;

mod gc_report;
pub use gc_report::GcReport;
