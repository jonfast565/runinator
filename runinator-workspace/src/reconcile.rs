//! Reconcile a captured namespace while retaining unaffected inode identities.
use crate::storage::{self, model::InodeData, store::WriteStore, transaction::Edit};
use runinator_models::errors::SendableError;
use std::{collections::BTreeMap, fs, path::Path};

pub(crate) fn prepare<S: WriteStore>(edit: &mut Edit<S>, root: &Path) -> Result<(), SendableError> {
    let mut groups = BTreeMap::<u64, Vec<String>>::new();
    inventory(edit, "", &mut groups)?;
    let mut split = std::collections::BTreeSet::new();
    for aliases in groups.values().filter(|aliases| aliases.len() > 1) {
        let mut identity = None;
        for path in aliases {
            if let Ok(metadata) = fs::symlink_metadata(root.join(path)) {
                if !metadata.is_file() {
                    continue;
                }
                let next = super::revision::file_identity(&metadata);
                if next.is_none() || identity.is_some_and(|before| Some(before) != next) {
                    split.extend(aliases.iter().cloned());
                    break;
                }
                identity = next;
            }
        }
    }
    purge(edit, root, "", &split)
}
fn inventory<S: WriteStore>(
    edit: &Edit<S>,
    path: &str,
    groups: &mut BTreeMap<u64, Vec<String>>,
) -> Result<(), SendableError> {
    let (number, inode) = edit.stat(path)?;
    match inode.data {
        InodeData::File(_) => groups.entry(number).or_default().push(path.into()),
        InodeData::Directory(_) => {
            for name in edit.list_names(path)? {
                let child = if path.is_empty() {
                    name
                } else {
                    format!("{path}/{name}")
                };
                inventory(edit, &child, groups)?;
            }
        }
        _ => {}
    }
    Ok(())
}
fn purge<S: WriteStore>(
    edit: &mut Edit<S>,
    root: &Path,
    path: &str,
    split: &std::collections::BTreeSet<String>,
) -> Result<(), SendableError> {
    purge_inner(edit, root, path, split, false)
}
fn purge_inner<S: WriteStore>(
    edit: &mut Edit<S>,
    root: &Path,
    path: &str,
    split: &std::collections::BTreeSet<String>,
    removed_parent: bool,
) -> Result<(), SendableError> {
    let (_, inode) = edit.stat(path)?;
    let metadata = if removed_parent {
        None
    } else {
        match fs::symlink_metadata(root.join(path)) {
            Ok(metadata) => Some(metadata),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ) =>
            {
                None
            }
            Err(error) => return Err(error.into()),
        }
    };
    let keep = path.is_empty()
        || match (&inode.data, metadata) {
            (InodeData::Directory(_), Some(metadata)) => metadata.is_dir(),
            (InodeData::File(_), Some(metadata)) => metadata.is_file() && !split.contains(path),
            (InodeData::Symlink(target), Some(metadata)) => {
                metadata.file_type().is_symlink()
                    && fs::read_link(root.join(path))? == Path::new(target)
            }
            _ => false,
        };
    if let InodeData::Directory(_) = inode.data {
        for name in edit.list_names(path)? {
            let child = if path.is_empty() {
                name
            } else {
                format!("{path}/{name}")
            };
            purge_inner(edit, root, &child, split, !keep)?;
        }
    }
    if !keep {
        match edit.unlink(path) {
            Ok(()) => {}
            Err(storage::Error::NotFound(_)) => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}
