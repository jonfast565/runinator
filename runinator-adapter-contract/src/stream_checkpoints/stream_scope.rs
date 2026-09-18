#[allow(unused_imports)]
use super::*;

use std::collections::BTreeSet;

/// One subject's declared streams within a [`StreamCheckpoints`]: the prefix its marks are stored
/// under, and which of them this pass seeded. A poll holds one scope per subject, so a kind that
/// walks several repositories keeps a single checkpoint across all of them.
pub struct StreamScope {
    prefix: String,
    seeded: BTreeSet<String>,
}

impl StreamScope {
    pub(super) fn new(prefix: &str) -> Self {
        Self {
            prefix: prefix.to_owned(),
            seeded: BTreeSet::new(),
        }
    }

    pub(super) fn seed(&mut self, stream: &str) {
        self.seeded.insert(stream.to_owned());
    }

    pub(super) fn key(&self, stream: &str) -> String {
        format!("{}:{}", self.prefix, stream)
    }

    /// Whether this pass established the stream's boundary rather than continuing it. A seeded
    /// stream has no history to enumerate, so its poller skips the fetch entirely.
    pub fn is_seeded(&self, stream: &str) -> bool {
        self.seeded.contains(stream)
    }
}
