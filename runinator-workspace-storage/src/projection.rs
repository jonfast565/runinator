//! Revision-local path Merkle projection with structural path references.
//!
//! A path reference is deliberately *not* a full pathname. It is the pair
//! `(parent_directory_inode, basename)`, persisted as an immutable CAS object.
//! Directory inodes have a second reverse index to the structural ref that
//! names the directory in its parent. Resolving a ref to a human pathname
//! walks directory-parent refs upward. Moving a directory therefore changes
//! only that directory's own parent ref; refs for all descendants remain valid.
use std::collections::BTreeMap;

use crate::{
    Id,
    codec::{Binary, Decoder, Encoder},
    error::{Result, corrupt, invalid},
    model::{Inode, InodeData, Kind, Link, Workspace},
    namespace, radix,
    store::{ReadStore, WriteStore, load, save},
};

#[derive(Clone, Debug)]
pub struct PathNode {
    pub inode_number: u64,
    pub inode_id: Id,
    pub children: Option<Id>,
}
impl PathNode {
    pub fn child<S: ReadStore + ?Sized>(&self, store: &S, name: &str) -> Result<Option<Id>> {
        radix::get(store, self.children, name.as_bytes())
    }
    pub fn entries<S: ReadStore + ?Sized>(&self, store: &S) -> Result<Vec<(String, Id)>> {
        let mut entries = Vec::new();
        radix::visit(store, self.children, &mut |name, id| {
            let name = std::str::from_utf8(name).map_err(|_| corrupt("invalid projected name"))?;
            validate_name(name)?;
            entries.push((name.to_owned(), id));
            Ok(())
        })?;
        Ok(entries)
    }
}
impl Binary for PathNode {
    fn encode(&self) -> Result<Vec<u8>> {
        if self.inode_number == 0 {
            return Err(invalid("invalid projection inode"));
        }
        let mut e = Encoder::new();
        e.u64(self.inode_number);
        e.id(self.inode_id);
        e.optional_id(self.children);
        e.finish()
    }
    fn decode(bytes: &[u8]) -> Result<Self> {
        let mut d = Decoder::new(bytes)?;
        let node = Self {
            inode_number: d.u64()?,
            inode_id: d.id()?,
            children: d.optional_id()?,
        };
        if node.inode_number == 0 {
            return Err(corrupt("invalid projection inode"));
        }
        d.finish()?;
        Ok(node)
    }
}

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

/// Canonically sorted set of structural PathRef object IDs.
#[derive(Clone, Debug)]
pub struct RefList {
    pub refs: Vec<Id>,
}

impl Binary for RefList {
    fn encode(&self) -> Result<Vec<u8>> {
        if self.refs.len() < 2 || self.refs.len() > 1_000_000 {
            return Err(invalid("invalid hard-link ref list"));
        }
        let mut e = Encoder::new();
        e.u32(self.refs.len() as u32);
        let mut previous: Option<Id> = None;
        for id in &self.refs {
            if previous.is_some_and(|p| p >= *id) {
                return Err(invalid("noncanonical hard-link ref list"));
            }
            e.id(*id);
            previous = Some(*id);
        }
        e.finish()
    }

    fn decode(bytes: &[u8]) -> Result<Self> {
        let mut d = Decoder::new(bytes)?;
        let count = d.u32()?;
        if !(2..=1_000_000).contains(&count) {
            return Err(corrupt("invalid hard-link ref list"));
        }
        let mut refs = Vec::with_capacity(count as usize);
        let mut previous: Option<Id> = None;
        for _ in 0..count {
            let id = d.id()?;
            if previous.is_some_and(|p| p >= id) {
                return Err(corrupt("noncanonical hard-link ref list"));
            }
            refs.push(id);
            previous = Some(id);
        }
        d.finish()?;
        Ok(Self { refs })
    }
}

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

fn validate_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 255
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || name.contains('\0')
    {
        return Err(invalid("invalid projected path component"));
    }
    Ok(())
}

