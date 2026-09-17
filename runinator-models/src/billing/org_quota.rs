#[allow(unused_imports)]
use super::*;

/// an org's spending/scale caps. a `0` in `max_nodes_per_kind` blocks that kind entirely; an absent
/// kind is unbounded on node count. `max_monthly_cents` of 0 means "no monthly budget cap".
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OrgQuota {
    pub org_id: Uuid,
    #[serde(default)]
    pub max_nodes_per_kind: BTreeMap<String, u32>,
    #[serde(default)]
    pub max_monthly_cents: u32,
}

impl OrgQuota {
    /// the node cap for a kind, or `None` when uncapped.
    pub fn max_nodes(&self, kind: ReplicaKind) -> Option<u32> {
        self.max_nodes_per_kind.get(kind.as_str()).copied()
    }
}
