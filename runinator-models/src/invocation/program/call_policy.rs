#[allow(unused_imports)]
use super::*;

/// per-call overrides for the enclosing node's policy.
///
/// every field is optional because the node supplies the defaults; a `with { … }` postfix only says
/// what differs.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct CallPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry: Option<CallRetry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// an expression, resolved against the run context at dispatch — not a literal, because the key
    /// usually names something about the run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<Value>,
}

impl CallPolicy {
    /// whether this policy says nothing at all.
    pub fn is_empty(&self) -> bool {
        self.timeout_seconds.is_none()
            && self.retry.is_none()
            && self.runner.is_none()
            && self.tags.is_empty()
            && self.idempotency_key.is_none()
    }

    /// overlay `self` onto `base`, field by field: a call-site value wins, an absent one inherits.
    pub fn overlay(&self, base: &CallPolicy) -> CallPolicy {
        CallPolicy {
            timeout_seconds: self.timeout_seconds.or(base.timeout_seconds),
            retry: self.retry.clone().or_else(|| base.retry.clone()),
            runner: self.runner.clone().or_else(|| base.runner.clone()),
            tags: if self.tags.is_empty() {
                base.tags.clone()
            } else {
                self.tags.clone()
            },
            idempotency_key: self
                .idempotency_key
                .clone()
                .or_else(|| base.idempotency_key.clone()),
        }
    }
}