fn inode_id<S: ReadStore + ?Sized>(store: &S, workspace: &Workspace, number: u64) -> Result<Id> {
    radix::get(store, workspace.inodes, &number.to_be_bytes())?
        .ok_or_else(|| corrupt(format!("dangling inode {number}")))
}

fn save_ref<S: WriteStore + ?Sized>(store: &S, parent_inode: u64, name: &str) -> Result<Id> {
    save(
        store,
        Kind::PathRef,
        &PathRef {
            parent_inode,
            name: name.to_owned(),
        },
    )
}

/// Derive the stable structural ref for a currently named path.
pub fn ref_for_path<S: WriteStore + ?Sized>(
    store: &S,
    workspace: &Workspace,
    path: &str,
) -> Result<Id> {
    let (parent_inode, name) = namespace::parent_ref(store, workspace, path)?;
    save_ref(store, parent_inode, &name)
}

fn build_node<S: WriteStore + ?Sized>(
    store: &S,
    workspace: &Workspace,
    number: u64,
    hardlinks: &mut BTreeMap<u64, Vec<Id>>,
    directory_refs: &mut Vec<(Vec<u8>, Id)>,
) -> Result<Id> {
    let object_id = inode_id(store, workspace, number)?;
    let inode: Inode = load(store, object_id, Kind::Inode)?;
    let mut child_entries = Vec::new();

    if let InodeData::Directory(root) = inode.data {
        let mut links = Vec::new();
        radix::visit(store, root, &mut |name, id| {
            let name =
                std::str::from_utf8(name).map_err(|_| corrupt("directory name is not UTF-8"))?;
            let link: Link = load(store, id, Kind::Link)?;
            links.push((name.to_owned(), link.0));
            Ok(())
        })?;
        links.sort_by(|a, b| a.0.cmp(&b.0));

        for (name, child_number) in links {
            let child_ref = save_ref(store, number, &name)?;
            let child_inode: Inode = load(
                store,
                inode_id(store, workspace, child_number)?,
                Kind::Inode,
            )?;
            if child_inode.links > 1 && matches!(child_inode.data, InodeData::File(_)) {
                hardlinks.entry(child_number).or_default().push(child_ref);
            }
            if matches!(child_inode.data, InodeData::Directory(_)) {
                directory_refs.push((child_number.to_be_bytes().to_vec(), child_ref));
            }
            let child = build_node(store, workspace, child_number, hardlinks, directory_refs)?;
            child_entries.push((name.into_bytes(), child));
        }
    }

    let children = radix::build_sorted(store, &child_entries)?;

    save(
        store,
        Kind::PathNode,
        &PathNode {
            inode_number: number,
            inode_id: object_id,
            children,
        },
    )
}

/// Build and persist the immutable path projection for migration/fsck tooling.
/// Normal transactions maintain this structure incrementally.
pub fn build<S: WriteStore + ?Sized>(store: &S, workspace: &Workspace) -> Result<Id> {
    let mut hardlink_map = BTreeMap::new();
    let mut directory_refs = Vec::new();
    let root = build_node(store, workspace, 1, &mut hardlink_map, &mut directory_refs)?;
    directory_refs.sort_by(|a, b| a.0.cmp(&b.0));
    let directory_refs = radix::build_sorted(store, &directory_refs)?;
    let mut hardlink_entries = Vec::new();
    for (number, refs) in hardlink_map {
        let refs = normalize_refs(refs);
        if refs.len() >= 2 {
            hardlink_entries.push((
                number.to_be_bytes().to_vec(),
                save(store, Kind::RefList, &RefList { refs })?,
            ));
        }
    }
    let hardlinks = radix::build_sorted(store, &hardlink_entries)?;
    save(
        store,
        Kind::PathProjection,
        &PathProjection {
            root,
            hardlinks,
            directory_refs,
        },
    )
}

pub fn hardlink_refs<S: ReadStore + ?Sized>(
    store: &S,
    projection: &PathProjection,
    inode: u64,
) -> Result<Vec<Id>> {
    let Some(id) = radix::get(store, projection.hardlinks, &inode.to_be_bytes())? else {
        return Ok(Vec::new());
    };
    Ok(load::<RefList, _>(store, id, Kind::RefList)?.refs)
}

