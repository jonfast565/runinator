#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestrationPendingIntent {
    pub id: Uuid,
    pub binding_id: Uuid,
    pub intent: String,
    pub priority: i32,
    pub source_event_ids: Vec<Uuid>,
    #[serde(default)]
    pub latest_payload: Value,
    pub wake_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
