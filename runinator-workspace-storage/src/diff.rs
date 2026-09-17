//! Resumable depth-first revision diffs with Merkle subtree skips.
use crate::{
    Id, Result,
    error::invalid,
    model::{Inode, InodeData, Kind},
    projection::PathNode,
    radix,
    store::{ReadStore, load},
    view::View,
};
use serde::{Deserialize, Serialize};

fn node<S: ReadStore>(view: &View<S>, id: Option<Id>) -> Result<Option<PathNode>> {
    id.map(|id| load(&view.store, id, Kind::PathNode))
        .transpose()
}

pub fn page<L: ReadStore, R: ReadStore>(
    left: &View<L>,
    right: &View<R>,
    cursor: Option<Cursor>,
    limit: usize,
) -> Result<Page> {
    if limit == 0 || limit > 1000 {
        return Err(invalid("diff page size must be 1 to 1000"));
    }
    let mut cursor = cursor.unwrap_or_else(|| Cursor {
        left: left.revision,
        right: right.revision,
        frames: vec![Frame {
            name: String::new(),
            left: Some(left.projection.root),
            right: Some(right.projection.root),
            emitted: false,
            after: None,
        }],
        results_after: None,
        done: false,
    });
    if cursor.left != left.revision || cursor.right != right.revision {
        return Err(invalid("diff cursor belongs to different revisions"));
    }
    if cursor.frames.len() > 2048
        || cursor
            .frames
            .iter()
            .map(|frame| frame.name.len() + 1)
            .sum::<usize>()
            > 4097
    {
        return Err(invalid("diff cursor exceeds path budget"));
    }
    // resolve cursor paths from the supplied immutable roots; wire cursors never carry object locations.
    let mut expected_left = Some(left.projection.root);
    let mut expected_right = Some(right.projection.root);
    for (index, frame) in cursor.frames.iter_mut().enumerate() {
        if index > 0 {
            expected_left = node(left, expected_left)?
                .map(|parent| parent.child(&left.store, &frame.name))
                .transpose()?
                .flatten();
            expected_right = node(right, expected_right)?
                .map(|parent| parent.child(&right.store, &frame.name))
                .transpose()?
                .flatten();
        }
        frame.left = expected_left;
        frame.right = expected_right;
    }
    let mut changes = Vec::new();
    // bound work even when many distinct namespace nodes describe equal user-visible metadata.
    let mut remaining = 4096;
    while !cursor.frames.is_empty() && changes.len() < limit && remaining > 0 {
        remaining -= 1;
        let path = cursor
            .frames
            .iter()
            .filter(|frame| !frame.name.is_empty())
            .map(|frame| frame.name.as_str())
            .collect::<Vec<_>>()
            .join("/");
        let frame = cursor
            .frames
            .last_mut()
            .ok_or_else(|| invalid("missing diff frame"))?;
        if frame.left == frame.right {
            cursor.frames.pop();
            continue;
        }
        let a = node(left, frame.left)?;
        let b = node(right, frame.right)?;
        if !frame.emitted {
            frame.emitted = true;
            let before: Option<Inode> = a
                .as_ref()
                .map(|node| load(&left.store, node.inode_id, Kind::Inode))
                .transpose()?;
            let after: Option<Inode> = b
                .as_ref()
                .map(|node| load(&right.store, node.inode_id, Kind::Inode))
                .transpose()?;
            let same = match (&before, &after) {
                (Some(a), Some(b))
                    if matches!(a.data, InodeData::Directory(_))
                        && matches!(b.data, InodeData::Directory(_)) =>
                {
                    a.metadata == b.metadata
                }
                _ => before == after,
            };
            if !same {
                changes.push(Change {
                    path,
                    left: a.as_ref().map(|node| node.inode_id),
                    right: b.as_ref().map(|node| node.inode_id),
                    result: false,
                });
                continue;
            }
        }
        let a = radix::page(
            &left.store,
            a.and_then(|node| node.children),
            frame.after.as_deref(),
            1,
        )?
        .pop();
        let b = radix::page(
            &right.store,
            b.and_then(|node| node.children),
            frame.after.as_deref(),
            1,
        )?
        .pop();
        let name = match (&a, &b) {
            (Some(a), Some(b)) => a.0.clone().min(b.0.clone()),
            (Some(a), None) => a.0.clone(),
            (None, Some(b)) => b.0.clone(),
            (None, None) => {
                cursor.frames.pop();
                continue;
            }
        };
        frame.after = Some(name.clone());
        let next = Frame {
            name: String::from_utf8(name.clone()).map_err(|_| invalid("invalid diff name"))?,
            left: a.filter(|a| a.0 == name).map(|a| a.1),
            right: b.filter(|b| b.0 == name).map(|b| b.1),
            emitted: false,
            after: None,
        };
        cursor.frames.push(next);
    }
    while cursor.frames.is_empty() && !cursor.done && changes.len() < limit && remaining > 0 {
        remaining -= 1;
        if left.attachments == right.attachments {
            cursor.done = true;
            break;
        }
        let a = radix::page(
            &left.store,
            left.attachments,
            cursor.results_after.as_deref(),
            1,
        )?
        .pop();
        let b = radix::page(
            &right.store,
            right.attachments,
            cursor.results_after.as_deref(),
            1,
        )?
        .pop();
        let name = match (&a, &b) {
            (Some(a), Some(b)) => a.0.clone().min(b.0.clone()),
            (Some(a), None) => a.0.clone(),
            (None, Some(b)) => b.0.clone(),
            (None, None) => {
                cursor.done = true;
                break;
            }
        };
        cursor.results_after = Some(name.clone());
        let a = a.filter(|a| a.0 == name).map(|a| a.1);
        let b = b.filter(|b| b.0 == name).map(|b| b.1);
        if a == b {
            continue;
        }

        changes.push(Change {
            path: String::from_utf8(name).map_err(|_| invalid("invalid result name"))?,
            left: a,
            right: b,
            result: true,
        });
    }
    Ok(Page {
        changes,
        cursor: (!cursor.done).then_some(cursor),
    })
}

mod frame;
use frame::Frame;

mod cursor;
pub use cursor::Cursor;

mod compact_cursor;
use compact_cursor::CompactCursor;

mod change;
pub use change::Change;

mod page;
pub use page::Page;