fn directory_ref<S: ReadStore + ?Sized>(
    store: &S,
    projection: &PathProjection,
    inode: u64,
) -> Result<Option<Id>> {
    radix::get(store, projection.directory_refs, &inode.to_be_bytes())
}

/// Resolve a structural ref to a current pathname without scanning namespace
/// children. Complexity is O(directory depth).
pub fn resolve_ref<S: ReadStore + ?Sized>(
    store: &S,
    projection: &PathProjection,
    id: Id,
) -> Result<String> {
    let mut current: PathRef = load(store, id, Kind::PathRef)?;
    let mut names = vec![current.name];
    let mut parent = current.parent_inode;
    let mut depth = 0usize;
    while parent != 1 {
        depth += 1;
        if depth > 128 {
            return Err(corrupt("path ref parent chain too deep or cyclic"));
        }
        let parent_ref_id = directory_ref(store, projection, parent)?
            .ok_or_else(|| corrupt("directory parent ref missing"))?;
        current = load(store, parent_ref_id, Kind::PathRef)?;
        names.push(current.name);
        parent = current.parent_inode;
    }
    names.reverse();
    Ok(names.join("/"))
}

pub fn hardlink_paths<S: ReadStore + ?Sized>(
    store: &S,
    projection: &PathProjection,
    inode: u64,
) -> Result<Vec<String>> {
    let mut paths = Vec::new();
    for id in hardlink_refs(store, projection, inode)? {
        paths.push(resolve_ref(store, projection, id)?);
    }
    paths.sort();
    Ok(paths)
}

fn verify_node<S: ReadStore + ?Sized>(
    store: &S,
    workspace: &Workspace,
    id: Id,
    expected_inode: u64,
) -> Result<()> {
    let node: PathNode = load(store, id, Kind::PathNode)?;
    if node.inode_number != expected_inode {
        return Err(corrupt("path projection inode number mismatch"));
    }
    let actual = inode_id(store, workspace, expected_inode)?;
    if node.inode_id != actual {
        return Err(corrupt("path projection inode object mismatch"));
    }
    let inode: Inode = load(store, actual, Kind::Inode)?;
    match inode.data {
        InodeData::Directory(root) => {
            let mut actual_children = BTreeMap::new();
            radix::visit(store, root, &mut |name, id| {
                let name = std::str::from_utf8(name)
                    .map_err(|_| corrupt("directory name is not UTF-8"))?;
                let link: Link = load(store, id, Kind::Link)?;
                actual_children.insert(name.to_owned(), link.0);
                Ok(())
            })?;
            if actual_children.len() != node.entries(store)?.len() {
                return Err(corrupt("path projection child count mismatch"));
            }
            for (name, child_inode) in actual_children {
                let child = node
                    .child(store, &name)?
                    .ok_or_else(|| corrupt("path projection child missing"))?;
                verify_node(store, workspace, child, child_inode)?;
            }
        }
        _ if node.children.is_some() => {
            return Err(corrupt("non-directory projection node has children"));
        }
        _ => {}
    }
    Ok(())
}

fn verify_directory_refs<S: ReadStore + ?Sized>(
    store: &S,
    projection: &PathProjection,
    node_id: Id,
) -> Result<()> {
    let node: PathNode = load(store, node_id, Kind::PathNode)?;
    for (name, child_id) in node.entries(store)? {
        let child: PathNode = load(store, child_id, Kind::PathNode)?;
        let child_inode: Inode = load(store, child.inode_id, Kind::Inode)?;
        if !matches!(child_inode.data, InodeData::Directory(_)) {
            continue;
        }

        let reference_id = directory_ref(store, projection, child.inode_number)?
            .ok_or_else(|| corrupt("projected directory is missing parent ref"))?;
        let reference: PathRef = load(store, reference_id, Kind::PathRef)?;
        if reference.parent_inode != node.inode_number || reference.name != name {
            return Err(corrupt("projected directory parent ref mismatch"));
        }
        verify_directory_refs(store, projection, child_id)?;
    }
    Ok(())
}

