//! Persistent path-compressed byte radix map. Insertion order does not change
//! the final root. A page map uses 8-byte big-endian page numbers; directories
//! use UTF-8 component names. Deletions collapse redundant internal nodes.
use crate::{
    Id,
    error::{Result, corrupt, invalid},
    model::{Kind, RadixNode},
    store::{ReadStore, WriteStore, load, load_many, save},
};
pub fn get<S: ReadStore + ?Sized>(
    s: &S,
    mut root: Option<Id>,
    mut key: &[u8],
) -> Result<Option<Id>> {
    while let Some(id) = root {
        let n: RadixNode = load(s, id, Kind::Radix)?;
        if !key.starts_with(&n.prefix) {
            return Ok(None);
        }
        key = &key[n.prefix.len()..];
        if key.is_empty() {
            return Ok(n.value);
        }
        root = n.children.get(&key[0]).copied();
        key = &key[1..];
    }
    Ok(None)
}

/// Return at most `limit` keys strictly after a key in byte order.
/// The caller binds the cursor to its immutable root and requests one extra key for continuation.
pub fn page<S: ReadStore + ?Sized>(
    store: &S,
    root: Option<Id>,
    after: Option<&[u8]>,
    limit: usize,
) -> Result<Vec<(Vec<u8>, Id)>> {
    if limit == 0 || limit > 1001 {
        return Err(invalid("page size must be between 1 and 1001"));
    }
    if after.is_some_and(|key| key.len() > 4096) {
        return Err(invalid("cursor key too long"));
    }
    fn preload<S: ReadStore + ?Sized>(
        store: &S,
        root: Id,
        after: Option<&[u8]>,
        budget: usize,
    ) -> Result<std::collections::HashMap<Id, RadixNode>> {
        let mut loaded = std::collections::HashMap::new();
        let mut frontier = vec![(root, Vec::new())];
        let mut processed = 0;
        while !frontier.is_empty() && processed < budget {
            let take = frontier.len().min(budget - processed);
            let batch = frontier.drain(..take).collect::<Vec<_>>();
            processed += batch.len();
            let nodes: Vec<RadixNode> = load_many(
                store,
                &batch.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
                Kind::Radix,
            )?;
            for ((id, mut prefix), node) in batch.into_iter().zip(nodes) {
                loaded.entry(id).or_insert_with(|| node.clone());
                prefix.extend(&node.prefix);
                if prefix.len() > 4096 {
                    return Err(corrupt("radix path too long"));
                }
                if after.is_some_and(|key| prefix.as_slice() < key && !key.starts_with(&prefix)) {
                    continue;
                }
                for (edge, child) in node.children {
                    let mut child_prefix = prefix.clone();
                    child_prefix.push(edge);
                    frontier.push((child, child_prefix));
                }
            }
        }
        Ok(loaded)
    }
    fn walk<S: ReadStore + ?Sized>(
        store: &S,
        node: RadixNode,
        loaded: &mut std::collections::HashMap<Id, RadixNode>,
        prefix: &mut Vec<u8>,
        after: Option<&[u8]>,
        limit: usize,
        output: &mut Vec<(Vec<u8>, Id)>,
    ) -> Result<()> {
        let base = prefix.len();
        prefix.extend(&node.prefix);
        if prefix.len() > 4096 {
            return Err(corrupt("radix path too long"));
        }
        // all descendants share this prefix, so an earlier, non-prefix subtree can be skipped.
        if after.is_some_and(|key| prefix.as_slice() < key && !key.starts_with(prefix)) {
            prefix.truncate(base);
            return Ok(());
        }
        if let Some(value) = node.value
            && after.is_none_or(|key| prefix.as_slice() > key)
        {
            output.push((prefix.clone(), value));
        }
        if output.len() >= limit {
            prefix.truncate(base);
            return Ok(());
        }
        let children = node.children.into_iter().collect::<Vec<_>>();
        let missing = children
            .iter()
            .map(|(_, id)| *id)
            .filter(|id| !loaded.contains_key(id))
            .collect::<Vec<_>>();
        for (id, node) in
            missing
                .iter()
                .copied()
                .zip(load_many::<RadixNode, _>(store, &missing, Kind::Radix)?)
        {
            loaded.insert(id, node);
        }
        let nodes = children
            .iter()
            .map(|(_, id)| {
                loaded
                    .get(id)
                    .cloned()
                    .ok_or_else(|| corrupt("missing prefetched radix child"))
            })
            .collect::<Result<Vec<_>>>()?;
        for ((edge, _), node) in children.into_iter().zip(nodes) {
            if output.len() >= limit {
                break;
            }
            prefix.push(edge);
            walk(store, node, loaded, prefix, after, limit, output)?;
            prefix.pop();
        }
        prefix.truncate(base);
        Ok(())
    }
    let mut output = Vec::new();
    if let Some(id) = root {
        let budget = limit.saturating_mul(4).clamp(32, 8192);
        let mut loaded = preload(store, id, after, budget)?;
        let node = loaded
            .remove(&id)
            .ok_or_else(|| corrupt("missing prefetched radix root"))?;
        walk(
            store,
            node,
            &mut loaded,
            &mut Vec::new(),
            after,
            limit,
            &mut output,
        )?;
    }
    Ok(output)
}
fn canonical<S: WriteStore + ?Sized>(s: &S, mut n: RadixNode) -> Result<Option<Id>> {
    if n.value.is_none() && n.children.is_empty() {
        return Ok(None);
    }
    if n.value.is_none() && n.children.len() == 1 {
        let (&edge, &child) = n.children.iter().next().unwrap();
        let child: RadixNode = load(s, child, Kind::Radix)?;
        n.prefix.push(edge);
        n.prefix.extend(child.prefix);
        n.value = child.value;
        n.children = child.children;
    }
    Ok(Some(save(s, Kind::Radix, &n)?))
}
pub fn set<S: WriteStore + ?Sized>(
    s: &S,
    root: Option<Id>,
    key: &[u8],
    value: Option<Id>,
) -> Result<Option<Id>> {
    if key.len() > 4096 {
        return Err(invalid("radix key too long"));
    }
    update(s, root, key, value, 0)
}
fn update<S: WriteStore + ?Sized>(
    s: &S,
    root: Option<Id>,
    key: &[u8],
    value: Option<Id>,
    depth: usize,
) -> Result<Option<Id>> {
    if depth > 4096 {
        return Err(corrupt("radix depth limit"));
    }
    let Some(id) = root else {
        return match value {
            None => Ok(None),
            Some(v) => canonical(
                s,
                RadixNode {
                    prefix: key.to_vec(),
                    value: Some(v),
                    ..Default::default()
                },
            ),
        };
    };
    let mut n: RadixNode = load(s, id, Kind::Radix)?;
    let common = n.prefix.iter().zip(key).take_while(|(a, b)| a == b).count();
    if common < n.prefix.len() {
        if value.is_none() {
            return Ok(root);
        }
        let mut parent = RadixNode {
            prefix: n.prefix[..common].to_vec(),
            ..Default::default()
        };
        let old_edge = n.prefix[common];
        n.prefix = n.prefix[common + 1..].to_vec();
        parent.children.insert(old_edge, canonical(s, n)?.unwrap());
        if common == key.len() {
            parent.value = value;
        } else {
            let leaf = RadixNode {
                prefix: key[common + 1..].to_vec(),
                value,
                ..Default::default()
            };
            parent
                .children
                .insert(key[common], canonical(s, leaf)?.unwrap());
        }
        return canonical(s, parent);
    }
    let suffix = &key[common..];
    if suffix.is_empty() {
        if n.value == value {
            return Ok(root);
        }
        n.value = value;
        return canonical(s, n);
    }
    let edge = suffix[0];
    let old = n.children.get(&edge).copied();
    let new = update(s, old, &suffix[1..], value, depth + 1)?;
    if new == old {
        return Ok(root);
    }
    match new {
        Some(id) => {
            n.children.insert(edge, id);
        }
        None => {
            n.children.remove(&edge);
        }
    }
    canonical(s, n)
}
/// Streaming ordered traversal; callback controls output allocation.
pub fn visit<S: ReadStore + ?Sized, F: FnMut(&[u8], Id) -> Result<()>>(
    s: &S,
    root: Option<Id>,
    f: &mut F,
) -> Result<()> {
    fn walk<S: ReadStore + ?Sized, F: FnMut(&[u8], Id) -> Result<()>>(
        s: &S,
        id: Id,
        prefix: &mut Vec<u8>,
        f: &mut F,
    ) -> Result<()> {
        let base = prefix.len();
        let n: RadixNode = load(s, id, Kind::Radix)?;
        prefix.extend(n.prefix);
        if prefix.len() > 4096 {
            return Err(corrupt("radix path too long"));
        }
        if let Some(v) = n.value {
            f(prefix, v)?;
        }
        for (edge, child) in n.children {
            prefix.push(edge);
            walk(s, child, prefix, f)?;
            prefix.pop();
        }
        prefix.truncate(base);
        Ok(())
    }
    if let Some(root) = root {
        walk(s, root, &mut Vec::new(), f)?;
    }
    Ok(())
}
/// Drop an ordered key interval without iterating absent pages. Entire covered
/// subtrees are discarded; only boundary paths are rewritten.
pub fn remove_range<S: WriteStore + ?Sized>(
    s: &S,
    root: Option<Id>,
    start: &[u8],
    end: Option<&[u8]>,
) -> Result<Option<Id>> {
    if end.is_some_and(|e| e <= start) {
        return Ok(root);
    }
    fn next_prefix(mut p: Vec<u8>) -> Option<Vec<u8>> {
        while let Some(x) = p.pop() {
            if x != 255 {
                p.push(x + 1);
                return Some(p);
            }
        }
        None
    }
    fn walk<S: WriteStore + ?Sized>(
        s: &S,
        root: Option<Id>,
        base: &[u8],
        start: &[u8],
        end: Option<&[u8]>,
    ) -> Result<Option<Id>> {
        let Some(id) = root else { return Ok(None) };
        let mut n: RadixNode = load(s, id, Kind::Radix)?;
        let mut p = base.to_vec();
        p.extend(&n.prefix);
        if p.len() > 4096 {
            return Err(corrupt("radix path too long"));
        }
        let hi = next_prefix(p.clone());
        if end.is_some_and(|e| p.as_slice() >= e)
            || hi.as_ref().is_some_and(|h| h.as_slice() <= start)
        {
            return Ok(root);
        }
        if p.as_slice() >= start
            && (end.is_none() || hi.as_ref().is_some_and(|h| h.as_slice() <= end.unwrap()))
        {
            return Ok(None);
        }
        let mut changed = false;
        if p.as_slice() >= start && end.is_none_or(|e| p.as_slice() < e) && n.value.is_some() {
            n.value = None;
            changed = true;
        }
        let children = n.children.clone();
        for (edge, child) in children {
            p.push(edge);
            let replacement = walk(s, Some(child), &p, start, end)?;
            p.pop();
            if replacement == Some(child) {
                continue;
            }
            changed = true;
            match replacement {
                Some(id) => {
                    n.children.insert(edge, id);
                }
                None => {
                    n.children.remove(&edge);
                }
            }
        }
        if !changed {
            return Ok(root);
        }
        canonical(s, n)
    }
    walk(s, root, &[], start, end)
}
