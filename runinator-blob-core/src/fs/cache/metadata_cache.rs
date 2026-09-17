#[allow(unused_imports)]
use super::*;

pub(crate) struct MetadataCache {
    pub(super) shards: Vec<Mutex<Shard>>,
    pub(super) capacity_per_shard: usize,
}

impl MetadataCache {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            shards: (0..SHARDS).map(|_| Mutex::new(Shard::default())).collect(),
            capacity_per_shard: capacity / SHARDS,
        }
    }

    pub(crate) fn get(&self, bucket: &str, key: &str) -> Option<Arc<ObjectMeta>> {
        if self.capacity_per_shard == 0 {
            return None;
        }
        let cache_key = cache_key(bucket, key);
        let mut shard = self.shards[shard_for(&cache_key)].lock().ok()?;
        let meta = shard.entries.get(&cache_key)?.meta.clone();
        shard.generation = shard.generation.wrapping_add(1);
        let generation = shard.generation;
        if let Some(entry) = shard.entries.get_mut(&cache_key) {
            entry.generation = generation;
        }
        shard.order.push_back((cache_key, generation));
        compact_order(&mut shard);
        Some(meta)
    }

    pub(crate) fn insert(&self, bucket: &str, meta: ObjectMeta, encoded_len: usize) {
        if self.capacity_per_shard == 0 {
            return;
        }
        let cache_key = cache_key(bucket, &meta.key);
        let mut shard = match self.shards[shard_for(&cache_key)].lock() {
            Ok(shard) => shard,
            Err(_) => return,
        };
        if let Some(previous) = shard.entries.remove(&cache_key) {
            shard.used = shard.used.saturating_sub(previous.weight);
        }
        shard.generation = shard.generation.wrapping_add(1);
        let generation = shard.generation;
        let weight = encoded_len
            .saturating_add(cache_key.len())
            .saturating_add(128);
        if weight > self.capacity_per_shard {
            return;
        }
        shard.used = shard.used.saturating_add(weight);
        shard.entries.insert(
            cache_key.clone(),
            Entry {
                meta: Arc::new(meta),
                weight,
                generation,
            },
        );
        shard.order.push_back((cache_key, generation));
        evict(&mut shard, self.capacity_per_shard);
    }

    pub(crate) fn remove(&self, bucket: &str, key: &str) {
        let cache_key = cache_key(bucket, key);
        let Ok(mut shard) = self.shards[shard_for(&cache_key)].lock() else {
            return;
        };
        if let Some(previous) = shard.entries.remove(&cache_key) {
            shard.used = shard.used.saturating_sub(previous.weight);
        }
    }

    pub(crate) fn remove_bucket(&self, bucket: &str) {
        let prefix = format!("{bucket}\0");
        for shard in &self.shards {
            let Ok(mut shard) = shard.lock() else {
                continue;
            };
            let removed: Vec<String> = shard
                .entries
                .keys()
                .filter(|key| key.starts_with(&prefix))
                .cloned()
                .collect();
            for key in removed {
                if let Some(entry) = shard.entries.remove(&key) {
                    shard.used = shard.used.saturating_sub(entry.weight);
                }
            }
            compact_order(&mut shard);
        }
    }
}
