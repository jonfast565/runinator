#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiUsageRecord {
    pub event_id: Uuid,
    pub effect_id: Uuid,
    pub workflow_run_id: Uuid,
    pub workflow_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    pub attempt: u32,
    pub provider: String,
    pub model: String,
    pub tokens: AiTokenUsage,
    #[serde(default)]
    pub cost_microusd: Option<u64>,
    #[serde(default)]
    pub cost_source: Option<AiCostSource>,
    pub recorded_at: DateTime<Utc>,
}
