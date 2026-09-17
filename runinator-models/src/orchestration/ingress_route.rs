#[allow(unused_imports)]
use super::*;

/// One static event-type route in a workflow or pipeline ingress policy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IngressRoute {
    pub event_type: String,
    pub lifecycle: IngressLifecycle,
    pub action: IngressAction,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub predicates: Vec<IngressPredicate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<String>,
}
