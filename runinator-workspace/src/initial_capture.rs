//! one-pass construction for workspaces without an existing namespace.

use crate::{
    revision::{file_identity, save_results, stored_metadata},
    storage::{
        self, Layout,
        model::{FileObject, Inode, InodeData, Kind, Link, Workspace},
        pages, radix,
        store::{WriteStore, load, save},
        transaction::Edit,
    },
};
use runinator_models::{
    errors::{SendableError, WORKSPACE_INVALID},
    value::Value,
    workspaces::{WorkspaceLimits, WorkspaceUsage},
};
use std::{collections::BTreeMap, fs, io::Read, path::Path};

struct InitialCapture<'a, S> {
    store: &'a S,
    limits: WorkspaceLimits,
    usage: WorkspaceUsage,
    next_inode: u64,
    inodes: BTreeMap<u64, Inode>,
    hardlinks: BTreeMap<(u64, u64), u64>,
}

impl<S: WriteStore> InitialCapture<'_, S> {
    fn allocate(&mut self) -> Result<u64, SendableError> {
        let number = self.next_inode;
        self.next_inode = number
            .checked_add(1)
            .ok_or_else(|| WORKSPACE_INVALID.error("inode number exhausted"))?;
        Ok(number)
    }

    fn build(
        &mut self,
        root: &Path,
        relative: &str,
        number: u64,
        metadata: fs::Metadata,
    ) -> Result<(), SendableError> {
        if metadata.is_dir() {
            let mut entries = fs::read_dir(root.join(relative))?.collect::<Result<Vec<_>, _>>()?;
            entries.sort_by_key(|entry| entry.file_name());
            let mut children = Vec::with_capacity(entries.len());
            for entry in entries {
                self.usage.entries = self
                    .usage
                    .entries
                    .checked_add(1)
                    .ok_or_else(|| WORKSPACE_INVALID.error("entry count overflow"))?;
                self.limits.check(self.usage)?;
                let name = entry
                    .file_name()
                    .into_string()
                    .map_err(|_| WORKSPACE_INVALID.error("workspace names must be UTF-8"))?;
                let path = if relative.is_empty() {
                    name.clone()
                } else {
                    format!("{relative}/{name}")
                };
                crate::validate_path(Path::new(&path))?;
                let child_metadata = fs::symlink_metadata(entry.path())?;
                let child = if child_metadata.is_file()
                    && let Some(existing) = file_identity(&child_metadata)
                        .and_then(|identity| self.hardlinks.get(&identity).copied())
                {
                    let inode = self
                        .inodes
                        .get_mut(&existing)
                        .ok_or_else(|| WORKSPACE_INVALID.error("hard link inode is missing"))?;
                    inode.links = inode
                        .links
                        .checked_add(1)
                        .ok_or_else(|| WORKSPACE_INVALID.error("link count overflow"))?;
                    inode.metadata = stored_metadata(&child_metadata);
                    existing
                } else {
                    let child = self.allocate()?;
                    self.build(root, &path, child, child_metadata)?;
                    child
                };
                children.push((
                    name.into_bytes(),
                    save(self.store, Kind::Link, &Link(child))?,
                ));
            }
            let directory = radix::build_sorted(self.store, &children)?;
            self.inodes.insert(
                number,
                Inode {
                    links: 1,
                    metadata: stored_metadata(&metadata),
                    data: InodeData::Directory(directory),
                },
            );
            return Ok(());
        }
        if metadata.file_type().is_symlink() {
            let target = fs::read_link(root.join(relative))?;
            crate::validate_link(Path::new(relative), &target)?;
            let target = target
                .to_str()
                .ok_or_else(|| WORKSPACE_INVALID.error("workspace link targets must be UTF-8"))?;
            self.inodes.insert(
                number,
                Inode {
                    links: 1,
                    metadata: stored_metadata(&metadata),
                    data: InodeData::Symlink(target.into()),
                },
            );
            return Ok(());
        }
        if !metadata.is_file() {
            return Err(WORKSPACE_INVALID.error("workspace contains a special file"));
        }
        self.usage.logical_bytes = self
            .usage
            .logical_bytes
            .checked_add(metadata.len())
            .ok_or_else(|| WORKSPACE_INVALID.error("workspace size overflow"))?;
        self.limits.check(self.usage)?;
        let file = fs::File::open(root.join(relative))?;
        let opened = file.metadata()?;
        let identity = file_identity(&opened);
        if !opened.is_file()
            || opened.len() != metadata.len()
            || identity != file_identity(&metadata)
        {
            return Err(WORKSPACE_INVALID.error("workspace changed during capture"));
        }
        let mut layout = Layout::default();
        layout.page_size = Layout::for_size(metadata.len())
            .page_size
            .max(layout.max.next_power_of_two());
        let file_id = pages::ingest(self.store, layout, (&file).take(metadata.len() + 1))?;
        let object: FileObject = load(self.store, file_id, Kind::File)?;
        let final_metadata = file.metadata()?;
        if object.size != metadata.len()
            || final_metadata.modified()? != opened.modified()?
            || final_metadata.len() != opened.len()
        {
            return Err(WORKSPACE_INVALID.error("workspace changed during capture"));
        }
        self.inodes.insert(
            number,
            Inode {
                links: 1,
                metadata: stored_metadata(&metadata),
                data: InodeData::File(file_id),
            },
        );
        if let Some(identity) = identity {
            self.hardlinks.insert(identity, number);
        }
        Ok(())
    }
}

pub(crate) fn capture<S: WriteStore>(
    store: S,
    root: &Path,
    results: &BTreeMap<String, Value>,
    limits: WorkspaceLimits,
    scratch: &Path,
) -> Result<(Edit<S>, WorkspaceUsage), SendableError> {
    let mut capture = InitialCapture {
        store: &store,
        limits,
        usage: WorkspaceUsage::default(),
        next_inode: 2,
        inodes: BTreeMap::new(),
        hardlinks: BTreeMap::new(),
    };
    capture.build(root, "", 1, fs::metadata(root)?)?;
    let inodes = capture
        .inodes
        .iter()
        .map(|(number, inode)| {
            Ok((
                number.to_be_bytes().to_vec(),
                save(&store, Kind::Inode, inode)?,
            ))
        })
        .collect::<storage::Result<Vec<_>>>()?;
    let workspace = Workspace {
        inodes: radix::build_sorted(&store, &inodes)?,
        next_inode: capture.next_inode,
    };
    let mut usage = capture.usage;
    let mut edit =
        Edit::from_workspace(store, workspace, None, Layout::default(), 16 * 1024 * 1024)?;
    let results_bytes = save_results(&mut edit, results, limits, scratch)?;
    usage.results_bytes = results_bytes;
    usage.logical_bytes = usage
        .logical_bytes
        .checked_add(results_bytes)
        .ok_or_else(|| WORKSPACE_INVALID.error("workspace size overflow"))?;
    limits.check(usage)?;
    Ok((edit, usage))
}
