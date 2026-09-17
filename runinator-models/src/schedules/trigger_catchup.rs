#[allow(unused_imports)]
use super::*;

/// a trigger's catch-up policy, read from its `configuration.catchup`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TriggerCatchup {
    #[serde(default)]
    pub policy: CatchupPolicy,
    /// lateness a `skip` policy tolerates. unused by the other policies.
    #[serde(default)]
    pub grace_seconds: Option<i64>,
    /// per-tick replay cap for `fire_all`. unused by the other policies.
    #[serde(default)]
    pub max_slots: Option<i64>,
}

impl Default for TriggerCatchup {
    fn default() -> Self {
        Self {
            policy: CatchupPolicy::FireOnce,
            grace_seconds: None,
            max_slots: None,
        }
    }
}

impl TriggerCatchup {
    pub fn grace(&self) -> i64 {
        self.grace_seconds
            .filter(|seconds| *seconds > 0)
            .unwrap_or(DEFAULT_CATCHUP_GRACE_SECONDS)
    }

    pub fn max_slots(&self) -> i64 {
        self.max_slots
            .filter(|slots| *slots > 0)
            .unwrap_or(DEFAULT_CATCHUP_MAX_SLOTS)
    }

    /// read the policy out of a trigger's `configuration` object. a `catchup` entry may be either
    /// the bare policy string (`"fire_all"`) or the full object.
    pub fn from_configuration(configuration: &Value) -> Self {
        let Some(catchup) = configuration.get("catchup") else {
            return Self::default();
        };
        if let Some(raw) = catchup.as_str() {
            return Self {
                policy: CatchupPolicy::from_str_opt(raw).unwrap_or_default(),
                ..Self::default()
            };
        }

        serde_json::from_value(catchup.clone().into()).unwrap_or_default()
    }
}
