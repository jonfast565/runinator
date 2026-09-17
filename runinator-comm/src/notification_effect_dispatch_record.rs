#[allow(unused_imports)]
use super::*;

/// One leased external notification delivery. It deliberately uses the provider-effect envelope so
/// workers share the same provider runtime, while its receipt and settlement remain outside the
/// workflow VM's continuation/effect tables.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationEffectDispatchRecord {
    pub delivery_id: Uuid,
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
