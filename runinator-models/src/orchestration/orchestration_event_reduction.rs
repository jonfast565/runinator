#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestrationEventReduction {
    pub id: Uuid,
    pub binding_id: Uuid,
    pub inbox_event_id: Uuid,
    pub sequence: i64,
    #[serde(default)]
    pub matched_intents: Vec<String>,
    #[serde(default)]
    pub winner: Option<String>,
    #[serde(default)]
    pub suppressed_intents: Vec<String>,
    pub binding_version: i64,
    pub disposition: String,
    #[serde(default)]
    pub detail: Value,
    pub created_at: DateTime<Utc>,
}
