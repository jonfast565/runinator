// per-org resource pricing (a rate card), spending quotas, and usage accounting. costs are tracked
// in integer cents to avoid floating-point drift; node-hours accrue from periodic sampling of the
// provisioner's live node counts (approximate, not exact cloud billing).

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::ai_usage::{AiRateEntry, AiTokenUsage};
use crate::provisioning::ProvisionBackend;
use crate::replicas::ReplicaKind;
use crate::validation::{Validate, ValidationError, identifier};

pub const AI_RATE_CARD_SCOPE: &str = "billing";
pub const AI_RATE_CARD_NAME: &str = "ai_rate_card";

/// one price line: what a single node of `kind` on `backend` costs per hour, in cents.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RateEntry {
    pub backend: ProvisionBackend,
    pub kind: ReplicaKind,
    pub hourly_cents: u32,
}

/// the platform-set price list. a missing (backend, kind) pair is treated as free (0 cents).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RateCard {
    pub entries: Vec<RateEntry>,
    #[serde(default)]
    pub ai_entries: Vec<AiRateEntry>,
}

impl RateCard {
    /// the hourly cents for a (backend, kind), or 0 when unpriced.
    pub fn hourly_cents(&self, backend: ProvisionBackend, kind: ReplicaKind) -> u32 {
        self.entries
            .iter()
            .find(|entry| entry.backend == backend && entry.kind == kind)
            .map(|entry| entry.hourly_cents)
            .unwrap_or(0)
    }

    pub fn ai_rate(&self, provider: &str, model: &str) -> Option<&AiRateEntry> {
        self.ai_entries
            .iter()
            .find(|entry| entry.provider == provider && entry.model == model)
            .or_else(|| {
                self.ai_entries
                    .iter()
                    .find(|entry| entry.provider == provider && entry.model == "*")
            })
    }

    pub fn price_ai_usage(
        &self,
        provider: &str,
        model: &str,
        tokens: &AiTokenUsage,
    ) -> Option<u64> {
        self.ai_rate(provider, model)
            .map(|entry| entry.price(tokens))
    }

    /// a conservative default price list so costs are non-zero out of the box.
    pub fn default_card() -> Self {
        let kinds = [
            ReplicaKind::Worker,
            ReplicaKind::Waker,
            ReplicaKind::Webservice,
            ReplicaKind::Postgres,
        ];
        let mut entries = Vec::new();
        for backend in [ProvisionBackend::Supervisor, ProvisionBackend::Kubernetes] {
            for kind in kinds {
                // workers are the costly compute; waker/webservice are lighter control-plane nodes.
                let hourly_cents = match kind {
                    ReplicaKind::Worker => 25,
                    ReplicaKind::Waker => 5,
                    ReplicaKind::Webservice => 10,
                    ReplicaKind::Postgres => 20,
                    // the archiver and engine worker are not provisioned/billed nodes; they only
                    // register for visibility.
                    ReplicaKind::Archiver | ReplicaKind::Background => 0,
                };
                entries.push(RateEntry {
                    backend,
                    kind,
                    hourly_cents,
                });
            }
        }
        Self {
            entries,
            ai_entries: Vec::new(),
        }
    }
}

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

fn default_dedicated() -> bool {
    true
}

/// a point-in-time sample of an org's running node count for a (backend, kind), used to accrue
/// node-hours between samples.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageSample {
    pub org_id: Uuid,
    pub backend: ProvisionBackend,
    pub kind: ReplicaKind,
    pub node_count: u32,
    pub sampled_at: DateTime<Utc>,
}

/// an org's rolled-up usage over a window: node-hours and accrued cost per (backend, kind).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OrgUsage {
    pub org_id: Uuid,
    pub since: Option<DateTime<Utc>>,
    pub node_hours: BTreeMap<String, f64>,
    pub accrued_cents: u64,
}

// ---- request/response DTOs ----

/// set the desired dedicated-node count for a kind in an org. `dedicated=false` reserved for future
/// shared-pool reservations; today only dedicated groups are provisioned per org.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScaleOrgNodesRequest {
    pub backend: ProvisionBackend,
    pub kind: ReplicaKind,
    pub desired: u32,
}

/// platform-admin quota update.
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateOrgQuotaRequest {
    #[serde(default)]
    pub max_nodes_per_kind: BTreeMap<String, u32>,
    #[serde(default)]
    pub max_monthly_cents: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateAiRateCardRequest {
    #[serde(default)]
    pub ai_entries: Vec<AiRateEntry>,
}

impl Validate for ScaleOrgNodesRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        Ok(())
    }
}

impl Validate for UpdateOrgQuotaRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        if self.max_nodes_per_kind.len() > 32 {
            return Err(ValidationError::new(
                "max_nodes_per_kind",
                "must contain at most 32 replica kinds",
            ));
        }
        for key in self.max_nodes_per_kind.keys() {
            identifier(&format!("max_nodes_per_kind.{key}"), key)?;
        }
        Ok(())
    }
}

impl Validate for UpdateAiRateCardRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        if self.ai_entries.len() > 256 {
            return Err(ValidationError::new(
                "ai_entries",
                "must contain at most 256 entries",
            ));
        }
        let mut seen = std::collections::BTreeSet::new();
        for (index, entry) in self.ai_entries.iter().enumerate() {
            identifier(&format!("ai_entries.{index}.provider"), &entry.provider)?;
            if entry.model.trim().is_empty() || entry.model.len() > 200 {
                return Err(ValidationError::new(
                    format!("ai_entries.{index}.model"),
                    "must contain 1 to 200 characters",
                ));
            }
            if !seen.insert((entry.provider.clone(), entry.model.clone())) {
                return Err(ValidationError::new(
                    format!("ai_entries.{index}"),
                    "duplicates an existing provider/model entry",
                ));
            }
        }
        Ok(())
    }
}
