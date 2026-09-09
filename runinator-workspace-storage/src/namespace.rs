//! A directory is a persistent radix map of name -> Link(inode number).
//! A workspace has a second persistent map of inode number -> Inode object.
//! Updating an inode changes every hard link in the new revision, not old ones.
use crate::{
    Error, Id,
    cache::ByteCache,
    error::{Result, corrupt, invalid},
    model::{FileObject, Inode, InodeData, Kind, Layout, Link, Metadata, Workspace},
    pages, radix,
    store::{ReadStore, WriteStore, load, save},
};
use std::io::Read;
pub fn components(path: &str) -> Result<Vec<&str>> {
    if path.len() > 4096 || path.contains('\0') || path.contains('\\') {
        return Err(invalid("invalid workspace path"));
    }
    let path = path.strip_prefix('/').unwrap_or(path);
    if path.is_empty() {
        return Ok(Vec::new());
    }
    let parts: Vec<_> = path.split('/').collect();
    if parts.len() > 128
        || parts
            .iter()
            .any(|p| p.is_empty() || *p == "." || *p == ".." || p.len() > 255)
    {
        return Err(invalid("invalid path component"));
    }
    Ok(parts)
}
pub fn inode<S: ReadStore + ?Sized>(s: &S, w: &Workspace, number: u64) -> Result<Inode> {
    let id = radix::get(s, w.inodes, &number.to_be_bytes())?
        .ok_or_else(|| corrupt(format!("dangling inode {number}")))?;
    load(s, id, Kind::Inode)
}
pub fn update_inode<S: WriteStore + ?Sized>(
    s: &S,
    w: &mut Workspace,
    number: u64,
    value: &Inode,
) -> Result<()> {
    let id = save(s, Kind::Inode, value)?;
    w.inodes = radix::set(s, w.inodes, &number.to_be_bytes(), Some(id))?;
    Ok(())
}
pub fn empty<S: WriteStore + ?Sized>(s: &S) -> Result<Workspace> {
    let mut w = Workspace {
        inodes: None,
        next_inode: 2,
    };
    update_inode(
        s,
        &mut w,
        1,
        &Inode {
            links: 1,
            metadata: Metadata {
                mode: 0o755,
                ..Default::default()
            },
            data: InodeData::Directory(None),
        },
    )?;
    Ok(w)
}
fn dir_root(i: &Inode) -> Result<Option<Id>> {
    match i.data {
        InodeData::Directory(root) => Ok(root),
        _ => Err(invalid(
            "not a directory (symlinks are not followed implicitly)",
        )),
    }
}
fn lookup_child<S: ReadStore + ?Sized>(
    s: &S,
    w: &Workspace,
    parent: u64,
    name: &str,
) -> Result<Option<u64>> {
    let dir = inode(s, w, parent)?;
    match radix::get(s, dir_root(&dir)?, name.as_bytes())? {
        None => Ok(None),
        Some(id) => Ok(Some(load::<Link, _>(s, id, Kind::Link)?.0)),
    }
}
fn resolve_parts<S: ReadStore + ?Sized>(s: &S, w: &Workspace, parts: &[&str]) -> Result<u64> {
    let mut number = 1;
    for part in parts {
        number =
            lookup_child(s, w, number, part)?.ok_or_else(|| Error::NotFound((*part).into()))?;
    }
    Ok(number)
}
pub fn resolve<S: ReadStore + ?Sized>(s: &S, w: &Workspace, path: &str) -> Result<u64> {
    resolve_parts(s, w, &components(path)?)
}
pub fn stat<S: ReadStore + ?Sized>(s: &S, w: &Workspace, path: &str) -> Result<(u64, Inode)> {
    let n = resolve(s, w, path)?;
    Ok((n, inode(s, w, n)?))
}
pub fn file<S: ReadStore + ?Sized>(s: &S, w: &Workspace, path: &str) -> Result<Id> {
    let (_, i) = stat(s, w, path)?;
    match i.data {
        InodeData::File(id) => Ok(id),
        _ => Err(invalid("not a regular file")),
    }
}
/// Resolve only the parent directory and basename of a workspace path.
/// This is used by the projection layer to form stable structural path refs.
pub fn parent_ref<S: ReadStore + ?Sized>(
    s: &S,
    w: &Workspace,
    path: &str,
) -> Result<(u64, String)> {
    let parts = components(path)?;
    let (name, parents) = parts
        .split_last()
        .ok_or_else(|| invalid("operation on root is not allowed"))?;
    let p = resolve_parts(s, w, parents)?;
    dir_root(&inode(s, w, p)?)?;
    Ok((p, (*name).to_owned()))
}
fn parent<'p, S: ReadStore + ?Sized>(
    s: &S,
    w: &Workspace,
    path: &'p str,
) -> Result<(u64, &'p str)> {
    let parts = components(path)?;
    let (name, parents) = parts
        .split_last()
        .ok_or_else(|| invalid("operation on root is not allowed"))?;
    let p = resolve_parts(s, w, parents)?;
    dir_root(&inode(s, w, p)?)?;
    Ok((p, name))
}
fn set_child<S: WriteStore + ?Sized>(
    s: &S,
    w: &mut Workspace,
    parent: u64,
    name: &str,
    child: Option<u64>,
) -> Result<()> {
    let mut dir = inode(s, w, parent)?;
    let root = dir_root(&dir)?;
    let value = match child {
        Some(n) => Some(save(s, Kind::Link, &Link(n))?),
        None => None,
    };
    dir.data = InodeData::Directory(radix::set(s, root, name.as_bytes(), value)?);
    update_inode(s, w, parent, &dir)
}
fn allocate<S: WriteStore + ?Sized>(
    s: &S,
    w: &mut Workspace,
    data: InodeData,
    metadata: Metadata,
) -> Result<u64> {
    let n = w.next_inode;
    w.next_inode = n
        .checked_add(1)
        .ok_or_else(|| invalid("inode number exhausted"))?;
    update_inode(
        s,
        w,
        n,
        &Inode {
            links: 1,
            metadata,
            data,
        },
    )?;
    Ok(n)
}
pub fn put_file<S: WriteStore + ?Sized, R: Read>(
    s: &S,
    w: &mut Workspace,
    path: &str,
    layout: Layout,
    source: R,
) -> Result<Id> {
    let (p, name) = parent(s, w, path)?;
    let existing = lookup_child(s, w, p, name)?;
    if let Some(n) = existing
        && !matches!(inode(s, w, n)?.data, InodeData::File(_))
    {
        return Err(invalid("cannot replace a non-file with file data"));
    }
    let id = pages::ingest(s, layout, source)?;
    match existing {
        Some(n) => {
            let mut i = inode(s, w, n)?;
            i.data = InodeData::File(id);
            update_inode(s, w, n, &i)?;
        }
        None => {
            let n = allocate(s, w, InodeData::File(id), Metadata::default())?;
            set_child(s, w, p, name, Some(n))?;
        }
    }
    Ok(id)
}
pub fn replace_file<S: WriteStore + ?Sized>(
    s: &S,
    w: &mut Workspace,
    path: &str,
    id: Id,
) -> Result<()> {
    let _: FileObject = load(s, id, Kind::File)?;
    let (n, mut i) = stat(s, w, path)?;
    if !matches!(i.data, InodeData::File(_)) {
        return Err(invalid("not a file"));
    }
    i.data = InodeData::File(id);
    update_inode(s, w, n, &i)
}
pub fn mkdir<S: WriteStore + ?Sized>(s: &S, w: &mut Workspace, path: &str) -> Result<()> {
    let (p, name) = parent(s, w, path)?;
    if lookup_child(s, w, p, name)?.is_some() {
        return Err(Error::Exists(path.into()));
    }
    let n = allocate(
        s,
        w,
        InodeData::Directory(None),
        Metadata {
            mode: 0o755,
            ..Default::default()
        },
    )?;
    set_child(s, w, p, name, Some(n))
}
pub fn mkdir_all<S: WriteStore + ?Sized>(s: &S, w: &mut Workspace, path: &str) -> Result<()> {
    let parts = components(path)?;
    let mut prefix = String::new();
    for part in parts {
        if !prefix.is_empty() {
            prefix.push('/');
        }
        prefix.push_str(part);
        match stat(s, w, &prefix) {
            Ok((_, i)) => {
                dir_root(&i)?;
            }
            Err(Error::NotFound(_)) => mkdir(s, w, &prefix)?,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
pub fn symlink<S: WriteStore + ?Sized>(
    s: &S,
    w: &mut Workspace,
    path: &str,
    target: &str,
) -> Result<()> {
    if target.is_empty() || target.len() > 4096 || target.contains('\0') || target.contains('\\') {
        return Err(invalid("invalid symlink target"));
    }
    let (p, name) = parent(s, w, path)?;
    if lookup_child(s, w, p, name)?.is_some() {
        return Err(Error::Exists(path.into()));
    }
    let n = allocate(
        s,
        w,
        InodeData::Symlink(target.into()),
        Metadata {
            mode: 0o777,
            ..Default::default()
        },
    )?;
    set_child(s, w, p, name, Some(n))
}
pub fn read_link<S: ReadStore + ?Sized>(s: &S, w: &Workspace, path: &str) -> Result<String> {
    match stat(s, w, path)?.1.data {
        InodeData::Symlink(t) => Ok(t),
        _ => Err(invalid("not a symlink")),
    }
}
/// Explicit symlink resolution, confined to the virtual root, with a loop bound.
pub fn resolve_follow<S: ReadStore + ?Sized>(s: &S, w: &Workspace, path: &str) -> Result<u64> {
    let mut pending: Vec<String> = components(path)?.into_iter().map(str::to_owned).collect();
    let mut follows = 0;
    loop {
        let mut prefix: Vec<String> = Vec::new();
        let mut number = 1;
        let mut replacement = None;
        for (index, name) in pending.iter().enumerate() {
            number =
                lookup_child(s, w, number, name)?.ok_or_else(|| Error::NotFound(name.clone()))?;
            let i = inode(s, w, number)?;
            if let InodeData::Symlink(target) = i.data {
                follows += 1;
                if follows > 40 {
                    return Err(invalid("too many symlink traversals"));
                }
                let mut next = if target.starts_with('/') {
                    Vec::new()
                } else {
                    prefix.clone()
                };
                for part in target.split('/') {
                    match part {
                        "" | "." => {}
                        ".." => {
                            if next.pop().is_none() {
                                return Err(invalid("symlink escapes virtual root"));
                            }
                        }
                        _ => {
                            if part.len() > 255 || part.contains('\\') || part.contains('\0') {
                                return Err(invalid("invalid symlink component"));
                            }
                            next.push(part.to_owned());
                        }
                    }
                }
                next.extend_from_slice(&pending[index + 1..]);
                components(&next.join("/"))?;
                replacement = Some(next);
                break;
            }
            prefix.push(name.clone());
        }
        match replacement {
            Some(next) => pending = next,
            None => return Ok(number),
        }
    }
}
pub fn hard_link<S: WriteStore + ?Sized>(
    s: &S,
    w: &mut Workspace,
    source: &str,
    dest: &str,
) -> Result<()> {
    let (n, mut i) = stat(s, w, source)?;
    if !matches!(i.data, InodeData::File(_)) {
        return Err(invalid("hard links are limited to regular files"));
    }
    let (p, name) = parent(s, w, dest)?;
    if lookup_child(s, w, p, name)?.is_some() {
        return Err(Error::Exists(dest.into()));
    }
    i.links = i
        .links
        .checked_add(1)
        .ok_or_else(|| invalid("link count overflow"))?;
    update_inode(s, w, n, &i)?;
    set_child(s, w, p, name, Some(n))
}
pub fn unlink<S: WriteStore + ?Sized>(s: &S, w: &mut Workspace, path: &str) -> Result<()> {
    let (p, name) = parent(s, w, path)?;
    let n = lookup_child(s, w, p, name)?.ok_or_else(|| Error::NotFound(path.into()))?;
    let mut i = inode(s, w, n)?;
    if let InodeData::Directory(Some(_)) = i.data {
        return Err(invalid("directory is not empty"));
    }
    set_child(s, w, p, name, None)?;
    if i.links == 1 {
        w.inodes = radix::set(s, w.inodes, &n.to_be_bytes(), None)?;
        return Ok(());
    }
    i.links -= 1;
    update_inode(s, w, n, &i)
}
pub fn rename<S: WriteStore + ?Sized>(
    s: &S,
    w: &mut Workspace,
    source: &str,
    dest: &str,
    replace: bool,
) -> Result<()> {
    let sp = components(source)?;
    let dp = components(dest)?;
    let (from, from_name) = parent(s, w, source)?;
    let n = lookup_child(s, w, from, from_name)?.ok_or_else(|| Error::NotFound(source.into()))?;
    let i = inode(s, w, n)?;
    let (to, to_name) = parent(s, w, dest)?;
    if sp == dp {
        return Ok(());
    }
    if matches!(i.data, InodeData::Directory(_)) && dp.starts_with(&sp) {
        return Err(invalid("cannot move a directory inside itself"));
    }
    if let Some(existing) = lookup_child(s, w, to, to_name)? {
        if existing == n {
            return Ok(());
        }
        if !replace {
            return Err(Error::Exists(dest.into()));
        }
        let other = inode(s, w, existing)?;
        if matches!(i.data, InodeData::Directory(_))
            != matches!(other.data, InodeData::Directory(_))
        {
            return Err(invalid("rename file/directory type mismatch"));
        }
        unlink(s, w, dest)?;
    }
    // Reload each parent after mutations; same-parent renames remain correct.
    set_child(s, w, from, from_name, None)?;
    set_child(s, w, to, to_name, Some(n))
}
pub fn set_metadata<S: WriteStore + ?Sized>(
    s: &S,
    w: &mut Workspace,
    path: &str,
    metadata: Metadata,
) -> Result<()> {
    let (n, mut i) = stat(s, w, path)?;
    i.metadata = metadata;
    update_inode(s, w, n, &i)
}
pub fn list<S: ReadStore + ?Sized, F: FnMut(&str, u64) -> Result<()>>(
    s: &S,
    w: &Workspace,
    path: &str,
    f: &mut F,
) -> Result<()> {
    let (_, i) = stat(s, w, path)?;
    let root = dir_root(&i)?;
    radix::visit(s, root, &mut |name, id| {
        let name = std::str::from_utf8(name).map_err(|_| corrupt("directory name is not UTF-8"))?;
        let link: Link = load(s, id, Kind::Link)?;
        f(name, link.0)
    })
}
/// All link names in this revision observe the new FileObject.
pub fn write<S: WriteStore + ?Sized>(
    s: &S,
    cache: &ByteCache,
    w: &mut Workspace,
    path: &str,
    offset: u64,
    bytes: &[u8],
) -> Result<Id> {
    let old = file(s, w, path)?;
    let id = pages::write_range(s, cache, old, offset, bytes)?;
    replace_file(s, w, path, id)?;
    Ok(id)
}
