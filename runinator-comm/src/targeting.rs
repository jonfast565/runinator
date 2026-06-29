use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// runtime routing key stamped on an [`crate::ActionCommand`] by the reducer. selects which
/// worker(s) may receive the action. `Any` preserves pre-targeting behavior, so existing serialized
/// commands (which carry no target) deserialize as `Any`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ActionTarget {
    /// any general-purpose (non-exclusive) worker.
    #[default]
    Any,
    /// any worker whose labels are a superset of `selector` (k8s nodeSelector style).
    Labels { selector: BTreeMap<String, String> },
    /// exactly one worker replica, identified by its replica id.
    Replica { replica_id: Uuid },
}

impl ActionTarget {
    /// build a label selector target from key/value pairs.
    pub fn labels(selector: impl IntoIterator<Item = (String, String)>) -> Self {
        Self::Labels {
            selector: selector.into_iter().collect(),
        }
    }

    /// true when a consumer presenting `profile` is allowed to receive an action carrying this
    /// target. this is the single matching predicate every routing backend defers to.
    pub fn matches(&self, profile: &ConsumerProfile) -> bool {
        match self {
            // an exclusive consumer (e.g. the desktop) never picks up general-pool work.
            ActionTarget::Any => !profile.exclusive,
            ActionTarget::Labels { selector } => selector
                .iter()
                .all(|(key, value)| profile.labels.get(key).is_some_and(|held| held == value)),
            ActionTarget::Replica { replica_id } => profile.replica_id == Some(*replica_id),
        }
    }
}

/// describes a consumer to the broker so it can route targeted action deliveries. supplied on the
/// targeting-aware `receive_for` path. a plain consumer is [`ConsumerProfile::shared`]; an exclusive
/// consumer never matches [`ActionTarget::Any`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsumerProfile {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replica_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
    #[serde(default)]
    pub exclusive: bool,
}

impl ConsumerProfile {
    /// a general-pool consumer: non-exclusive, unlabeled, no replica binding. matches `Any` and any
    /// label/replica target it happens to satisfy. this is the server-worker default.
    pub fn shared(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            replica_id: None,
            labels: BTreeMap::new(),
            exclusive: false,
        }
    }

    /// bind this consumer to a specific replica id (so it can receive `Replica`-targeted actions).
    pub fn with_replica_id(mut self, replica_id: Uuid) -> Self {
        self.replica_id = Some(replica_id);
        self
    }

    /// attach routing labels (so it can receive matching `Labels`-targeted actions).
    pub fn with_labels(mut self, labels: BTreeMap<String, String>) -> Self {
        self.labels = labels;
        self
    }

    /// mark this consumer exclusive: it never receives general-pool (`Any`) work, only `Replica`/
    /// `Labels` targets it satisfies. used by the desktop worker.
    pub fn exclusive(mut self) -> Self {
        self.exclusive = true;
        self
    }
}
