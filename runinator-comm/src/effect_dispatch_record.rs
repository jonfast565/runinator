#[allow(unused_imports)]
use super::*;

/// One leased entry in the VM effect-delivery outbox.
///
/// This deliberately carries the complete frozen command: a publisher must never reconstruct an
/// effect from mutable workflow state after the receipt was committed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectDispatchRecord {
    pub id: Uuid,
    pub effect_id: Uuid,
    pub dedupe_key: String,
    pub command: EffectCommand,
    pub attempts: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_until: Option<DateTime<Utc>>,
}
