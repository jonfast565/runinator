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

fn default_dedicated() -> bool {
    true
}

// ---- request/response DTOs ----

mod rate_entry;
pub use rate_entry::RateEntry;

mod rate_card;
pub use rate_card::RateCard;

mod org_quota;
pub use org_quota::OrgQuota;

mod org_resource_group;
pub use org_resource_group::OrgResourceGroup;

mod usage_sample;
pub use usage_sample::UsageSample;

mod org_usage;
pub use org_usage::OrgUsage;

mod scale_org_nodes_request;
pub use scale_org_nodes_request::ScaleOrgNodesRequest;

mod update_org_quota_request;
pub use update_org_quota_request::UpdateOrgQuotaRequest;

mod update_ai_rate_card_request;
pub use update_ai_rate_card_request::UpdateAiRateCardRequest;
