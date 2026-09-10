//! Immutable reads independent of publication, transport, and retention policy.

use crate::{
    Id, Result,
    cache::ByteCache,
    error::invalid,
    model::{FileObject, Inode, InodeData, Kind, Revision, Workspace},
    namespace, pages,
    projection::{PathNode, PathProjection},
    radix,
    store::{ReadStore, load, load_many},
};

pub struct View<S> {
    pub store: S,
    pub revision: Id,
    pub workspace: Workspace,
    pub projection: PathProjection,
    pub attachments: Option<Id>,
    pages: ByteCache,
}

#[derive(Debug)]
pub struct Entry {
    pub name: String,
    pub inode_number: u64,
    pub inode_id: Id,
    pub inode: Inode,
    pub size: u64,
}

impl<S: ReadStore> View<S> {
    pub fn new(store: S, revision: Id) -> Result<Self> {
        let r: Revision = load(&store, revision, Kind::Revision)?;
        Ok(Self {
            workspace: load(&store, r.workspace, Kind::Workspace)?,
            projection: load(&store, r.projection, Kind::PathProjection)?,
            attachments: r.attachments,
            store,
            revision,
            pages: ByteCache::new(16 * 1024 * 1024),
        })
    }

    pub fn stat(&self, path: &str) -> Result<(u64, Inode)> {
        namespace::stat(&self.store, &self.workspace, path)
    }

    pub fn directory(&self, path: &str, after: Option<&str>, limit: usize) -> Result<Vec<Entry>> {
        let (_, inode) = self.stat(path)?;
        if !matches!(inode.data, InodeData::Directory(_)) {
            return Err(invalid("path is not a directory"));
        }
        let node = if path.is_empty() {
            self.projection.root
        } else {
            crate::projection::subtree(&self.store, &self.projection, path)?
        };
        let node: PathNode = load(&self.store, node, Kind::PathNode)?;
        let children = radix::page(&self.store, node.children, after.map(str::as_bytes), limit)?;
        let nodes: Vec<PathNode> = load_many(
            &self.store,
            &children.iter().map(|(_, id)| *id).collect::<Vec<_>>(),
            Kind::PathNode,
        )?;
        let inodes: Vec<Inode> = load_many(
            &self.store,
            &nodes.iter().map(|node| node.inode_id).collect::<Vec<_>>(),
            Kind::Inode,
        )?;
        let file_ids: Vec<_> = inodes
            .iter()
            .filter_map(|inode| match inode.data {
                InodeData::File(id) => Some(id),
                _ => None,
            })
            .collect();
        let files: Vec<FileObject> = load_many(&self.store, &file_ids, Kind::File)?;
        let sizes: std::collections::HashMap<_, _> = file_ids
            .into_iter()
            .zip(files.into_iter().map(|file| file.size))
            .collect();
        children
            .into_iter()
            .zip(nodes)
            .zip(inodes)
            .map(|(((name, _), node), inode)| {
                let size = match inode.data {
                    InodeData::File(id) => sizes[&id],
                    _ => 0,
                };
                Ok(Entry {
                    name: String::from_utf8(name).map_err(|_| invalid("invalid directory name"))?,
                    inode_number: node.inode_number,
                    inode_id: node.inode_id,
                    inode,
                    size,
                })
            })
            .collect()
    }

    pub fn read_range(&self, path: &str, offset: u64, length: usize) -> Result<Vec<u8>> {
        if length > 8 * 1024 * 1024 {
            return Err(invalid("range exceeds 8 MiB"));
        }
        pages::read_range(
            &self.store,
            &self.pages,
            namespace::file(&self.store, &self.workspace, path)?,
            offset,
            length,
        )
    }

    pub fn copy_to<W: std::io::Write>(&self, path: &str, output: W) -> Result<u64> {
        pages::copy_to(
            &self.store,
            &self.pages,
            namespace::file(&self.store, &self.workspace, path)?,
            output,
        )
    }

    /// Materialize holes by seeking, while preserving the file's full logical length.
    pub fn materialize_file(&self, path: &str, output: &mut std::fs::File) -> Result<()> {
        use std::io::{Seek, SeekFrom, Write};
        let id = namespace::file(&self.store, &self.workspace, path)?;
        let file: FileObject = load(&self.store, id, Kind::File)?;
        output.set_len(file.size)?;
        if file.small.is_some() {
            output.seek(SeekFrom::Start(0))?;
            pages::copy_to(&self.store, &self.pages, id, output)?;
            return Ok(());
        }
        radix::visit(&self.store, file.pages, &mut |key, page| {
            let number =
                u64::from_be_bytes(key.try_into().map_err(|_| invalid("invalid page key"))?);
            let offset = number
                .checked_mul(file.layout.page_size as u64)
                .ok_or_else(|| invalid("page offset overflow"))?;
            let length = file
                .size
                .saturating_sub(offset)
                .min(file.layout.page_size as u64) as usize;
            let bytes = pages::pin_page(&self.store, &self.pages, page, file.layout.page_size)?;
            for (index, bytes) in bytes[..length].chunks(65536).enumerate() {
                if bytes.iter().any(|byte| *byte != 0) {
                    output.seek(SeekFrom::Start(offset + index as u64 * 65536))?;
                    output.write_all(bytes)?;
                }
            }
            Ok(())
        })
    }
}
