#[allow(unused_imports)]
use super::*;

/// an org's requested dedicated node allocation for a (backend, kind). the provisioner's aggregate
/// count for a kind is reconciled to the sum of these across orgs; per-org attribution and cost live
/// here. `dedicated=true` means nodes are labeled `org=<slug>` for that tenant.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OrgResourceGroup {
    pub org_id: Uuid,
    pub backend: ProvisionBackend,
    pub kind: ReplicaKind,
    pub desired: u32,
    #[serde(default = "default_dedicated")]
    pub dedicated: bool,
}
