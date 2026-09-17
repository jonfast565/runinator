#[allow(unused_imports)]
use super::*;

/// one price line: what a single node of `kind` on `backend` costs per hour, in cents.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RateEntry {
    pub backend: ProvisionBackend,
    pub kind: ReplicaKind,
    pub hourly_cents: u32,
}