pub fn verify<S: ReadStore + ?Sized>(
    store: &S,
    workspace: &Workspace,
    projection: &PathProjection,
) -> Result<()> {
    verify_node(store, workspace, projection.root, 1)?;
    verify_directory_refs(store, projection, projection.root)?;

    // Every indexed directory ref must resolve to that directory inode.
    radix::visit(store, projection.directory_refs, &mut |key, id| {
        let number = u64::from_be_bytes(
            key.try_into()
                .map_err(|_| corrupt("bad directory-ref key"))?,
        );
        if number == 1 {
            return Err(corrupt("root directory must not have a parent ref"));
        }
        let path = resolve_ref(store, projection, id)?;
        if namespace::resolve(store, workspace, &path)? != number {
            return Err(corrupt("directory structural ref mismatch"));
        }
        if !matches!(
            namespace::inode(store, workspace, number)?.data,
            InodeData::Directory(_)
        ) {
            return Err(corrupt("directory-ref index points to non-directory"));
        }
        Ok(())
    })?;

    radix::visit(store, projection.hardlinks, &mut |key, id| {
        let number = u64::from_be_bytes(
            key.try_into()
                .map_err(|_| corrupt("bad projected hard-link key"))?,
        );
        let refs: RefList = load(store, id, Kind::RefList)?;
        let inode: Inode = load(store, inode_id(store, workspace, number)?, Kind::Inode)?;
        if inode.links != refs.refs.len() as u64
            || inode.links < 2
            || !matches!(inode.data, InodeData::File(_))
        {
            return Err(corrupt("projected hard-link count/type mismatch"));
        }
        for reference in refs.refs {
            let path = resolve_ref(store, projection, reference)?;
            if namespace::resolve(store, workspace, &path)? != number {
                return Err(corrupt("projected hard-link ref mismatch"));
            }
        }
        Ok(())
    })
}

// --- Incremental transaction maintenance ---------------------------------

fn path_parts(path: &str) -> Result<Vec<&str>> {
    if path.is_empty() || path.starts_with('/') || path.ends_with('/') {
        return Err(invalid("projection path must be a non-empty relative path"));
    }
    let mut out = Vec::new();
    for part in path.split('/') {
        validate_name(part)?;
        out.push(part);
    }
    Ok(out)
}

fn projected_node_at<S: ReadStore + ?Sized>(store: &S, root: Id, parts: &[&str]) -> Result<Id> {
    let mut id = root;
    for part in parts {
        let node: PathNode = load(store, id, Kind::PathNode)?;
        id = node
            .child(store, part)?
            .ok_or_else(|| corrupt("projection path missing"))?;
    }
    Ok(id)
}

fn rewrite_at<S, F>(store: &S, node_id: Id, parts: &[&str], f: &mut F) -> Result<Id>
where
    S: WriteStore + ?Sized,
    F: FnMut(PathNode) -> Result<PathNode>,
{
    let mut node: PathNode = load(store, node_id, Kind::PathNode)?;
    if parts.is_empty() {
        return save(store, Kind::PathNode, &f(node)?);
    }
    let name = parts[0];
    let child = node
        .child(store, name)?
        .ok_or_else(|| corrupt("projection path missing"))?;
    let next = rewrite_at(store, child, &parts[1..], f)?;
    if next != child {
        node.children = radix::set(store, node.children, name.as_bytes(), Some(next))?;
    }
    save(store, Kind::PathNode, &node)
}

fn rewrite_parent<S, F>(
    store: &S,
    workspace: &Workspace,
    root: Id,
    parent_parts: &[&str],
    f: &mut F,
) -> Result<Id>
where
    S: WriteStore + ?Sized,
    F: FnMut(&mut Option<Id>) -> Result<()>,
{
    rewrite_at(store, root, parent_parts, &mut |mut node| {
        node.inode_id = inode_id(store, workspace, node.inode_number)?;
        f(&mut node.children)?;
        Ok(node)
    })
}

