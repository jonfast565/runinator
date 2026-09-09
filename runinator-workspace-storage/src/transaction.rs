//! Backend-neutral copy-on-write transactions. Finalization does not publish a ref.
use crate::{
    Error, Id,
    cache::ByteCache,
    error::{Result, invalid},
    model::{Inode, InodeData, Kind, Layout, Metadata, Revision, Workspace},
    namespace, pages, projection,
    store::{WriteStore, load, save},
};
use std::io::Read;
pub struct Edit<S> {
    pub store: S,
    pub expected: Option<Id>,
    pub workspace: Workspace,
    pub projection: projection::PathProjection,
    pub attachments: Option<Id>,
    pages: ByteCache,
    layout: Layout,
}
impl<S: WriteStore> Edit<S> {
    pub fn new(
        store: S,
        expected: Option<Id>,
        layout: Layout,
        page_cache_bytes: usize,
    ) -> Result<Self> {
        layout.validate()?;
        if page_cache_bytes < layout.page_size as usize {
            return Err(invalid("page cache must hold an 8 MiB page"));
        }
        let (workspace, projection, attachments) = if let Some(id) = expected {
            let r: Revision = load(&store, id, Kind::Revision)?;
            (
                load(&store, r.workspace, Kind::Workspace)?,
                load(&store, r.projection, Kind::PathProjection)?,
                r.attachments,
            )
        } else {
            let w = namespace::empty(&store)?;
            let projection = load(&store, projection::build(&store, &w)?, Kind::PathProjection)?;
            (w, projection, None)
        };
        Ok(Self {
            store,
            expected,
            workspace,
            projection,
            attachments,
            pages: ByteCache::new(page_cache_bytes),
            layout,
        })
    }
    pub fn finish(&self, message: &str, parent: Option<Id>) -> Result<Id> {
        let projection = save(&self.store, Kind::PathProjection, &self.projection)?;
        let workspace = save(&self.store, Kind::Workspace, &self.workspace)?;
        save(
            &self.store,
            Kind::Revision,
            &Revision {
                parent,
                workspace,
                projection,
                attachments: self.attachments,
                message: message.into(),
            },
        )
    }
}
impl<S: WriteStore> Edit<S> {
    /// Each API operation is atomic with respect to the transaction's namespace.
    /// An error discards its new roots; already staged immutable objects become garbage.
    fn edit<T, F>(&mut self, f: F) -> Result<T>
    where
        F: FnOnce(&S, &ByteCache, &mut Workspace, &mut projection::PathProjection) -> Result<T>,
    {
        let mut next = self.workspace.clone();
        let mut next_projection = self.projection.clone();
        let result = f(&self.store, &self.pages, &mut next, &mut next_projection)?;
        self.workspace = next;
        self.projection = next_projection;
        Ok(result)
    }
    pub fn base(&self) -> Option<Id> {
        self.expected
    }
    pub fn mkdir(&mut self, path: &str) -> Result<()> {
        self.edit(|s, _, w, p| {
            namespace::mkdir(s, w, path)?;
            let (n, _) = namespace::stat(s, w, path)?;
            projection::insert_new(s, w, p, path, n)
        })
    }
    pub fn mkdir_all(&mut self, path: &str) -> Result<()> {
        self.edit(|s, _, w, p| {
            let mut prefix = String::new();
            for part in path.split('/') {
                if part.is_empty() {
                    return Err(invalid("invalid empty path component"));
                }
                if !prefix.is_empty() {
                    prefix.push('/');
                }
                prefix.push_str(part);
                match namespace::stat(s, w, &prefix) {
                    Ok((_, i)) => {
                        if !matches!(i.data, InodeData::Directory(_)) {
                            return Err(invalid("path component is not a directory"));
                        }
                    }
                    Err(Error::NotFound(_)) => {
                        namespace::mkdir(s, w, &prefix)?;
                        let (n, _) = namespace::stat(s, w, &prefix)?;
                        projection::insert_new(s, w, p, &prefix, n)?;
                    }
                    Err(e) => return Err(e),
                }
            }
            Ok(())
        })
    }
    pub fn put<R: Read>(&mut self, path: &str, input: R) -> Result<Id> {
        let layout = self.layout;
        self.edit(|s, _, w, p| {
            let before = namespace::stat(s, w, path).ok().map(|(n, _)| n);
            let id = namespace::put_file(s, w, path, layout, input)?;
            let (n, _) = namespace::stat(s, w, path)?;
            if before.is_some() {
                projection::refresh_inode(s, w, p, n, path)?;
            } else {
                projection::insert_new(s, w, p, path, n)?;
            }
            Ok(id)
        })
    }
    /// Use 1/4/8 MiB pages selected before ingestion, checking the supplied size.
    /// The repository's CDC parameters are retained. Existing files are not
    /// silently resized/re-paged by write(), append(), or truncate().
    pub fn put_sized<R: Read>(&mut self, path: &str, input: R, expected_size: u64) -> Result<Id> {
        let mut layout = self.layout;
        layout.page_size = Layout::for_size(expected_size)
            .page_size
            .max(layout.max.next_power_of_two());
        layout.validate()?;
        if self.pages.capacity() < layout.page_size as usize {
            return Err(invalid(
                "page cache cannot hold the selected large-file page",
            ));
        }
        let read_limit = expected_size
            .checked_add(1)
            .ok_or_else(|| invalid("size hint exceeds supported range"))?;
        self.edit(|s, _, w, p| {
            let before = namespace::stat(s, w, path).ok().map(|(n, _)| n);
            // Take at most the hint plus one byte. A lying hint must not cause
            // an unbounded ingestion before the mismatch is noticed.
            let id = namespace::put_file(s, w, path, layout, input.take(read_limit))?;
            let file: crate::model::FileObject = load(s, id, Kind::File)?;
            if file.size != expected_size {
                return Err(invalid("input length differs from size hint"));
            }
            let (n, _) = namespace::stat(s, w, path)?;
            if before.is_some() {
                projection::refresh_inode(s, w, p, n, path)?;
            } else {
                projection::insert_new(s, w, p, path, n)?;
            }
            Ok(id)
        })
    }
    pub fn write(&mut self, path: &str, offset: u64, input: &[u8]) -> Result<Id> {
        self.edit(|s, c, w, p| {
            let (n, _) = namespace::stat(s, w, path)?;
            let id = namespace::write(s, c, w, path, offset, input)?;
            projection::refresh_inode(s, w, p, n, path)?;
            Ok(id)
        })
    }
    pub fn write_from<R: Read>(&mut self, path: &str, offset: u64, input: R) -> Result<u64> {
        self.edit(|s, c, w, p| {
            let (n, _) = namespace::stat(s, w, path)?;
            let file = namespace::file(s, w, path)?;
            let (id, count) = pages::write_from(s, c, file, offset, input)?;
            namespace::replace_file(s, w, path, id)?;
            projection::refresh_inode(s, w, p, n, path)?;
            Ok(count)
        })
    }
    pub fn append(&mut self, path: &str, input: &[u8]) -> Result<Id> {
        self.edit(|s, c, w, p| {
            let (n, _) = namespace::stat(s, w, path)?;
            let file = namespace::file(s, w, path)?;
            let id = pages::append(s, c, file, input)?;
            namespace::replace_file(s, w, path, id)?;
            projection::refresh_inode(s, w, p, n, path)?;
            Ok(id)
        })
    }
    pub fn truncate(&mut self, path: &str, size: u64) -> Result<Id> {
        self.edit(|s, c, w, p| {
            let (n, _) = namespace::stat(s, w, path)?;
            let id = pages::truncate(s, c, namespace::file(s, w, path)?, size)?;
            namespace::replace_file(s, w, path, id)?;
            projection::refresh_inode(s, w, p, n, path)?;
            Ok(id)
        })
    }
    pub fn punch_hole(&mut self, path: &str, offset: u64, len: u64) -> Result<Id> {
        self.edit(|s, c, w, p| {
            let (n, _) = namespace::stat(s, w, path)?;
            let id = pages::punch_hole(s, c, namespace::file(s, w, path)?, offset, len)?;
            namespace::replace_file(s, w, path, id)?;
            projection::refresh_inode(s, w, p, n, path)?;
            Ok(id)
        })
    }
    pub fn clone_range(
        &mut self,
        source: &str,
        source_offset: u64,
        dest: &str,
        dest_offset: u64,
        len: u64,
    ) -> Result<Id> {
        self.edit(|s, c, w, p| {
            let (n, _) = namespace::stat(s, w, dest)?;
            let src = namespace::file(s, w, source)?;
            let dst = namespace::file(s, w, dest)?;
            let id = pages::clone_range(s, c, src, source_offset, dst, dest_offset, len)?;
            namespace::replace_file(s, w, dest, id)?;
            projection::refresh_inode(s, w, p, n, dest)?;
            Ok(id)
        })
    }
    pub fn hard_link(&mut self, source: &str, dest: &str) -> Result<()> {
        self.edit(|s, _, w, p| {
            let (n, _) = namespace::stat(s, w, source)?;
            let mut aliases = projection::hardlink_refs(s, p, n)?;
            if aliases.is_empty() {
                aliases.push(projection::ref_for_path(s, w, source)?);
            }
            namespace::hard_link(s, w, source, dest)?;
            projection::insert_new(s, w, p, dest, n)?;
            aliases.push(projection::ref_for_path(s, w, dest)?);
            projection::set_hardlink_refs(s, p, n, aliases)?;
            projection::refresh_inode(s, w, p, n, dest)
        })
    }
    pub fn symlink(&mut self, path: &str, target: &str) -> Result<()> {
        self.edit(|s, _, w, p| {
            namespace::symlink(s, w, path, target)?;
            let (n, _) = namespace::stat(s, w, path)?;
            projection::insert_new(s, w, p, path, n)
        })
    }
    pub fn rename(&mut self, source: &str, dest: &str, replace: bool) -> Result<()> {
        self.edit(|s, _, w, p| {
            if source == dest {
                return Ok(());
            }
            let (source_inode, source_meta) = namespace::stat(s, w, source)?;
            if let Ok((dest_inode, _)) = namespace::stat(s, w, dest)
                && dest_inode == source_inode
            {
                return Ok(());
            }
            let source_subtree = projection::subtree(s, p, source)?;
            let source_ref = projection::ref_for_path(s, w, source)?;
            let destination = match namespace::stat(s, w, dest) {
                Ok((n, _)) => Some((
                    n,
                    projection::ref_for_path(s, w, dest)?,
                    projection::hardlink_refs(s, p, n)?,
                )),
                Err(Error::NotFound(_)) => None,
                Err(e) => return Err(e),
            };
            namespace::rename(s, w, source, dest, replace)?;
            if destination.is_some() {
                let _ = projection::remove(s, w, p, dest)?;
            }
            let _ = projection::remove(s, w, p, source)?;
            projection::insert_subtree(s, w, p, dest, source_subtree)?;

            // A directory move changes only the moved directory's parent ref;
            // descendant refs remain stable because they name parent inodes.
            if source_meta.links > 1 && matches!(source_meta.data, InodeData::File(_)) {
                let new_ref = projection::ref_for_path(s, w, dest)?;
                projection::replace_hardlink_ref(s, p, source_inode, source_ref, new_ref)?;
            }

            if let Some((old_inode, old_ref, mut aliases)) = destination {
                aliases.retain(|id| *id != old_ref);
                match namespace::inode(s, w, old_inode) {
                    Ok(current) if current.links > 0 => {
                        for reference in &aliases {
                            let path = projection::resolve_ref(s, p, *reference)?;
                            projection::refresh_path(s, w, p, &path)?;
                        }
                        projection::set_hardlink_refs(s, p, old_inode, aliases)?;
                    }
                    _ => projection::set_hardlink_refs(s, p, old_inode, Vec::new())?,
                }
            }
            Ok(())
        })
    }
    pub fn unlink(&mut self, path: &str) -> Result<()> {
        self.edit(|s, _, w, p| {
            let (n, i) = namespace::stat(s, w, path)?;
            let removed_ref = projection::ref_for_path(s, w, path)?;
            let mut aliases = projection::hardlink_refs(s, p, n)?;
            namespace::unlink(s, w, path)?;
            let _ = projection::remove(s, w, p, path)?;
            aliases.retain(|id| *id != removed_ref);
            if i.links > 1 {
                for reference in &aliases {
                    let alias = projection::resolve_ref(s, p, *reference)?;
                    projection::refresh_path(s, w, p, &alias)?;
                }
                projection::set_hardlink_refs(s, p, n, aliases)?;
            } else {
                projection::set_hardlink_refs(s, p, n, Vec::new())?;
            }
            Ok(())
        })
    }
    pub fn set_metadata(&mut self, path: &str, metadata: Metadata) -> Result<()> {
        self.edit(|s, _, w, p| {
            let (n, _) = namespace::stat(s, w, path)?;
            namespace::set_metadata(s, w, path, metadata)?;
            projection::refresh_inode(s, w, p, n, path)
        })
    }
    pub fn set_xattr(&mut self, path: &str, name: &str, value: Option<&[u8]>) -> Result<()> {
        self.edit(|s, _, w, p| {
            let (n, mut i) = namespace::stat(s, w, path)?;
            match value {
                Some(bytes) => {
                    i.metadata.xattrs.insert(name.into(), bytes.to_vec());
                }
                None => {
                    i.metadata.xattrs.remove(name);
                }
            }
            namespace::set_metadata(s, w, path, i.metadata)?;
            projection::refresh_inode(s, w, p, n, path)
        })
    }
    pub fn stat(&self, path: &str) -> Result<(u64, Inode)> {
        namespace::stat(&self.store, &self.workspace, path)
    }
    pub fn list_names(&self, path: &str) -> Result<Vec<String>> {
        let mut names = Vec::new();
        namespace::list(&self.store, &self.workspace, path, &mut |name, _| {
            names.push(name.to_owned());
            Ok(())
        })?;
        names.sort();
        Ok(names)
    }
    pub fn read_range(&self, path: &str, offset: u64, len: usize) -> Result<Vec<u8>> {
        pages::read_range(
            &self.store,
            &self.pages,
            namespace::file(&self.store, &self.workspace, path)?,
            offset,
            len,
        )
    }
}
