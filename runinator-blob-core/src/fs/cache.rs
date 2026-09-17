//! bounded metadata caching for the filesystem backend.

use std::collections::{HashMap, VecDeque};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{Arc, Mutex};

use crate::meta::ObjectMeta;

const SHARDS: usize = 16;

/// a small sharded approximate-lru whose byte accounting is derived from the serialized metadata.

fn cache_key(bucket: &str, key: &str) -> String {
    format!("{bucket}\0{key}")
}

fn shard_for(key: &str) -> usize {
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    hasher.finish() as usize % SHARDS
}

fn evict(shard: &mut Shard, capacity: usize) {
    while shard.used > capacity {
        let Some((key, generation)) = shard.order.pop_front() else {
            break;
        };
        let current = shard.entries.get(&key).map(|entry| entry.generation);
        if current != Some(generation) {
            continue;
        }
        if let Some(entry) = shard.entries.remove(&key) {
            shard.used = shard.used.saturating_sub(entry.weight);
        }
    }
    compact_order(shard);
}

fn compact_order(shard: &mut Shard) {
    if shard.order.len() <= shard.entries.len().saturating_mul(4).saturating_add(64) {
        return;
    }
    let mut current: Vec<(String, u64)> = shard
        .entries
        .iter()
        .map(|(key, entry)| (key.clone(), entry.generation))
        .collect();
    current.sort_by_key(|(_, generation)| *generation);
    shard.order = current.into();
}

mod entry;
use entry::Entry;

mod shard;
use shard::Shard;

mod metadata_cache;
pub(super) use metadata_cache::MetadataCache;