fn shallow_node<S: WriteStore + ?Sized>(
    store: &S,
    workspace: &Workspace,
    number: u64,
) -> Result<Id> {
    let object_id = inode_id(store, workspace, number)?;
    let inode: Inode = load(store, object_id, Kind::Inode)?;
    let children = match inode.data {
        InodeData::Directory(None) => None,
        InodeData::Directory(Some(_)) => {
            return Err(invalid(
                "incremental insertion of a non-empty directory requires an existing projected subtree",
            ));
        }
        _ => None,
    };
    save(
        store,
        Kind::PathNode,
        &PathNode {
            inode_number: number,
            inode_id: object_id,
            children,
        },
    )
}

pub fn subtree<S: ReadStore + ?Sized>(
    store: &S,
    projection: &PathProjection,
    path: &str,
) -> Result<Id> {
    projected_node_at(store, projection.root, &path_parts(path)?)
}

fn update_directory_ref<S: WriteStore + ?Sized>(
    store: &S,
    projection: &mut PathProjection,
    inode: u64,
    value: Option<Id>,
) -> Result<()> {
    projection.directory_refs = radix::set(
        store,
        projection.directory_refs,
        &inode.to_be_bytes(),
        value,
    )?;
    Ok(())
}

pub fn insert_new<S: WriteStore + ?Sized>(
    store: &S,
    workspace: &Workspace,
    projection: &mut PathProjection,
    path: &str,
    inode: u64,
) -> Result<()> {
    let parts = path_parts(path)?;
    let (name, parent) = parts
        .split_last()
        .ok_or_else(|| invalid("cannot insert projection root"))?;
    let child = shallow_node(store, workspace, inode)?;
    projection.root = rewrite_parent(store, workspace, projection.root, parent, &mut |children| {
        if radix::get(store, *children, name.as_bytes())?.is_some() {
            return Err(corrupt("projection child already existed"));
        }
        *children = radix::set(store, *children, name.as_bytes(), Some(child))?;
        Ok(())
    })?;
    let current: Inode = load(store, inode_id(store, workspace, inode)?, Kind::Inode)?;
    if matches!(current.data, InodeData::Directory(_)) {
        let reference = ref_for_path(store, workspace, path)?;
        update_directory_ref(store, projection, inode, Some(reference))?;
    }
    Ok(())
}

/// Insert a pre-existing projected subtree during rename. Descendant refs are
/// untouched. If the subtree root is a directory, only its own parent ref is
/// changed to reflect the new directory entry.
pub fn insert_subtree<S: WriteStore + ?Sized>(
    store: &S,
    workspace: &Workspace,
    projection: &mut PathProjection,
    path: &str,
    child: Id,
) -> Result<()> {
    let parts = path_parts(path)?;
    let (name, parent) = parts
        .split_last()
        .ok_or_else(|| invalid("cannot insert projection root"))?;
    projection.root = rewrite_parent(store, workspace, projection.root, parent, &mut |children| {
        if radix::get(store, *children, name.as_bytes())?.is_some() {
            return Err(corrupt("projection child already existed"));
        }
        *children = radix::set(store, *children, name.as_bytes(), Some(child))?;
        Ok(())
    })?;
    let node: PathNode = load(store, child, Kind::PathNode)?;
    let inode: Inode = load(store, node.inode_id, Kind::Inode)?;
    if matches!(inode.data, InodeData::Directory(_)) {
        let reference = ref_for_path(store, workspace, path)?;
        update_directory_ref(store, projection, node.inode_number, Some(reference))?;
    }
    Ok(())
}

