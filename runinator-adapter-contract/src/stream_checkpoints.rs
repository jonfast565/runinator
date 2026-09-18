#[allow(unused_imports)]
use super::*;

use std::collections::BTreeMap;

/// The per-stream high-water marks a polling adapter carries in [`AdapterPollRequest::checkpoint`].
///
/// Marks are per stream rather than per adapter: an adapter's event kinds advance independently,
/// and a single shared mark would let a busy stream drag the watermark past events a quiet one had
/// not emitted yet.
///
/// Declaring a subject's streams up front is what makes adding one safe. A declared stream with no
/// mark — a first poll, or a kind added to an adapter that has been running for weeks —
/// establishes its boundary at the current instant and reports itself as seeded, so the pass that
/// introduces it skips the stream instead of replaying the source's entire history. Leaving that
/// to each poller is how the GitHub kind came to do it and the Jira kind did not.
pub struct StreamCheckpoints {
    initialize: bool,
    /// the marks the poll arrived with; reads come from here so a stream seeded in this pass reads
    /// as unbounded rather than as bounded by the instant it was seeded at.
    previous: BTreeMap<String, String>,
    /// the flat `{"updated_at": ...}` form predating per-stream marks. it stands in for every
    /// stream, so an adapter written before this scheme keeps its position instead of starting cold.
    legacy: Option<String>,
    /// the marks the poll will return. it starts from `previous` because a stream that reports
    /// nothing new must hold its position; rebuilding from only the streams that produced events
    /// would reset the quiet ones to a cold start on the very next poll.
    marks: BTreeMap<String, String>,
}

impl StreamCheckpoints {
    /// Open the checkpoint a poll carries.
    pub fn open(request: &AdapterPollRequest) -> Self {
        let previous: BTreeMap<String, String> = request
            .checkpoint
            .get("streams")
            .and_then(Value::as_object)
            .map(|streams| {
                streams
                    .iter()
                    .filter_map(|(name, value)| {
                        value.as_str().map(|value| (name.clone(), value.to_owned()))
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self {
            initialize: request.initialize,
            legacy: request
                .checkpoint
                .get("updated_at")
                .and_then(Value::as_str)
                .map(str::to_owned),
            marks: previous.clone(),
            previous,
        }
    }

    /// Declare the streams a subject contributes. `prefix` scopes the marks to that subject — a
    /// repository, a Jira instance — and `now` is the boundary a stream with no mark starts from.
    pub fn declare(&mut self, prefix: &str, streams: &[&str], now: &str) -> StreamScope {
        let mut scope = StreamScope::new(prefix);
        for stream in streams {
            let key = format!("{prefix}:{stream}");
            if !self.initialize && (self.previous.contains_key(&key) || self.legacy.is_some()) {
                continue;
            }
            self.marks.insert(key, now.to_owned());
            scope.seed(stream);
        }
        scope
    }

    /// The mark a stream's poll starts from, or `None` for a stream this pass seeded.
    pub fn mark(&self, scope: &StreamScope, stream: &str) -> Option<String> {
        if scope.is_seeded(stream) {
            return None;
        }
        self.previous
            .get(&scope.key(stream))
            .cloned()
            .or_else(|| self.legacy.clone())
    }

    /// Advance a stream's mark for an event that was actually emitted. Advancing before the caller
    /// decides to skip a malformed item would carry the watermark past events that were never
    /// reported, and they would never be enumerated again.
    pub fn advance(&mut self, scope: &StreamScope, stream: &str, stamp: &str) {
        if stamp.is_empty() {
            return;
        }
        let entry = self.marks.entry(scope.key(stream)).or_default();
        if stamp > entry.as_str() {
            *entry = stamp.to_owned();
        }
    }

    /// The checkpoint to return with the batch.
    pub fn into_checkpoint(self) -> Value {
        serde_json::json!({ "streams": self.marks })
    }
}

mod stream_scope;
pub use stream_scope::StreamScope;

#[cfg(test)]
#[path = "stream_checkpoints_tests.rs"]
mod stream_checkpoints_tests;
