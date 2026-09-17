#[allow(unused_imports)]
use super::*;

/// an org's rolled-up usage over a window: node-hours and accrued cost per (backend, kind).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OrgUsage {
    pub org_id: Uuid,
    pub since: Option<DateTime<Utc>>,
    pub node_hours: BTreeMap<String, f64>,
    pub accrued_cents: u64,
}