pub fn remove<S: WriteStore + ?Sized>(
    store: &S,
    workspace: &Workspace,
    projection: &mut PathProjection,
    path: &str,
) -> Result<Id> {
    let parts = path_parts(path)?;
    let (name, parent) = parts
        .split_last()
        .ok_or_else(|| invalid("cannot remove projection root"))?;
    let old_parent = projected_node_at(store, projection.root, parent)?;
    let old_node: PathNode = load(store, old_parent, Kind::PathNode)?;
    let removed = old_node
        .child(store, name)?
        .ok_or_else(|| corrupt("projection child missing"))?;
    projection.root = rewrite_parent(store, workspace, projection.root, parent, &mut |children| {
        *children = radix::set(store, *children, name.as_bytes(), None)?;
        Ok(())
    })?;
    let node: PathNode = load(store, removed, Kind::PathNode)?;
    let inode: Inode = load(store, node.inode_id, Kind::Inode)?;
    if matches!(inode.data, InodeData::Directory(_)) {
        update_directory_ref(store, projection, node.inode_number, None)?;
    }
    Ok(removed)
}

pub fn refresh_path<S: WriteStore + ?Sized>(
    store: &S,
    workspace: &Workspace,
    projection: &mut PathProjection,
    path: &str,
) -> Result<()> {
    let parts = if path.is_empty() {
        Vec::new()
    } else {
        path_parts(path)?
    };
    projection.root = rewrite_at(store, projection.root, &parts, &mut |mut node| {
        node.inode_id = inode_id(store, workspace, node.inode_number)?;
        Ok(node)
    })?;
    Ok(())
}

fn normalize_refs(mut refs: Vec<Id>) -> Vec<Id> {
    refs.sort();
    refs.dedup();
    refs
}

fn set_ref_list_root<S: WriteStore + ?Sized>(
    store: &S,
    root: &mut Option<Id>,
    inode: u64,
    refs: Vec<Id>,
) -> Result<()> {
    let refs = normalize_refs(refs);
    let value = if refs.len() >= 2 {
        Some(save(store, Kind::RefList, &RefList { refs })?)
    } else {
        None
    };
    *root = radix::set(store, *root, &inode.to_be_bytes(), value)?;
    Ok(())
}

pub fn set_hardlink_refs<S: WriteStore + ?Sized>(
    store: &S,
    projection: &mut PathProjection,
    inode: u64,
    refs: Vec<Id>,
) -> Result<()> {
    set_ref_list_root(store, &mut projection.hardlinks, inode, refs)
}

/// Resolve aliases only when projection-tree mutation needs their current path.
/// The reverse index itself remains structural and requires no rewrite on an
/// ancestor directory move.
pub fn refresh_inode<S: WriteStore + ?Sized>(
    store: &S,
    workspace: &Workspace,
    projection: &mut PathProjection,
    inode: u64,
    hint: &str,
) -> Result<()> {
    if hint.is_empty() {
        return refresh_path(store, workspace, projection, hint);
    }
    let current: Inode = load(store, inode_id(store, workspace, inode)?, Kind::Inode)?;
    let mut refs = if current.links > 1 {
        hardlink_refs(store, projection, inode)?
    } else {
        vec![ref_for_path(store, workspace, hint)?]
    };
    if current.links > 1 {
        let hinted = ref_for_path(store, workspace, hint)?;
        if !refs.contains(&hinted) {
            refs.push(hinted);
        }
    }
    refs = normalize_refs(refs);
    for reference in &refs {
        let path = resolve_ref(store, projection, *reference)?;
        refresh_path(store, workspace, projection, &path)?;
    }
    set_hardlink_refs(store, projection, inode, refs)
}

/// Replace one alias ref after a rename. Directory ancestor moves require no
/// calls here because descendant refs are parent-inode based and stay stable.
pub fn replace_hardlink_ref<S: WriteStore + ?Sized>(
    store: &S,
    projection: &mut PathProjection,
    inode: u64,
    old_ref: Id,
    new_ref: Id,
) -> Result<()> {
    let mut refs = hardlink_refs(store, projection, inode)?;
    if refs.is_empty() {
        return Ok(());
    }
    let Some(slot) = refs.iter_mut().find(|id| **id == old_ref) else {
        return Err(corrupt("hard-link projection missing renamed alias ref"));
    };
    *slot = new_ref;
    set_hardlink_refs(store, projection, inode, refs)
}
